use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum YamlError {
    Unsupported { line: usize, construct: &'static str },
    Malformed { line: usize, message: String },
    NotAScalar { path: String, found: &'static str },
    NotACollection { path: String },
    Missing { path: String },
}

impl fmt::Display for YamlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            YamlError::Unsupported { line, construct } => {
                write!(f, "line {line}: {construct} are outside the supported YAML subset, so the text is left alone")
            }
            YamlError::Malformed { line, message } => write!(f, "line {line}: {message}"),
            YamlError::NotAScalar { path, found } => write!(f, "{path} is {found}, not a scalar"),
            YamlError::NotACollection { path } => write!(f, "{path} is a scalar, so it has no keys"),
            YamlError::Missing { path } => write!(f, "{path} does not exist, so there is nothing to replace"),
        }
    }
}

impl std::error::Error for YamlError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Style {
    Plain,
    Single,
    Double,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Scalar {
    start: usize,
    end: usize,
    style: Style,
    value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Node {
    Empty { at: usize },
    Scalar(Scalar),
    Map(Vec<(String, Node)>),
    Seq(Vec<Node>),
}

impl Node {
    fn describe(&self) -> &'static str {
        match self {
            Node::Empty { .. } => "empty",
            Node::Scalar(_) => "a scalar",
            Node::Map(_) => "a mapping",
            Node::Seq(_) => "a sequence",
        }
    }
}

struct Line<'t> {
    start: usize,
    text: &'t str,
    indent: usize,
    kind: Kind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Blank,
    Comment,
    Content,
}

fn lines(text: &str) -> Result<Vec<Line<'_>>, YamlError> {
    let mut found = Vec::new();
    let mut start = 0;
    for (number, segment) in text.split_inclusive('\n').enumerate() {
        let body = segment.strip_suffix('\n').map_or(segment, |body| body.strip_suffix('\r').unwrap_or(body));
        if number == 0 && body.starts_with('\u{feff}') {
            return Err(YamlError::Unsupported { line: 1, construct: "a byte order mark" });
        }
        let indent = body.bytes().take_while(|b| *b == b' ').count();
        let after_spaces = body.get(indent..).unwrap_or_default();
        let rest = after_spaces.trim_start_matches([' ', '\t']);
        let kind = if rest.is_empty() {
            Kind::Blank
        } else if rest.starts_with('#') {
            Kind::Comment
        } else {
            Kind::Content
        };
        let leading = after_spaces.get(..after_spaces.len() - rest.len()).unwrap_or_default();
        if kind != Kind::Blank && leading.contains('\t') {
            return Err(YamlError::Unsupported { line: number + 1, construct: "tabs used for indentation" });
        }
        found.push(Line { start, text: body, indent, kind });
        start += segment.len();
    }
    Ok(found)
}

struct Parser<'t> {
    lines: Vec<Line<'t>>,
}

type Parsed = Result<(Node, usize), YamlError>;

fn malformed(line: usize, message: impl Into<String>) -> YamlError {
    YamlError::Malformed { line: line + 1, message: message.into() }
}

fn unsupported(line: usize, construct: &'static str) -> YamlError {
    YamlError::Unsupported { line: line + 1, construct }
}

fn is_marker(text: &str, marker: &str) -> bool {
    text.strip_prefix(marker).is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
}

fn is_dash(rest: &str) -> bool {
    rest == "-" || rest.starts_with("- ")
}

