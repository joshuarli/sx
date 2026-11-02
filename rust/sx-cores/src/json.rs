use std::collections::HashSet;
use std::fmt;

use sx_kernel::json::MAX_DEPTH;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JsonError {
    Malformed { line: usize, column: usize, message: String },
    NotAScalar { path: String, found: &'static str },
    NotACollection { path: String },
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonError::Malformed { line, column, message } => write!(f, "line {line} column {column}: {message}"),
            JsonError::NotAScalar { path, found } => write!(f, "{path} is {found}, not a scalar"),
            JsonError::NotACollection { path } => write!(f, "{path} is a scalar, so it has no keys"),
        }
    }
}

impl std::error::Error for JsonError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Absent,
    Null,
    Boolean,
    Number,
    String,
    Array,
    Object,
}

impl Kind {
    pub const ALL: [Kind; 7] =
        [Kind::Absent, Kind::Null, Kind::Boolean, Kind::Number, Kind::String, Kind::Array, Kind::Object];

    pub fn variant(self) -> &'static str {
        match self {
            Kind::Absent => "Absent",
            Kind::Null => "Null",
            Kind::Boolean => "Boolean",
            Kind::Number => "Number",
            Kind::String => "String",
            Kind::Array => "Array",
            Kind::Object => "Object",
        }
    }

    pub fn from_variant(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|kind| kind.variant() == name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Node {
    Null,
    Bool(bool),
    Number(String),
    Text(String),
    Array(Vec<Node>),
    Object(Vec<(String, Node)>),
}

impl Node {
    fn kind(&self) -> Kind {
        match self {
            Node::Null => Kind::Null,
            Node::Bool(_) => Kind::Boolean,
            Node::Number(_) => Kind::Number,
            Node::Text(_) => Kind::String,
            Node::Array(_) => Kind::Array,
            Node::Object(_) => Kind::Object,
        }
    }
}

enum Open {
    Array(Vec<Node>),
    Object { entries: Vec<(String, Node)>, known: HashSet<String>, key: String },
}

struct Parser<'t> {
    text: &'t str,
    at: usize,
}

type Parsed<T> = Result<T, JsonError>;

impl<'t> Parser<'t> {
    fn byte(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }

    fn here(&self) -> char {
        self.text.get(self.at..).and_then(|rest| rest.chars().next()).unwrap_or('\u{fffd}')
    }

    fn fail<T>(&self, at: usize, message: impl Into<String>) -> Parsed<T> {
        let before = self.text.get(..at).unwrap_or_default();
        let line = before.matches('\n').count() + 1;
        let column = before.rsplit('\n').next().map_or(0, |last| last.chars().count()) + 1;
        Err(JsonError::Malformed { line, column, message: message.into() })
    }

