use std::collections::BTreeMap;
use std::fmt;

use crate::json::Json;
use crate::types::{Type, show_type};
use crate::values::{DecodeError, Value, decode};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputProblem {
    Unknown { name: String, declared: Vec<String> },
    Missing { missing: Vec<String>, required: Vec<(String, String)> },
    Mismatch { name: String, error: DecodeError },
}

impl fmt::Display for InputProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InputProblem::Unknown { name, declared } if declared.is_empty() => {
                write!(f, "{name} is not an input, because the script declares none")
            }
            InputProblem::Unknown { name, declared } => {
                write!(f, "{name} is not an input of the script, whose inputs are {}", declared.join(", "))
            }
            InputProblem::Missing { missing, required } => {
                let required: Vec<String> = required.iter().map(|(name, t)| format!("{name}: {t}")).collect();
                write!(
                    f,
                    "the script needs a value for input {}; its required inputs are {}",
                    missing.join(", "),
                    required.join(", ")
                )
            }
            InputProblem::Mismatch { name, error } => write!(f, "input {name}: {error}"),
        }
    }
}

impl std::error::Error for InputProblem {}

fn omittable(t: &Type) -> bool {
    matches!(t, Type::Option(_))
}

pub fn supply(
    declared: &BTreeMap<String, Type>,
    supplied: &[(String, Json)],
) -> Result<Vec<(String, Value)>, InputProblem> {
    if let Some((name, _)) = supplied.iter().find(|(name, _)| !declared.contains_key(name)) {
        return Err(InputProblem::Unknown { name: name.clone(), declared: declared.keys().cloned().collect() });
    }
    let missing: Vec<String> = declared
        .iter()
        .filter(|(name, t)| !omittable(t) && !supplied.iter().any(|(given, _)| given == *name))
        .map(|(name, _)| name.clone())
        .collect();
    if !missing.is_empty() {
        let required =
            declared.iter().filter(|(_, t)| !omittable(t)).map(|(name, t)| (name.clone(), show_type(t))).collect();
        return Err(InputProblem::Missing { missing, required });
    }
    let mut values = Vec::new();
    for (name, t) in declared {
        let value = match supplied.iter().rev().find(|(given, _)| given == name) {
            Some((_, json)) => decode(json, t, &format!("${name}"))
                .map_err(|error| InputProblem::Mismatch { name: name.clone(), error })?,
            None => Value::None,
        };
        values.push((name.clone(), value));
    }
    Ok(values)
}
