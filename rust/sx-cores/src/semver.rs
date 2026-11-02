#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: i64,
    pub minor: i64,
    pub patch: i64,
}

fn component(text: &str) -> Option<i64> {
    let canonical = text == "0" || (!text.starts_with('0') && !text.is_empty());
    if !canonical || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

pub fn parse(text: &str) -> Option<Version> {
    let mut parts = text.split('.');
    let major = component(parts.next()?)?;
    let minor = component(parts.next()?)?;
    let patch = component(parts.next()?)?;
    match parts.next() {
        None => Some(Version { major, minor, patch }),
        Some(_) => None,
    }
}

pub fn compare(a: Version, b: Version) -> i64 {
    a.cmp(&b) as i64
}

pub fn show(version: Version) -> String {
    format!("{}.{}.{}", version.major, version.minor, version.patch)
}
