#![deny(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

pub mod fs;
pub mod http;
pub mod mcp;
pub mod process;

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
    vec![fs::pack(), http::pack(), process::pack()]
}

pub fn pack(id: &str) -> Option<Pack> {
    packs().into_iter().find(|pack| pack.id == id)
}

pub fn performer(capability: &str) -> Option<Performer> {
    fs::performer(capability).or_else(|| http::performer(capability)).or_else(|| process::performer(capability))
}

pub fn implemented(endpoints: &http::Endpoints, capability: &str) -> bool {
    endpoints.get(capability).is_some() || performer(capability).is_some()
}

pub fn perform(
    endpoints: &http::Endpoints,
    capability: &str,
    scope: &Value,
    input: &Value,
) -> Option<Result<Value, Failure>> {
    match endpoints.get(capability) {
        Some(endpoint) => Some(endpoint.perform(scope, input)),
        None => performer(capability).map(|perform| perform(scope, input)),
    }
}
