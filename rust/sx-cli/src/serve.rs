use std::io;
use std::process::ExitCode;

use sx_cores::kernel::{Cores, core};
use sx_kernel::machine::CoreImplementations;
use sx_kernel::protocol::{CoreLibrary, Ending, serve};
use sx_kernel::registry::{Core, EndpointBinder, Pack};
use sx_packs::DeclaredLoader;

struct Library;

impl CoreLibrary for Library {
    fn core(&self, id: &str) -> Option<Core> {
        core(id)
    }

    fn pack(&self, id: &str) -> Option<Pack> {
        sx_packs::pack(id)
    }

    fn implementations(&self) -> &dyn CoreImplementations {
        &Cores
    }

    fn endpoints(&self) -> Option<Box<dyn EndpointBinder>> {
        Some(Box::new(DeclaredLoader::new()))
    }
}

pub fn main(args: &[String]) -> ExitCode {
    if let Some(extra) = args.first() {
        eprintln!("sx serve: unexpected argument {extra:?}; the host protocol is spoken on standard input and output");
        return ExitCode::from(2);
    }
    match serve(io::stdin().lock(), io::stdout().lock(), &Library) {
        Ok(Ending::Idle) => ExitCode::SUCCESS,
        Ok(Ending::Abandoned) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("sx serve: {error}");
            ExitCode::FAILURE
        }
    }
}
