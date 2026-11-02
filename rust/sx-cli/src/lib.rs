#![forbid(unsafe_code)]

mod argparse;
mod commands;
mod helptext;
mod native;
mod pyfile;
mod pyjson;

use sx_kernel::registry::Registry;

pub const EXIT_OK: i32 = 0;
pub const EXIT_REJECTED: i32 = 1;
pub const EXIT_FAILED: i32 = 2;
pub const EXIT_USAGE: i32 = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exit {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub fn run(args: &[String], new_registry: &dyn Fn() -> Registry) -> Exit {
    match argparse::parse(args) {
        argparse::Parsed::Done(exit) => exit,
        argparse::Parsed::Invoke(invocation) => commands::execute(invocation, new_registry()),
    }
}