impl<'t> Parser<'t> {
    fn line(&self, index: usize) -> Result<&Line<'t>, YamlError> {
        self.lines.get(index).ok_or_else(|| malformed(index, "the document ends unexpectedly"))
    }

    fn next_content(&self, from: usize) -> Option<usize> {
        (from..self.lines.len()).find(|index| self.lines.get(*index).is_some_and(|line| line.kind == Kind::Content))
    }

    fn document(&self) -> Result<Node, YamlError> {
        let Some(mut first) = self.next_content(0) else {
            return Ok(Node::Empty { at: 0 });
        };
        let marker = self.line(first)?;
        if marker.indent == 0 && is_marker(marker.text, "---") {
            if marker.text.get(3..).is_some_and(|rest| !rest.trim_start().is_empty() && !rest.trim_start().starts_with('#')) {
                return Err(unsupported(first, "content on the document start line"));
            }
            match self.next_content(first + 1) {
                Some(next) => first = next,
                None => return Ok(Node::Empty { at: marker.start }),
            }
        }
        let line = self.line(first)?;
        self.reject_marker(first)?;
        let (node, next) = self.node(first, line.indent, None)?;
        if let Some(extra) = self.next_content(next) {
            self.reject_marker(extra)?;
            return Err(malformed(extra, "unexpected content; the indentation does not match the structure above"));
        }
        Ok(node)
    }

    fn reject_marker(&self, index: usize) -> Result<(), YamlError> {
        let line = self.line(index)?;
        if line.indent == 0 && (is_marker(line.text, "---") || is_marker(line.text, "...")) {
            return Err(unsupported(index, "multiple documents or document end markers"));
        }
        if line.indent == 0 && line.text.starts_with('%') {
            return Err(unsupported(index, "directives"));
        }
        Ok(())
    }

    fn node(&self, index: usize, column: usize, parent: Option<usize>) -> Parsed {
        let line = self.line(index)?;
        let rest = line.text.get(column..).unwrap_or_default();
        if is_dash(rest) {
            return self.sequence(index, column);
        }
        if self.key_at(index, column)?.is_some() {
            return self.mapping(index, column);
        }
        let scalar = self.scalar_at(index, column)?;
        self.no_continuation(index, parent)?;
        Ok((Node::Scalar(scalar), index + 1))
    }

    fn no_continuation(&self, index: usize, parent: Option<usize>) -> Result<(), YamlError> {
        match (self.next_content(index + 1), parent) {
            (Some(next), Some(parent)) if self.line(next)?.indent > parent => {
                Err(unsupported(next, "multi-line scalars"))
            }
            _ => Ok(()),
        }
    }

    fn key_at(&self, index: usize, column: usize) -> Result<Option<(String, usize)>, YamlError> {
        let line = self.line(index)?;
        let rest = line.text.get(column..).unwrap_or_default();
        let Some(first) = rest.chars().next() else {
            return Ok(None);
        };
        if matches!(first, '[' | '{' | '&' | '*' | '!' | '|' | '>' | '%' | '@' | '`') {
            return Ok(None);
        }
        if (first == '?' || first == ':') && rest.get(1..).is_some_and(|after| after.is_empty() || after.starts_with(' ')) {
            return Err(unsupported(index, "complex keys"));
        }
        let (key, after_key) = if first == '\'' || first == '"' {
            match quoted(index, rest)? {
                Some((key, used)) => (key, used),
                None => return Ok(None),
            }
        } else {
            match plain_key_end(rest) {
                Some(end) => (rest.get(..end).unwrap_or_default().trim_end().to_string(), end),
                None => return Ok(None),
            }
        };
        let tail = rest.get(after_key..).unwrap_or_default();
        let spaces = tail.len() - tail.trim_start_matches(' ').len();
        let Some(after_colon) = tail.get(spaces..).and_then(|t| t.strip_prefix(':')) else {
            return Ok(None);
        };
        if !(after_colon.is_empty() || after_colon.starts_with([' ', '\t'])) {
            return Ok(None);
        }
        if key.is_empty() {
            return Err(malformed(index, "a mapping key cannot be empty"));
        }
        Ok(Some((key, column + after_key + spaces + 1)))
    }

    fn mapping(&self, first: usize, column: usize) -> Parsed {
        let mut entries: Vec<(String, Node)> = Vec::new();
        let mut index = first;
        loop {
            let Some((key, after_colon)) = self.key_at(index, column)? else {
                return Err(malformed(index, "expected a mapping key"));
            };
            if entries.iter().any(|(known, _)| *known == key) {
                return Err(malformed(index, format!("the key {key:?} appears twice in one mapping")));
            }
            let line = self.line(index)?;
            let tail = line.text.get(after_colon..).unwrap_or_default();
            let value_start = after_colon + (tail.len() - tail.trim_start_matches([' ', '\t']).len());
            let value = line.text.get(value_start..).unwrap_or_default();
            let (node, next) = if value.is_empty() || value.starts_with('#') {
                self.block_value(index, column, after_colon)?
            } else {
                if is_dash(value) {
                    return Err(malformed(index, "a block sequence cannot start on the same line as its key"));
                }
                let scalar = self.scalar_at(index, value_start)?;
                self.no_continuation(index, Some(column))?;
                (Node::Scalar(scalar), index + 1)
            };
            entries.push((key, node));
            index = next;
            match self.next_content(index) {
                Some(following) => {
                    let upcoming = self.line(following)?;
                    if upcoming.indent == column && !is_dash(upcoming.text.get(column..).unwrap_or_default()) {
                        self.reject_marker(following)?;
                        index = following;
                    } else if upcoming.indent > column {
                        return Err(malformed(following, "this line is indented deeper than the mapping it follows"));
                    } else {
                        break;
                    }
                }
                None => break,
            }
        }
        Ok((Node::Map(entries), index))
    }

    fn block_value(&self, index: usize, column: usize, insert_at: usize) -> Parsed {
        let start = self.line(index)?.start;
        let empty = (Node::Empty { at: start + insert_at }, index + 1);
        let Some(next) = self.next_content(index + 1) else {
            return Ok(empty);
        };
        let upcoming = self.line(next)?;
        let rest = upcoming.text.get(upcoming.indent..).unwrap_or_default();
        if upcoming.indent > column {
            self.reject_marker(next)?;
            let (node, after) = self.node(next, upcoming.indent, Some(column))?;
            if matches!(node, Node::Scalar(_)) {
                return Err(unsupported(next, "multi-line scalars"));
            }
            Ok((node, after))
        } else if upcoming.indent == column && is_dash(rest) {
            self.sequence(next, column)
        } else {
            Ok(empty)
        }
    }

    fn sequence(&self, first: usize, column: usize) -> Parsed {
        let mut items = Vec::new();
        let mut index = first;
        loop {
            let line = self.line(index)?;
            let rest = line.text.get(column..).unwrap_or_default();
            let after_dash = rest.get(1..).unwrap_or_default();
            let spaces = after_dash.len() - after_dash.trim_start_matches([' ', '\t']).len();
            let content = after_dash.get(spaces..).unwrap_or_default();
            let (node, next) = if content.is_empty() || content.starts_with('#') {
                self.block_item(index, column, line.start + column + 1)?
            } else {
                self.node(index, column + 1 + spaces, Some(column))?
            };
            items.push(node);
            index = next;
            match self.next_content(index) {
                Some(following) => {
                    let upcoming = self.line(following)?;
                    let upcoming_rest = upcoming.text.get(upcoming.indent..).unwrap_or_default();
                    if upcoming.indent == column && is_dash(upcoming_rest) {
                        self.reject_marker(following)?;
                        index = following;
                    } else if upcoming.indent > column {
                        return Err(malformed(following, "this line is indented deeper than the sequence item it follows"));
                    } else {
                        break;
                    }
                }
                None => break,
            }
        }
        Ok((Node::Seq(items), index))
    }

    fn block_item(&self, index: usize, column: usize, insert_at: usize) -> Parsed {
        let empty = (Node::Empty { at: insert_at }, index + 1);
        let Some(next) = self.next_content(index + 1) else {
            return Ok(empty);
        };
        let upcoming = self.line(next)?;
        if upcoming.indent > column {
            self.reject_marker(next)?;
            let (node, after) = self.node(next, upcoming.indent, Some(column))?;
            if matches!(node, Node::Scalar(_)) {
                return Err(unsupported(next, "multi-line scalars"));
            }
            Ok((node, after))
        } else {
            Ok(empty)
        }
    }

    fn scalar_at(&self, index: usize, column: usize) -> Result<Scalar, YamlError> {
        let line = self.line(index)?;
        let rest = line.text.get(column..).unwrap_or_default();
        let start = line.start + column;
        let Some(first) = rest.chars().next() else {
            return Err(malformed(index, "expected a value"));
        };
        let refusal = match first {
            '[' | '{' => Some("flow collections"),
            '&' => Some("anchors"),
            '*' => Some("aliases"),
            '!' => Some("tags"),
            '|' | '>' => Some("multi-line scalars"),
            '%' => Some("directives"),
            _ => None,
        };
        if let Some(construct) = refusal {
            return Err(unsupported(index, construct));
        }
        if matches!(first, '@' | '`' | ',' | ']' | '}') {
            return Err(malformed(index, format!("a plain scalar cannot start with {first:?}; quote it")));
        }
        if (first == '?' || first == ':') && rest.get(1..).is_some_and(|after| after.is_empty() || after.starts_with(' ')) {
            return Err(unsupported(index, "complex keys"));
        }
        if first == '\'' || first == '"' {
            let Some((value, used)) = quoted(index, rest)? else {
                return Err(unsupported(index, "multi-line scalars"));
            };
            let tail = rest.get(used..).unwrap_or_default();
            let trimmed = tail.trim_start_matches([' ', '\t']);
            if !(trimmed.is_empty() || (trimmed.starts_with('#') && trimmed.len() < tail.len())) {
                return Err(malformed(index, "unexpected text after a quoted scalar"));
            }
            let style = if first == '\'' { Style::Single } else { Style::Double };
            return Ok(Scalar { start, end: start + used, style, value });
        }
        let end = plain_end(rest);
        let text = rest.get(..end).unwrap_or_default().trim_end();
        if text.contains(": ") || text.ends_with(':') {
            return Err(malformed(index, "a plain scalar cannot contain `: `; quote it"));
        }
        Ok(Scalar { start, end: start + text.len(), style: Style::Plain, value: text.to_string() })
    }
}

