use regex_lite::{Regex, RegexBuilder};

pub const SIZE_LIMIT_BYTES: usize = 1 << 20;
pub const NEST_LIMIT: u32 = 50;

pub fn compile(pattern: &str) -> Result<Regex, String> {
    RegexBuilder::new(pattern)
        .size_limit(SIZE_LIMIT_BYTES)
        .nest_limit(NEST_LIMIT)
        .build()
        .map_err(|error| error.to_string())
}

pub fn validate(pattern: &str) -> Result<(), String> {
    compile(pattern).map(|_| ())
}

fn groups(captures: &regex_lite::Captures<'_>) -> Vec<String> {
    captures.iter().map(|group| group.map_or("", |m| m.as_str()).to_owned()).collect()
}

pub fn find(pattern: &str, s: &str) -> Result<Option<Vec<String>>, String> {
    Ok(compile(pattern)?.captures(s).map(|captures| groups(&captures)))
}

pub fn find_all(pattern: &str, s: &str) -> Result<Vec<Vec<String>>, String> {
    Ok(compile(pattern)?.captures_iter(s).map(|captures| groups(&captures)).collect())
}

pub fn matches(pattern: &str, s: &str) -> Result<Option<Vec<String>>, String> {
    validate(pattern)?;
    let whole = compile(&format!("\\A(?:{pattern})\\z"))?;
    Ok(whole.captures(s).map(|captures| groups(&captures)))
}

pub fn test(pattern: &str, s: &str) -> Result<bool, String> {
    Ok(compile(pattern)?.is_match(s))
}

pub fn replace(pattern: &str, s: &str, replacement: &str) -> Result<String, String> {
    Ok(compile(pattern)?.replace_all(s, replacement).into_owned())
}

pub fn split(pattern: &str, s: &str) -> Result<Vec<String>, String> {
    Ok(compile(pattern)?.split(s).map(str::to_owned).collect())
}
