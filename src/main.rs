use gleamc::{translate_standalone, write_c_project};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
usage: gleamc <input.gleam> [options]

Compiles a Gleam program to a native binary using cc. The generated C files
are written to a staging directory (default: gleam-out) and removed again
after a successful compile.

options:
  -o <dir>   staging/output directory (default: gleam-out)
  -c         write a single self-contained C file and skip compilation
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

    let source = match fs::read_to_string(&input) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("{input}: {error}");
            return ExitCode::FAILURE;
        }
    };

    if codegen_only {
        // Emit one self-contained C file that can be compiled manually.
        let stem = Path::new(&input)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("main");
        let c_path = output.join(format!("{stem}.c"));
        let result = translate_standalone(&source)
            .map_err(|error| error.to_string())
            .and_then(|c_source| {
                fs::create_dir_all(&output).map_err(|error| error.to_string())?;
                fs::write(&c_path, c_source).map_err(|error| error.to_string())
            });
        return match result {
            Ok(()) => {
                println!(
                    "wrote {} (compile with: cc -std=c11 {} -o program)",
                    c_path.display(),
                    c_path.display()
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{input}: {error}");
                ExitCode::FAILURE
            }
        };
    }

    // Default: generate C, compile it with cc, then clean up the staging
    // directory.
    let module = match gleamc::parse(&source) {
        Ok(module) => module,
        Err(error) => {
            eprintln!("{input}: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = write_c_project(&source, &module, &output) {
        eprintln!("{input}: {error}");
        return ExitCode::FAILURE;
    }

    let mut cc_args: Vec<&str> = vec!["-std=c11", "-Wall", "-Wextra"];
    if release {
        cc_args.extend([
            "-O3",
            "-flto",
            "-ffunction-sections",
            "-fdata-sections",
            "-Wl,--gc-sections",
            "-DNDEBUG",
        ]);
    }

    let binary = PathBuf::from("program");
    let compile = Command::new("cc")
        .args(&cc_args)
        .arg(output.join("main.c"))
        .arg(output.join("gleam_runtime.c"))
        .arg("-o")
        .arg(&binary)
        .output();

    match compile {
        Ok(result) if result.status.success() => {
            if let Err(error) = fs::remove_dir_all(&output) {
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
                "C files kept in {} (compile with: cc -std=c11 {}/main.c {}/gleam_runtime.c -o program)",
                output.display(),
                output.display(),
                output.display()
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
