#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    eprintln!("sx: the Rust command line is not built yet; use the Python sx");
    ExitCode::from(2)
}
