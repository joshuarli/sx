#![deny(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

mod declaration;
pub mod declared;
pub mod fs;
pub mod guide;
pub mod http;
pub mod mcp;
pub mod process;
pub mod sqlite;

use std::fmt;

use sx_kernel::registry::Pack;
use sx_kernel::types::CallErrorKind;
use sx_kernel::values::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub kind: CallErrorKind,
    pub message: String,
}

impl Failure {
    pub fn new(kind: CallErrorKind, message: impl Into<String>) -> Failure {
        Failure { kind, message: message.into() }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind.name(), self.message)
    }
}

pub type Performer = fn(&Value, &Value) -> Result<Value, Failure>;

pub fn packs() -> Vec<Pack> {
    vec![fs::pack(), http::pack(), process::pack(), sqlite::pack()]
}

pub fn pack(id: &str) -> Option<Pack> {
    packs().into_iter().find(|pack| pack.id == id)
}

pub fn performer(capability: &str) -> Option<Performer> {
    fs::performer(capability)
        .or_else(|| http::performer(capability))
        .or_else(|| process::performer(capability))
        .or_else(|| sqlite::performer(capability))
}

pub use declared::{Declared, DeclaredLoader};

pub fn implemented(declared: &Declared, capability: &str) -> bool {
    declared.endpoints.get(capability).is_some()
        || declared.statements.get(capability).is_some()
        || performer(capability).is_some()
}

pub fn perform(
    declared: &Declared,
    capability: &str,
    scope: &Value,
    input: &Value,
) -> Option<Result<Value, Failure>> {
    if let Some(endpoint) = declared.endpoints.get(capability) {
        return Some(endpoint.perform(scope, input));
    }
    if let Some(statement) = declared.statements.get(capability) {
        return Some(statement.perform(scope, input));
    }
    performer(capability).map(|perform| perform(scope, input))
}
