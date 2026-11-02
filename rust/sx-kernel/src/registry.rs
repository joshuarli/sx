#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Effect {
    Read,
    Write,
}

impl Effect {
    pub fn name(self) -> &'static str {
        match self {
            Effect::Read => "read",
            Effect::Write => "write",
        }
    }

    pub fn from_name(name: &str) -> Option<Effect> {
        match name {
            "read" => Some(Effect::Read),
            "write" => Some(Effect::Write),
            _ => None,
        }
    }
}
