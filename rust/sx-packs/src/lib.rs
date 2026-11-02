#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

pub mod fs;

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
    vec![fs::pack()]
}

pub fn pack(id: &str) -> Option<Pack> {
    packs().into_iter().find(|pack| pack.id == id)
}

pub fn performer(capability: &str) -> Option<Performer> {
    fs::performer(capability)
}
