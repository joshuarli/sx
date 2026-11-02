#![forbid(unsafe_code)]

mod serve;

use std::io::Write;
use std::process::ExitCode;

use sx_kernel::registry::Registry;

fn main() -> ExitCode {
    let mut args = Vec::new();
    for arg in std::env::args_os().skip(1) {
        match arg.into_string() {
            Ok(arg) => args.push(arg),
            Err(arg) => {
                eprintln!("sx: argument {arg:?} is not valid UTF-8");
                return ExitCode::from(64);
            }
        }
    }
    if args.first().is_some_and(|arg| arg == "serve") {
        return serve::main(&args[1..]);
    }
    let exit = sx_cli::run(&args, &Registry::new);
    let _ = std::io::stdout().write_all(exit.stdout.as_bytes());
    let _ = std::io::stderr().write_all(exit.stderr.as_bytes());
    ExitCode::from(u8::try_from(exit.code).unwrap_or(1))
}
