use gleamc::codegen::RUNTIME_HEADER;
use gleamc::{translate_file_with_externals, write_c_project_file_with_externals, CTranslation};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
usage: gleamc <input.gleam> [options]

Compiles a Gleam program to a native binary using cc. The generated C files
are written to a staging directory (default: gleam-out) and removed again
after a successful compile.

options:
  -o <dir>   staging/output directory (default: gleam-out)
  -c         emit C without compiling (foreign sources are linked separately)
  -release   compile with optimizations (-O3, LTO, section garbage collection)
";

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(input) = args.next() else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    let mut output = PathBuf::from("gleam-out");
    let mut codegen_only = false;
    let mut release = false;
    let mut pending_output = false;
    for arg in args {
        if pending_output {
            output = PathBuf::from(arg);
            pending_output = false;
            continue;
        }
        match arg.as_str() {
            "-o" => pending_output = true,
            "-c" => codegen_only = true,
            "-release" => release = true,
            _ => {
                eprint!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    if pending_output {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    }

    if codegen_only {
        let result = translate_file_with_externals(&input, true)
            .map_err(|error| error.to_string())
            .and_then(|translation| {
                emit_c(Path::new(&input), &output, &translation).map_err(|error| error.to_string())
            });
        return match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        };
    }

    // Default: generate C, compile it with cc, then clean up the staging
    // directory.
    let output = match staging_directory(&output) {
        Ok(output) => output,
        Err(error) => {
            eprintln!("{}: {error}", output.display());
            return ExitCode::FAILURE;
        }
    };
    let native_sources = match write_c_project_file_with_externals(&input, &output) {
        Ok(sources) => sources,
        Err(error) => {
            let _ = fs::remove_dir(&output);
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };

    let binary = Path::new("program");
    let mut command = c_compiler(&output, release);
    command
        .arg(output.join("main.c"))
        .arg(output.join("gleam_runtime.c"))
        .args(&native_sources)
        .arg("-o")
        .arg(binary);
    let compile = command.output();

    match compile {
        Ok(result) if result.status.success() => {
            if let Err(error) = remove_staging_files(&output) {
                eprintln!("note: could not remove {}: {error}", output.display());
            } else if release {
                println!(
                    "compiled {} -> {} (release build, removed {})",
                    input,
                    binary.display(),
                    output.display()
                );
            } else {
                println!(
                    "compiled {} -> {} (removed {})",
                    input,
                    binary.display(),
                    output.display()
                );
            }
            ExitCode::SUCCESS
        }
        Ok(result) => {
            eprintln!(
                "{input}: cc failed:\n{}",
                String::from_utf8_lossy(&result.stderr)
            );
            eprintln!(
                "C files kept in {} (compile with: {})",
                output.display(),
                display_command(&command)
            );
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("{input}: failed to run cc: {error}");
            eprintln!("C files kept in {}", output.display());
            ExitCode::FAILURE
        }
    }
}

fn emit_c(input: &Path, output: &Path, translation: &CTranslation) -> io::Result<()> {
    let stem = input
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("main");
    let c_path = output.join(format!("{stem}.c"));
    if c_path
        .canonicalize()
        .is_ok_and(|path| translation.native_sources.contains(&path))
    {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "generated C would overwrite external source `{}`",
                c_path.display()
            ),
        ));
    }
    fs::create_dir_all(output)?;
    if !translation.native_sources.is_empty() {
        write_runtime_header(&output.join("gleam_runtime.h"))?;
    }
    fs::write(&c_path, &translation.source)?;

    let mut command = c_compiler(output, false);
    command
        .arg(&c_path)
        .args(&translation.native_sources)
        .args(["-o", "program"]);
    println!(
        "wrote {} (compile with: {})",
        c_path.display(),
        display_command(&command)
    );
    Ok(())
}

fn write_runtime_header(path: &Path) -> io::Result<()> {
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => file.write_all(RUNTIME_HEADER.as_bytes()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if fs::read(path)? == RUNTIME_HEADER.as_bytes() {
                return Ok(());
            }
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "output header `{}` already exists with different contents",
                    path.display()
                ),
            ))
        }
        Err(error) => Err(error),
    }
}

fn c_compiler(include_dir: &Path, release: bool) -> Command {
    let mut command = Command::new("cc");
    command
        .args(["-std=c11", "-Wall", "-Wextra", "-x", "c", "-I"])
        .arg(include_dir);
    if release {
        command.args([
            "-O3",
            "-flto",
            "-ffunction-sections",
            "-fdata-sections",
            "-Wl,--gc-sections",
            "-DNDEBUG",
        ]);
    }
    command
}

fn display_command(command: &Command) -> String {
    std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|arg| {
            let arg = arg.to_string_lossy();
            if !arg.is_empty()
                && arg
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-_./=,".contains(&byte))
            {
                arg.into_owned()
            } else {
                format!("'{}'", arg.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn staging_directory(output: &Path) -> io::Result<PathBuf> {
    match fs::create_dir(output) {
        Ok(()) => return Ok(output.to_owned()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && output.is_dir() => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(output)?;
            return Ok(output.to_owned());
        }
        Err(error) => return Err(error),
    }

    for suffix in 0.. {
        let staging = output.join(format!("gleamc-{}-{suffix}", std::process::id()));
        match fs::create_dir(&staging) {
            Ok(()) => return Ok(staging),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    unreachable!("unique staging directory")
}

fn remove_staging_files(output: &Path) -> io::Result<()> {
    for file in ["main.c", "gleam_runtime.c", "gleam_runtime.h"] {
        fs::remove_file(output.join(file))?;
    }
    fs::remove_dir(output)
}