fn plain_end(rest: &str) -> usize {
    let bytes = rest.as_bytes();
    for (at, byte) in bytes.iter().enumerate() {
        if *byte == b'#' && at > 0 && matches!(bytes.get(at - 1), Some(b' ' | b'\t')) {
            return at;
        }
    }
    rest.len()
}

fn plain_key_end(rest: &str) -> Option<usize> {
    let limit = plain_end(rest);
    let candidate = rest.get(..limit)?;
    let mut search = 0;
    while let Some(found) = candidate.get(search..)?.find(':') {
        let at = search + found;
        let after = candidate.get(at + 1..)?;
        if after.is_empty() || after.starts_with([' ', '\t']) {
            return Some(at);
        }
        search = at + 1;
    }
    None
}

fn quoted(line: usize, rest: &str) -> Result<Option<(String, usize)>, YamlError> {
    let mut chars = rest.char_indices();
    let Some((_, quote)) = chars.next() else {
        return Ok(None);
    };
    let mut value = String::new();
    while let Some((at, c)) = chars.next() {
        if quote == '\'' {
            if c == '\'' {
                if rest.get(at + 1..).is_some_and(|after| after.starts_with('\'')) {
                    value.push('\'');
                    chars.next();
                    continue;
                }
                return Ok(Some((value, at + 1)));
            }
            value.push(c);
        } else if c == '"' {
            return Ok(Some((value, at + 1)));
        } else if c == '\\' {
            let Some((_, escape)) = chars.next() else {
                return Err(unsupported(line, "multi-line scalars"));
            };
            let decoded = match escape {
                '0' => '\0',
                'a' => '\u{7}',
                'b' => '\u{8}',
                't' | '\t' => '\t',
                'n' => '\n',
                'v' => '\u{b}',
                'f' => '\u{c}',
                'r' => '\r',
                'e' => '\u{1b}',
                ' ' => ' ',
                '"' => '"',
                '/' => '/',
                '\\' => '\\',
                'N' => '\u{85}',
                '_' => '\u{a0}',
                'L' => '\u{2028}',
                'P' => '\u{2029}',
                'x' => hex_escape(&mut chars, 2, line)?,
                'u' => hex_escape(&mut chars, 4, line)?,
                'U' => hex_escape(&mut chars, 8, line)?,
                other => return Err(malformed(line, format!("the escape \\{other} is not defined in YAML"))),
            };
            value.push(decoded);
        } else {
            value.push(c);
        }
    }
    Ok(None)
}

