use sx_kernel::json::{Json, python_repr, repr_str};

pub(crate) fn string_of<'j>(json: &'j Json, what: &str) -> Result<&'j str, String> {
    json.as_str().ok_or_else(|| format!("{what} must be a string, not {}", python_repr(json)))
}

pub(crate) fn required<'j>(json: &'j Json, key: &str, what: &str) -> Result<&'j Json, String> {
    json.get(key).ok_or_else(|| format!("{what} needs {}", repr_str(key)))
}

pub(crate) fn unknown_keys(json: &Json, allowed: &[&str], what: &str) -> Result<(), String> {
    let Json::Object(entries) = json else {
        return Err(format!("{what} must be an object, not {}", python_repr(json)));
    };
    match entries.iter().find(|(key, _)| !allowed.contains(&key.as_str())) {
        Some((key, _)) => Err(format!("{what} has an unknown key {}", repr_str(key))),
        None => Ok(()),
    }
}
