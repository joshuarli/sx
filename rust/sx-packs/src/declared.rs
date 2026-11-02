use sx_kernel::json::Json;
use sx_kernel::registry::{EndpointBinder, Registry};

use crate::http::{EndpointLoader, Endpoints};
use crate::sqlite::{StatementLoader, Statements};

#[derive(Clone, Debug, Default)]
pub struct Declared {
    pub endpoints: Endpoints,
    pub statements: Statements,
}

#[derive(Debug, Default)]
pub struct DeclaredLoader {
    endpoints: EndpointLoader,
    statements: StatementLoader,
}

impl DeclaredLoader {
    pub fn new() -> DeclaredLoader {
        DeclaredLoader::default()
    }

    pub fn finish(self) -> Declared {
        Declared { endpoints: self.endpoints.finish(), statements: self.statements.finish() }
    }
}

impl EndpointBinder for DeclaredLoader {
    fn bind(&mut self, registry: &mut Registry, entry: &Json) -> Result<(), String> {
        if entry.get("sqlite").is_some() {
            self.statements.bind(registry, entry)
        } else {
            self.endpoints.bind(registry, entry)
        }
    }
}