fn hex_escape(chars: &mut std::str::CharIndices<'_>, digits: usize, line: usize) -> Result<char, YamlError> {
    let mut code: u32 = 0;
    for _ in 0..digits {
        let digit = chars.next().and_then(|(_, c)| c.to_digit(16)).ok_or_else(|| malformed(line, "a hexadecimal escape is cut short"))?;
        code = code.checked_mul(16).and_then(|shifted| shifted.checked_add(digit)).ok_or_else(|| malformed(line, "a hexadecimal escape is too large"))?;
    }
    char::from_u32(code).ok_or_else(|| malformed(line, format!("U+{code:X} is not a Unicode scalar value")))
}

fn parse(text: &str) -> Result<Node, YamlError> {
    let lines = lines(text)?;
    Parser { lines }.document()
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
            Node::Map(entries) => &entries.iter().find(|(key, _)| key == part)?.1,
            Node::Seq(items) => items.get(index_of(part)?)?,
            Node::Empty { .. } | Node::Scalar(_) => return None,
        };
    }
    Some(current)
}

pub fn get(text: &str, path: &[&str]) -> Result<Option<String>, YamlError> {
    let root = parse(text)?;
    if path.is_empty() && matches!(root, Node::Empty { .. }) {
        return Ok(None);
    }
    match locate(&root, path) {
        None => Ok(None),
        Some(Node::Scalar(scalar)) => Ok(Some(scalar.value.clone())),
        Some(Node::Empty { .. }) => Ok(Some(String::new())),
        Some(other) => Err(YamlError::NotAScalar { path: shown_path(path), found: other.describe() }),
    }
}

