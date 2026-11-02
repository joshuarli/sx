#![forbid(unsafe_code)]

mod serve;

use std::io::Write;
use std::process::ExitCode;

use sx_kernel::registry::Registry;

const EXIT_UNUSABLE_ARGUMENT: u8 = 64;
const EXIT_NO_WORKER: u8 = 70;
const EXIT_WRITE_FAILED: u8 = 74;

fn main() -> ExitCode {
    let mut args = Vec::new();
    for arg in std::env::args_os().skip(1) {
        match arg.into_string() {
            Ok(arg) => args.push(arg),
            Err(arg) => {
                eprintln!("sx: argument {arg:?} is not valid UTF-8");
                return ExitCode::from(EXIT_UNUSABLE_ARGUMENT);
            }
        }
    }
    if let Some(("serve", rest)) = args.split_first().map(|(first, rest)| (first.as_str(), rest)) {
        return serve::main(rest);
    }
    let exit = match sx_kernel::stack::on_sized_stack(|| sx_cli::run(&args, &Registry::new)) {
        Ok(exit) => exit,
        Err(error) => {
            eprintln!("sx: cannot start the worker thread: {error}");
            return ExitCode::from(EXIT_NO_WORKER);
        }
    };
    let written = std::io::stdout()
        .write_all(exit.stdout.as_bytes())
        .and_then(|()| std::io::stdout().flush())
        .and_then(|()| std::io::stderr().write_all(exit.stderr.as_bytes()));
    if let Err(error) = written {
        eprintln!("sx: cannot write the output: {error}");
        return ExitCode::from(EXIT_WRITE_FAILED);
    }
    ExitCode::from(u8::try_from(exit.code).unwrap_or(1))
}