    fn blanks(&mut self) {
        while matches!(self.byte(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn found(&self) -> String {
        if self.byte().is_none() { "the end of the text".to_string() } else { format!("{:?}", self.here()) }
    }

    fn document(&mut self) -> Parsed<Node> {
        if self.text.starts_with('\u{feff}') {
            return self.fail(0, "a byte order mark is not allowed");
        }
        let mut open: Vec<Open> = Vec::new();
        'values: loop {
            self.blanks();
            let mut done = match self.byte() {
                Some(b'{') => {
                    self.enter(open.len())?;
                    self.at += 1;
                    self.blanks();
                    if self.byte() == Some(b'}') {
                        self.at += 1;
                        Node::Object(Vec::new())
                    } else {
                        let mut known = HashSet::new();
                        let key = self.key(&mut known)?;
                        open.push(Open::Object { entries: Vec::new(), known, key });
                        continue 'values;
                    }
                }
                Some(b'[') => {
                    self.enter(open.len())?;
                    self.at += 1;
                    self.blanks();
                    if self.byte() == Some(b']') {
                        self.at += 1;
                        Node::Array(Vec::new())
                    } else {
                        open.push(Open::Array(Vec::new()));
                        continue 'values;
                    }
                }
                Some(b'"') => Node::Text(self.string()?),
                Some(b'-' | b'0'..=b'9') => self.number()?,
                Some(b) if b.is_ascii_alphabetic() => self.word()?,
                _ => return self.fail(self.at, format!("expected a value, found {}", self.found())),
            };
            loop {
                self.blanks();
                match open.pop() {
                    None => {
                        if self.byte().is_some() {
                            return self.fail(self.at, "unexpected content after the value");
                        }
                        return Ok(done);
                    }
                    Some(Open::Array(mut items)) => {
                        items.push(done);
                        match self.byte() {
                            Some(b',') => {
                                let comma = self.at;
                                self.at += 1;
                                self.blanks();
                                if self.byte() == Some(b']') {
                                    return self.fail(comma, "a trailing comma is not allowed");
                                }
                                open.push(Open::Array(items));
                                continue 'values;
                            }
                            Some(b']') => {
                                self.at += 1;
                                done = Node::Array(items);
                            }
                            _ => {
                                return self.fail(self.at, format!("expected ',' or ']' in an array, found {}", self.found()));
                            }
                        }
                    }
                    Some(Open::Object { mut entries, mut known, key }) => {
                        entries.push((key, done));
                        match self.byte() {
                            Some(b',') => {
                                let comma = self.at;
                                self.at += 1;
                                self.blanks();
                                if self.byte() == Some(b'}') {
                                    return self.fail(comma, "a trailing comma is not allowed");
                                }
                                let key = self.key(&mut known)?;
                                open.push(Open::Object { entries, known, key });
                                continue 'values;
                            }
                            Some(b'}') => {
                                self.at += 1;
                                done = Node::Object(entries);
                            }
                            _ => {
                                return self.fail(self.at, format!("expected ',' or '}}' in an object, found {}", self.found()));
                            }
                        }
                    }
                }
            }
        }
    }

    fn key(&mut self, known: &mut HashSet<String>) -> Parsed<String> {
        if self.byte() != Some(b'"') {
            return self.fail(self.at, format!("expected a string key, found {}", self.found()));
        }
        let key_at = self.at;
        let key = self.string()?;
        if !known.insert(key.clone()) {
            return self.fail(key_at, format!("the key {key:?} appears twice in one object"));
        }
        self.blanks();
        if self.byte() != Some(b':') {
            return self.fail(self.at, format!("expected ':' after the key, found {}", self.found()));
        }
        self.at += 1;
        Ok(key)
    }

    fn enter(&self, enclosing: usize) -> Parsed<()> {
        if enclosing + 1 > MAX_DEPTH {
            return self.fail(self.at, format!("the value nests deeper than {MAX_DEPTH} levels"));
        }
        Ok(())
    }

    fn word(&mut self) -> Parsed<Node> {
        let start = self.at;
        while self.byte().is_some_and(|b| b.is_ascii_alphanumeric()) {
            self.at += 1;
        }
        match self.text.get(start..self.at) {
            Some("true") => Ok(Node::Bool(true)),
            Some("false") => Ok(Node::Bool(false)),
            Some("null") => Ok(Node::Null),
            other => self.fail(start, format!("expected a value, found {:?}", other.unwrap_or_default())),
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.at;
        while self.byte().is_some_and(|b| b.is_ascii_digit()) {
            self.at += 1;
        }
        self.at - start
    }

    fn number(&mut self) -> Parsed<Node> {
        let start = self.at;
        if self.byte() == Some(b'-') {
            self.at += 1;
            if !self.byte().is_some_and(|b| b.is_ascii_digit()) {
                return self.fail(self.at, format!("a number needs a digit after '-', found {}", self.found()));
            }
        }
        if self.byte() == Some(b'0') {
            self.at += 1;
            if self.byte().is_some_and(|b| b.is_ascii_digit()) {
                return self.fail(self.at - 1, "a number cannot have leading zeros");
            }
        } else {
            self.digits();
        }
        if self.byte() == Some(b'.') {
            self.at += 1;
            if self.digits() == 0 {
                return self.fail(self.at, format!("a number needs digits after the decimal point, found {}", self.found()));
            }
        }
        if matches!(self.byte(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.byte(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if self.digits() == 0 {
                return self.fail(self.at, format!("a number needs digits in its exponent, found {}", self.found()));
            }
        }
        Ok(Node::Number(self.text.get(start..self.at).unwrap_or_default().to_string()))
    }

    fn hex4(&mut self, escape: usize) -> Parsed<u32> {
        let mut code = 0;
        for _ in 0..4 {
            let Some(digit) = self.byte().and_then(|b| char::from(b).to_digit(16)) else {
                return self.fail(escape, "a \\u escape needs four hexadecimal digits");
            };
            code = code * 16 + digit;
            self.at += 1;
        }
        Ok(code)
    }

    fn unicode_escape(&mut self, escape: usize) -> Parsed<char> {
        let first = self.hex4(escape)?;
        let code = match first {
            0xD800..=0xDBFF => {
                let second_escape = self.at;
                if self.text.get(self.at..).is_some_and(|rest| rest.starts_with("\\u")) {
                    self.at += 2;
                    let second = self.hex4(second_escape)?;
                    if !(0xDC00..=0xDFFF).contains(&second) {
                        return self.fail(escape, "a high surrogate escape must be followed by a low surrogate escape");
                    }
                    0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                } else {
                    return self.fail(escape, "a high surrogate escape must be followed by a low surrogate escape");
                }
            }
            0xDC00..=0xDFFF => return self.fail(escape, "a low surrogate escape must follow a high surrogate escape"),
            other => other,
        };
        match char::from_u32(code) {
            Some(c) => Ok(c),
            None => self.fail(escape, "a \\u escape is not a Unicode scalar value"),
        }
    }

    fn string(&mut self) -> Parsed<String> {
        let open = self.at;
        self.at += 1;
        let mut out = String::new();
        let mut run = self.at;
        loop {
            match self.byte() {
                None => return self.fail(open, "the string is not closed"),
                Some(b'"') => {
                    out.push_str(self.text.get(run..self.at).unwrap_or_default());
                    self.at += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    out.push_str(self.text.get(run..self.at).unwrap_or_default());
                    let escape = self.at;
                    self.at += 1;
                    let decoded = match self.byte() {
                        Some(b'"') => '"',
                        Some(b'\\') => '\\',
                        Some(b'/') => '/',
                        Some(b'b') => '\u{8}',
                        Some(b'f') => '\u{c}',
                        Some(b'n') => '\n',
                        Some(b'r') => '\r',
                        Some(b't') => '\t',
                        Some(b'u') => {
                            self.at += 1;
                            let c = self.unicode_escape(escape)?;
                            out.push(c);
                            run = self.at;
                            continue;
                        }
                        None => return self.fail(open, "the string is not closed"),
                        Some(_) => {
                            let found = self.here();
                            return self.fail(escape, format!("the escape \\{found} is not defined in JSON"));
                        }
                    };
                    out.push(decoded);
                    self.at += 1;
                    run = self.at;
                }
                Some(b) if b < 0x20 => {
                    return self.fail(self.at, "a control character in a string must be escaped");
                }
                Some(_) => self.at += 1,
            }
        }
    }
}

fn parse(text: &str) -> Parsed<Node> {
    Parser { text, at: 0 }.document()
}

fn shown_path(path: &[&str]) -> String {
    if path.is_empty() {
        "the document root".to_string()
    } else {
        format!("the path {}", path.iter().map(|part| format!("{part:?}")).collect::<Vec<_>>().join("/"))
    }
}

fn index_of(part: &str) -> Option<usize> {
    if part.is_empty() || (part.len() > 1 && part.starts_with('0')) || !part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    part.parse().ok()
}

fn locate<'n>(root: &'n Node, path: &[&str]) -> Option<&'n Node> {
    let mut current = root;
    for part in path {
        current = match current {
            Node::Object(entries) => &entries.iter().find(|(key, _)| key == part)?.1,
            Node::Array(items) => items.get(index_of(part)?)?,
            Node::Null | Node::Bool(_) | Node::Number(_) | Node::Text(_) => return None,
        };
    }
    Some(current)
}

pub fn get(text: &str, path: &[&str]) -> Result<Option<String>, JsonError> {
    let root = parse(text)?;
    match locate(&root, path) {
        None | Some(Node::Null) => Ok(None),
        Some(Node::Bool(flag)) => Ok(Some(flag.to_string())),
        Some(Node::Number(number)) => Ok(Some(number.clone())),
        Some(Node::Text(text)) => Ok(Some(text.clone())),
        Some(Node::Array(_)) => Err(JsonError::NotAScalar { path: shown_path(path), found: "an array" }),
        Some(Node::Object(_)) => Err(JsonError::NotAScalar { path: shown_path(path), found: "an object" }),
    }
}

pub fn keys(text: &str, path: &[&str]) -> Result<Vec<String>, JsonError> {
    let root = parse(text)?;
    match locate(&root, path) {
        None | Some(Node::Null) => Ok(Vec::new()),
        Some(Node::Object(entries)) => Ok(entries.iter().map(|(key, _)| key.clone()).collect()),
        Some(Node::Array(items)) => Ok((0..items.len()).map(|index| index.to_string()).collect()),
        Some(Node::Bool(_) | Node::Number(_) | Node::Text(_)) => {
            Err(JsonError::NotACollection { path: shown_path(path) })
        }
    }
}

pub fn kind(text: &str, path: &[&str]) -> Result<Kind, JsonError> {
    let root = parse(text)?;
    Ok(locate(&root, path).map_or(Kind::Absent, Node::kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nested_document_reads_scalars_keys_and_kinds() {
        let text = "{\"app\": {\"replicas\": 3, \"tags\": [\"a\", \"b\"], \"debug\": false, \"owner\": null}}";
        assert_eq!(get(text, &["app", "replicas"]), Ok(Some("3".to_string())));
        assert_eq!(get(text, &["app", "tags", "1"]), Ok(Some("b".to_string())));
        assert_eq!(get(text, &["app", "debug"]), Ok(Some("false".to_string())));
        assert_eq!(get(text, &["app", "owner"]), Ok(None));
        assert_eq!(keys(text, &["app"]), Ok(vec!["replicas".into(), "tags".into(), "debug".into(), "owner".into()]));
        assert_eq!(keys(text, &["app", "tags"]), Ok(vec!["0".to_string(), "1".to_string()]));
        assert_eq!(kind(text, &["app", "owner"]), Ok(Kind::Null));
        assert_eq!(kind(text, &["app", "gone"]), Ok(Kind::Absent));
    }

    #[test]
    fn a_number_keeps_the_text_it_was_written_with() {
        let text = "[1.50, 1E+400, -0, 123456789012345678901234567890, 0.1e-2]";
        let read: Vec<_> = (0..5).map(|i| get(text, &[&i.to_string()]).unwrap_or_default().unwrap_or_default()).collect();
        assert_eq!(read, ["1.50", "1E+400", "-0", "123456789012345678901234567890", "0.1e-2"]);
    }

    #[test]
    fn an_error_names_the_line_and_column_in_characters() {
        let text = "{\n  \"caf\u{e9}\": [1, 2,\n  ]}";
        let message = "a trailing comma is not allowed".to_string();
        assert_eq!(get(text, &[]), Err(JsonError::Malformed { line: 2, column: 16, message }));
    }
}