pub fn keys(text: &str, path: &[&str]) -> Result<Vec<String>, YamlError> {
    let root = parse(text)?;
    match locate(&root, path) {
        None | Some(Node::Empty { .. }) => Ok(Vec::new()),
        Some(Node::Map(entries)) => Ok(entries.iter().map(|(key, _)| key.clone()).collect()),
        Some(Node::Seq(items)) => Ok((0..items.len()).map(|index| index.to_string()).collect()),
        Some(Node::Scalar(_)) => Err(YamlError::NotACollection { path: shown_path(path) }),
    }
}

pub fn set(text: &str, path: &[&str], value: &str) -> Result<String, YamlError> {
    let root = parse(text)?;
    let (start, end, replacement) = match locate(&root, path) {
        None => return Err(YamlError::Missing { path: shown_path(path) }),
        Some(Node::Empty { .. }) if path.is_empty() => return Err(YamlError::Missing { path: shown_path(path) }),
        Some(Node::Scalar(scalar)) => (scalar.start, scalar.end, render(value, scalar.style, Some(&scalar.value))),
        Some(Node::Empty { at }) => (*at, *at, format!(" {}", render(value, Style::Plain, None))),
        Some(other) => return Err(YamlError::NotAScalar { path: shown_path(path), found: other.describe() }),
    };
    let head = text.get(..start).ok_or_else(|| malformed(0, "an edit position fell inside a character"))?;
    let tail = text.get(end..).ok_or_else(|| malformed(0, "an edit position fell inside a character"))?;
    Ok(format!("{head}{replacement}{tail}"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Text,
    Other,
}

fn class(value: &str) -> Class {
    let lowered = value.to_ascii_lowercase();
    let unsigned = lowered.strip_prefix(['+', '-']).unwrap_or(&lowered);
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit() || b == b'_');
    let number = digits(unsigned)
        || unsigned.strip_prefix("0x").is_some_and(|h| !h.is_empty() && h.bytes().all(|b| b.is_ascii_hexdigit() || b == b'_'))
        || unsigned.strip_prefix("0o").is_some_and(|o| !o.is_empty() && o.bytes().all(|b| (b'0'..=b'7').contains(&b)))
        || matches!(unsigned, ".inf" | ".nan")
        || float_shaped(unsigned)
        || dated(unsigned);
    let word = matches!(
        lowered.as_str(),
        "" | "~" | "null" | "true" | "false" | "yes" | "no" | "on" | "off" | "y" | "n" | "=" | "<<"
    );
    if number || word { Class::Other } else { Class::Text }
}

fn float_shaped(unsigned: &str) -> bool {
    let (mantissa, exponent) = match unsigned.split_once('e') {
        Some((m, e)) => (m, Some(e)),
        None => (unsigned, None),
    };
    if let Some(e) = exponent {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        if e.is_empty() || !e.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
    }
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let all_digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit() || b == b'_');
    all_digits(whole) && all_digits(fraction) && (!whole.is_empty() || !fraction.is_empty()) && (mantissa.contains('.') || exponent.is_some())
}

