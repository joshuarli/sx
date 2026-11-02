#![forbid(unsafe_code)]

mod serve;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some((command, rest)) = args.split_first()
        && command == "serve"
    {
        return serve::main(rest);
    }
    eprintln!("sx: the Rust command line is not built yet; use the Python sx");
    ExitCode::from(2)
}
