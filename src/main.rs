use gleam_to_c::write_c_project;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(input) = args.next() else {
        eprintln!("usage: gleamc <input.gleam> [-o <output-directory>]");
        return ExitCode::from(2);
    };
    let output = match (args.next().as_deref(), args.next()) {
        (Some("-o"), Some(directory)) => PathBuf::from(directory),
        (None, None) => PathBuf::from("gleam-out"),
        _ => {
            eprintln!("usage: gleamc <input.gleam> [-o <output-directory>]");
            return ExitCode::from(2);
        }
    };
    if args.next().is_some() {
        eprintln!("usage: gleamc <input.gleam> [-o <output-directory>]");
        return ExitCode::from(2);
    }

    let result = fs::read_to_string(&input)
        .map_err(|error| error.to_string())
        .and_then(|source| {
            let module = gleam_to_c::parse(&source).map_err(|error| error.to_string())?;
            write_c_project(&source, &module, &output).map_err(|error| error.to_string())
        });
    match result {
        Ok(()) => {
            println!(
                "generated C project in {} (compile with: gcc -Wall -Wextra gleam-out/main.c gleam-out/gleam_runtime.c -o program)",
                output.display()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{input}: {error}");
            ExitCode::FAILURE
        }
    }
}