fn dated(unsigned: &str) -> bool {
    let bytes = unsigned.as_bytes();
    bytes.len() >= 8 && bytes.iter().take(4).all(u8::is_ascii_digit) && bytes.get(4) == Some(&b'-')
}

fn plain_safe(value: &str) -> bool {
    let Some(first) = value.chars().next() else {
        return false;
    };
    let after_first = value.get(first.len_utf8()..).unwrap_or_default();
    let indicator = match first {
        ',' | '[' | ']' | '{' | '}' | '#' | '&' | '*' | '!' | '|' | '>' | '\'' | '"' | '%' | '@' | '`' => true,
        '-' | '?' | ':' => after_first.is_empty() || after_first.starts_with([' ', '\t']),
        _ => false,
    };
    !indicator
        && !value.starts_with(char::is_whitespace)
        && !value.ends_with(char::is_whitespace)
        && !value.contains(": ")
        && !value.contains(" #")
        && !value.contains('\t')
        && !value.ends_with(':')
        && !value.starts_with("---")
        && !value.starts_with("...")
        && !value.chars().any(|c| c.is_control() || matches!(c, '\u{85}' | '\u{2028}' | '\u{2029}' | '\u{feff}'))
}

fn single_quotable(value: &str) -> bool {
    !value.chars().any(|c| c.is_control() || matches!(c, '\u{85}' | '\u{2028}' | '\u{2029}' | '\u{feff}'))
}

fn double_quoted(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            '\u{85}' => out.push_str("\\N"),
            '\u{2028}' => out.push_str("\\L"),
            '\u{2029}' => out.push_str("\\P"),
            '\u{feff}' => out.push_str("\\uFEFF"),
            c if c.is_control() => out.push_str(&format!("\\x{:02X}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn render(value: &str, style: Style, old: Option<&str>) -> String {
    match style {
        Style::Double => double_quoted(value),
        Style::Single if single_quotable(value) => format!("'{}'", value.replace('\'', "''")),
        Style::Single => double_quoted(value),
        Style::Plain => {
            let protects_text = old.is_some_and(|previous| class(previous) == Class::Text) && class(value) == Class::Other;
            if plain_safe(value) && !protects_text { value.to_string() } else { double_quoted(value) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nested_document_reads_and_edits_in_place() {
        let text = "# manifest\nname: demo  # the name\nspec:\n  replicas: 3\n  ports:\n  - 80\n  - 443\n";
        assert_eq!(get(text, &["spec", "replicas"]), Ok(Some("3".to_string())));
        assert_eq!(get(text, &["spec", "ports", "1"]), Ok(Some("443".to_string())));
        assert_eq!(
            set(text, &["spec", "replicas"], "5").as_deref(),
            Ok("# manifest\nname: demo  # the name\nspec:\n  replicas: 5\n  ports:\n  - 80\n  - 443\n")
        );
    }

    #[test]
    fn a_text_scalar_keeps_its_type_when_the_new_value_looks_like_a_number() {
        let text = "tag: stable\ncount: 3\n";
        assert_eq!(set(text, &["tag"], "1.10").as_deref(), Ok("tag: \"1.10\"\ncount: 3\n"));
        assert_eq!(set(text, &["count"], "4").as_deref(), Ok("tag: stable\ncount: 4\n"));
    }
}
