use crate::tables::{CASEFOLD, STRIPPED_WHITESPACE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextError {
    EmptySeparator,
}

pub fn casefold(s: &str) -> String {
    let mut folded = String::with_capacity(s.len());
    for c in s.chars() {
        match CASEFOLD.binary_search_by_key(&c, |&(code_point, _)| code_point) {
            Ok(index) => match CASEFOLD.get(index) {
                Some(&(_, expansion)) => folded.push_str(expansion),
                None => folded.push(c),
            },
            Err(_) => folded.push(c),
        }
    }
    folded
}

pub fn split(s: &str, separator: &str) -> Result<Vec<String>, TextError> {
    if separator.is_empty() {
        return Err(TextError::EmptySeparator);
    }
    Ok(s.split(separator).map(str::to_owned).collect())
}

pub fn join<S: AsRef<str>>(separator: &str, parts: &[S]) -> String {
    let mut joined = String::new();
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            joined.push_str(separator);
        }
        joined.push_str(part.as_ref());
    }
    joined
}

pub fn chars(s: &str) -> Vec<String> {
    s.chars().map(String::from).collect()
}

pub fn trim(s: &str) -> String {
    s.trim_matches(|c| STRIPPED_WHITESPACE.binary_search(&c).is_ok()).to_owned()
}

pub fn length(s: &str) -> i64 {
    i64::try_from(s.chars().count()).unwrap_or(i64::MAX)
}

pub fn starts_with(s: &str, start: &str) -> bool {
    s.starts_with(start)
}

pub fn ends_with(s: &str, end: &str) -> bool {
    s.ends_with(end)
}

pub fn contains(s: &str, part: &str) -> bool {
    s.contains(part)
}

pub fn replace(s: &str, old: &str, new: &str) -> String {
    s.replace(old, new)
}

pub fn from_int(n: i64) -> String {
    n.to_string()
}

pub fn to_int(s: &str) -> Option<i64> {
    let digits = s.strip_prefix('-').unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}
