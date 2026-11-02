use crate::definitions;
use crate::diagnostics::Diagnostic;
use crate::syntax::{DURATION_UNITS, Fields, Items, Keyword, Node, NodeKind, Position, RecordField, Span};
use crate::unicode;

pub const INT_MIN: i64 = i64::MIN;
pub const INT_MAX: i64 = i64::MAX;

const WHITESPACE: &[char] = &[' ', '\t', '\r', '\n', ','];
const DELIMITERS: &[char] = &['(', ')', '[', ']', '{', '}', '"', ';'];
const OPERATORS: &[&str] = &["+", "-", "*", "/", "=", "not=", "<", "<=", ">", ">=", "_"];
const NIL_SPELLINGS: &[&str] = &["nil", "null", "undefined", "None"];
const JS_OPERATORS: &[(&str, &str)] = &[
    ("&&", "and"),
    ("||", "or"),
    ("!", "not"),
    ("!=", "not="),
    ("!==", "not="),
    ("==", "="),
    ("===", "="),
    ("%", "rem"),
];
const UNIT_SPELLINGS: &[(&str, &str)] = &[
    ("msec", "ms"),
    ("millis", "ms"),
    ("sec", "s"),
    ("secs", "s"),
    ("second", "s"),
    ("seconds", "s"),
    ("min", "m"),
    ("mins", "m"),
    ("minute", "m"),
    ("minutes", "m"),
    ("hr", "h"),
    ("hrs", "h"),
    ("hour", "h"),
    ("hours", "h"),
];
const THREADING: &[&str] = &["->", "->>", "some->", "as->", "cond->"];

fn is_pattern_escape(c: char) -> bool {
    matches!(c, 'd' | 'D' | 'w' | 'W' | 's' | 'S' | 'b' | 'B' | 'A' | 'z' | 'p' | 'P')
        || matches!(c, '.' | '+' | '*' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$' | '-' | '/')
}

pub(crate) fn closer_of(opener: char) -> Option<char> {
    match opener {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        _ => None,
    }
}

fn unit_size(unit: &str) -> Option<i64> {
    DURATION_UNITS.iter().find(|(u, _)| *u == unit).map(|(_, size)| *size)
}

pub(crate) fn indent(line: &str) -> usize {
    line.chars().count() - unicode::lstrip_chars(line, " \t").chars().count()
}

pub(crate) fn is_blank(line: &str) -> bool {
    let text = unicode::strip_space(line);
    text.is_empty() || text.starts_with(';')
}

fn at(span: Span) -> String {
    format!("{}:{}", span.start.line, span.start.column)
}

fn digits(chars: &[char]) -> usize {
    chars.iter().take_while(|c| c.is_ascii_digit()).count()
}

fn match_signed_digits(chars: &[char]) -> Option<usize> {
    let sign = usize::from(chars.first() == Some(&'-'));
    let count = digits(&chars[sign..]);
    (count > 0).then_some(sign + count)
}

fn match_exponent(chars: &[char]) -> Option<usize> {
    if !matches!(chars.first(), Some('e' | 'E')) {
        return None;
    }
    let sign = usize::from(matches!(chars.get(1), Some('-' | '+')));
    let count = digits(&chars[1 + sign..]);
    (count > 0).then_some(1 + sign + count)
}

fn is_int(text: &[char]) -> bool {
    match_signed_digits(text) == Some(text.len())
}

fn is_float(text: &[char]) -> bool {
    let Some(lead) = match_signed_digits(text) else {
        return false;
    };
    let rest = &text[lead..];
    if rest.first() == Some(&'.') {
        let count = digits(&rest[1..]);
        if count == 0 {
            return false;
        }
        let after = &rest[1 + count..];
        return after.is_empty() || match_exponent(after) == Some(after.len());
    }
    match_exponent(rest) == Some(rest.len())
}

fn is_unfinished_float(text: &[char]) -> bool {
    let Some(lead) = match_signed_digits(text) else {
        return false;
    };
    let rest = &text[lead..];
    if rest.first() != Some(&'.') {
        return false;
    }
    let after = &rest[1..];
    after.is_empty() || match_exponent(after) == Some(after.len())
}

fn duration(text: &[char]) -> Option<(String, &'static str)> {
    let count = digits(text);
    if count == 0 {
        return None;
    }
    let unit: String = text[count..].iter().collect();
    let unit = DURATION_UNITS.iter().map(|(u, _)| *u).find(|u| *u == unit)?;
    Some((text[..count].iter().collect(), unit))
}

fn duration_like(text: &[char]) -> Option<(String, String)> {
    let mut amount = match_signed_digits(text)?;
    if text.get(amount) == Some(&'.') {
        let count = digits(&text[amount + 1..]);
        if count > 0 {
            amount += 1 + count;
        }
    }
    let unit = &text[amount..];
    if unit.is_empty() || !unit.iter().all(char::is_ascii_lowercase) {
        return None;
    }
    Some((text[..amount].iter().collect(), unit.iter().collect()))
}

fn is_name(text: &[char]) -> bool {
    let Some(first) = text.first() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    let body = 1 + text[1..].iter().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')).count();
    let suffix = &text[body..];
    suffix.is_empty() || suffix == ['?'] || suffix.iter().all(|c| *c == '\'')
}

fn is_variant(text: &[char]) -> bool {
    match text.first() {
        Some(first) if first.is_ascii_uppercase() => text[1..].iter().all(char::is_ascii_alphanumeric),
        _ => false,
    }
}

fn is_core_id(text: &[char]) -> bool {
    let mut segments = text.split(|c| *c == '-');
    let Some(first) = segments.next() else {
        return false;
    };
    let lower_alnum = |c: &char| c.is_ascii_lowercase() || c.is_ascii_digit();
    if first.is_empty() || !first[0].is_ascii_lowercase() || !first.iter().all(lower_alnum) {
        return false;
    }
    segments.all(|segment| !segment.is_empty() && segment.iter().all(lower_alnum))
}

fn is_qualified(text: &[char]) -> bool {
    let Some(dot) = text.iter().position(|c| *c == '.') else {
        return false;
    };
    let member = &text[dot + 1..];
    is_core_id(&text[..dot]) && (is_name(member) || is_variant(member))
}

fn is_keyword(text: &[char]) -> bool {
    if text.first() != Some(&':') {
        return false;
    }
    let Some(first) = text.get(1) else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || *first == '_') {
        return false;
    }
    let body = 2 + text[2..].iter().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')).count();
    let suffix = &text[body..];
    suffix.is_empty() || suffix == ['?']
}

fn kebab(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut spaced = String::with_capacity(text.len() + 4);
    for (i, c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() && i > 0 && (chars[i - 1].is_ascii_lowercase() || chars[i - 1].is_ascii_digit()) {
            spaced.push('-');
        }
        spaced.push(*c);
    }
    spaced.replace('_', "-").to_lowercase()
}

fn chars_of(text: &str) -> Vec<char> {
    text.chars().collect()
}

fn float_is_integer(value: f64) -> bool {
    value.is_finite() && value.fract() == 0.0
}

#[derive(Debug)]
pub struct ReadResult {
    pub program: Option<Node>,
    pub diagnostics: Vec<Diagnostic>,
}

struct Closed {
    opener: &'static str,
    opener_span: Span,
    closer: char,
    closer_span: Span,
}

struct Frame {
    opener: &'static str,
    closer: char,
    span: Span,
    as_list: bool,
    items: Vec<Node>,
    poisoned: bool,
    shorthand: bool,
}

struct Reader<'a> {
    source: Vec<char>,
    index: usize,
    line: usize,
    column: usize,
    diagnostics: Vec<Diagnostic>,
    stack: Vec<Frame>,
    top: Vec<Node>,
    lines: Vec<&'a str>,
    closed_top: Vec<Closed>,
}

impl<'a> Reader<'a> {
    fn new(source: &'a str) -> Reader<'a> {
        Reader {
            source: source.chars().collect(),
            index: 0,
            line: 1,
            column: 1,
            diagnostics: Vec::new(),
            stack: Vec::new(),
            top: Vec::new(),
            lines: source.split('\n').collect(),
            closed_top: Vec::new(),
        }
    }

    fn position(&self) -> Position {
        Position { line: self.line, column: self.column }
    }

    fn peek(&self) -> Option<char> {
        self.source.get(self.index).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.source.get(self.index).copied()?;
        self.index += 1;
        if c == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(c)
    }

    fn error(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
        self.poison();
    }

    fn poison(&mut self) {
        if let Some(frame) = self.stack.last_mut() {
            frame.poisoned = true;
        }
    }

    fn emit(&mut self, node: Node) {
        match self.stack.last_mut() {
            Some(frame) => frame.items.push(node),
            None => self.top.push(node),
        }
    }

    fn read(mut self) -> ReadResult {
        while let Some(c) = self.peek() {
            if WHITESPACE.contains(&c) {
                self.advance();
            } else if c == ';' {
                while self.peek().is_some_and(|c| c != '\n') {
                    self.advance();
                }
            } else if let Some(closer) = closer_of(c) {
                let start = self.position();
                self.advance();
                let opener = match c {
                    '(' => "(",
                    '[' => "[",
                    _ => "{",
                };
                self.open(opener, closer, Span::new(start, self.position()), c == '[');
            } else if matches!(c, ')' | ']' | '}') {
                self.close();
            } else if c == '"' {
                self.string();
            } else if c == '#' {
                self.dispatch();
            } else if c == '\'' {
                let start = self.position();
                self.advance();
                self.error(
                    Diagnostic::new(
                        "habit",
                        Span::new(start, self.position()),
                        "Sx has no quoting. Write a list literal as [a b c].",
                    )
                    .construct("quote"),
                );
            } else if matches!(c, '`' | '~' | '@' | '^') {
                let start = self.position();
                self.advance();
                self.error(
                    Diagnostic::new(
                        "habit",
                        Span::new(start, self.position()),
                        format!("Sx has no reader macros, so {c} has no meaning."),
                    )
                    .construct("reader-macro"),
                );
            } else {
                self.atom();
            }
        }
        let unclosed: Vec<(&'static str, char, Span)> = self.stack.iter().map(|f| (f.opener, f.closer, f.span)).collect();
        for (opener, closer, span) in unclosed {
            let (last, cut) = self.form_end(span);
            let where_text = match last {
                None => "at the end of the program".to_string(),
                Some(last) => format!("at the end of line {last}"),
            };
            let mut diagnostic = Diagnostic::new("unclosed", span, format!("This {opener} is never closed."))
                .fix(format!("add {closer} {where_text}"));
            if let (Some(_), Some(cut)) = (last, cut) {
                diagnostic = diagnostic.detail(format!("Line {cut} is indented like a new form, not inside this {opener}."));
            }
            self.diagnostics.push(diagnostic);
        }
        self.finish()
    }

    fn form_end(&self, opener: Span) -> (Option<usize>, Option<usize>) {
        let column = opener.start.column - 1;
        let mut last = opener.start.line;
        for number in opener.start.line + 1..=self.lines.len() {
            let text = self.lines[number - 1];
            if is_blank(text) {
                continue;
            }
            if indent(text) <= column {
                return (Some(last), Some(number));
            }
            last = number;
        }
        (None, None)
    }

    fn finish(mut self) -> ReadResult {
        if self.diagnostics.is_empty() {
            if self.top.is_empty() {
                let at = self.position();
                self.diagnostics.push(Diagnostic::new(
                    "empty-program",
                    Span::new(at, at),
                    "The program is empty. A program is one expression.",
                ));
            } else {
                match definitions::assemble(std::mem::take(&mut self.top)) {
                    Ok(program) => self.top.push(program),
                    Err(diagnostics) => self.diagnostics.extend(diagnostics),
                }
            }
        }
        if !self.diagnostics.is_empty() {
            return ReadResult { program: None, diagnostics: self.diagnostics };
        }
        ReadResult { program: self.top.pop(), diagnostics: Vec::new() }
    }

    fn open(&mut self, opener: &'static str, closer: char, span: Span, as_list: bool) {
        self.stack.push(Frame { opener, closer, span, as_list, items: Vec::new(), poisoned: false, shorthand: false });
    }

    fn close(&mut self) {
        let start = self.position();
        let Some(c) = self.advance() else {
            return;
        };
        let span = Span::new(start, self.position());
        self.close_with(c, span);
    }

    fn close_with(&mut self, c: char, span: Span) {
        loop {
            let Some(frame) = self.stack.last() else {
                self.extra_close(c, span);
                return;
            };
            if c != frame.closer && self.stays_open(frame, span) {
                let message = format!(
                    "This {c} is one too many: the {} at {} is still open, because the next line is indented inside it.",
                    frame.opener,
                    at(frame.span)
                );
                self.error(Diagnostic::new("unexpected-close", span, message).fix("remove it"));
                return;
            }
            let Some(mut frame) = self.stack.pop() else {
                return;
            };
            let whole = Span::new(frame.span.start, span.end);
            let mut resume = false;
            if c != frame.closer {
                let (diagnostic, again) = self.mismatched_close(c, span, &frame);
                resume = again;
                self.diagnostics.push(diagnostic);
                frame.poisoned = true;
            } else if self.stack.is_empty() {
                self.closed_top.push(Closed { opener: frame.opener, opener_span: frame.span, closer: c, closer_span: span });
            }
            let poisoned = frame.poisoned;
            let node = self.build(frame, whole);
            if poisoned {
                self.poison();
            }
            if let Some(node) = node {
                self.emit(node);
            }
            if !resume {
                return;
            }
        }
    }

    fn stays_open(&self, frame: &Frame, closer: Span) -> bool {
        let line = self.lines.get(closer.start.line - 1).copied().unwrap_or("");
        let rest: String = line.chars().skip(closer.end.column - 1).collect();
        let stripped = unicode::strip_chars(&rest, " \t)]}");
        if !stripped.is_empty() && !stripped.starts_with(';') {
            return false;
        }
        let column = frame.span.start.column - 1;
        for text in self.lines.iter().skip(closer.start.line) {
            if !is_blank(text) {
                return indent(text) > column;
            }
        }
        false
    }

    fn extra_close(&mut self, c: char, span: Span) {
        let message = format!("This {c} closes nothing, because every bracket before it is already closed.");
        let closed = self.closed_top.iter().find(|closed| self.encloses_by_indent(closed, span));
        let Some(closed) = closed else {
            self.error(Diagnostic::new("unexpected-close", span, message).fix("remove it"));
            return;
        };
        let early = at(closed.closer_span);
        let detail = format!(
            "The {} at {} closed at {early}, but line {} is indented as if inside it.",
            closed.opener,
            at(closed.opener_span),
            span.start.line
        );
        let fix = format!("remove the {} at {early}, then keep this one", closed.closer);
        self.error(Diagnostic::new("unexpected-close", span, message).detail(detail).fix(fix));
    }

    fn encloses_by_indent(&self, closed: &Closed, extra: Span) -> bool {
        if extra.start.line <= closed.closer_span.start.line {
            return false;
        }
        let column = closed.opener_span.start.column - 1;
        let from = closed.closer_span.start.line.min(self.lines.len());
        let to = extra.start.line.min(self.lines.len());
        self.lines[from..to].iter().all(|text| is_blank(text) || indent(text) > column)
    }

    fn mismatched_close(&self, c: char, span: Span, frame: &Frame) -> (Diagnostic, bool) {
        let message =
            format!("Expected {} to close the {} at {}, found {c}.", frame.closer, frame.opener, at(frame.span));
        let owner = self.stack.iter().rev().find(|f| f.closer == c);
        let (last, cut) = self.form_end(frame.span);
        if let (Some(owner), Some(last), Some(cut)) = (owner, last, cut)
            && cut <= span.start.line
        {
            let detail = format!(
                "This {c} most likely closes the {} at {}. Line {cut} is indented like a new form, so the {} at {} should end on line {last}.",
                owner.opener,
                at(owner.span),
                frame.opener,
                at(frame.span)
            );
            let diagnostic = Diagnostic::new("unexpected-close", span, message)
                .detail(detail)
                .fix(format!("add {} at the end of line {last}", frame.closer));
            return (diagnostic, true);
        }
        let diagnostic =
            Diagnostic::new("unexpected-close", span, message).fix(format!("replace {c} with {}", frame.closer));
        (diagnostic, false)
    }

    fn build(&mut self, frame: Frame, span: Span) -> Option<Node> {
        if frame.opener == "(" {
            return Some(Node { kind: NodeKind::Form(Items::new(frame.items)), span });
        }
        if frame.as_list {
            return Some(Node { kind: NodeKind::List(Items::new(frame.items)), span });
        }
        if frame.poisoned {
            return None;
        }
        self.record(frame.items, span)
    }

    fn record(&mut self, items: Vec<Node>, span: Span) -> Option<Node> {
        let mut fields: Vec<RecordField> = Vec::new();
        let mut seen: Vec<(String, Span)> = Vec::new();
        let mut ok = true;
        let mut items = items.into_iter();
        while let Some(key) = items.next() {
            let value = items.next();
            let NodeKind::Keyword(name) = &key.kind else {
                ok = false;
                let fix = match &key.kind {
                    NodeKind::String(text) => Some(format!(":{text}")),
                    _ => None,
                };
                self.diagnostics.push(
                    Diagnostic::new("record-key", key.span, "A record field name must be a keyword such as :name.")
                        .maybe_fix(fix),
                );
                continue;
            };
            let Some(value) = value else {
                ok = false;
                self.diagnostics.push(
                    Diagnostic::new("record-missing-value", key.span, format!("The field :{name} has no value."))
                        .detail("A record literal alternates field names and values: {:a 1 :b 2}."),
                );
                continue;
            };
            if let Some((_, first)) = seen.iter().find(|(n, _)| n == name) {
                ok = false;
                self.diagnostics.push(
                    Diagnostic::new(
                        "duplicate-field",
                        key.span,
                        format!("The field :{name} appears twice in this record."),
                    )
                    .detail(format!("First use: {}", at(*first))),
                );
                continue;
            }
            seen.push((name.clone(), key.span));
            fields.push(RecordField { key: Keyword { name: name.clone(), span: key.span }, value });
        }
        if !ok {
            self.poison();
            return None;
        }
        Some(Node { kind: NodeKind::Record(Fields::new(fields)), span })
    }

    fn dispatch(&mut self) {
        let start = self.position();
        self.advance();
        match self.peek() {
            Some('"') => self.pattern_string(start),
            Some('(') => {
                self.advance();
                let span = Span::new(start, self.position());
                self.diagnostics.push(
                    Diagnostic::new(
                        "habit",
                        span,
                        "Sx has no #(...) shorthand. Write the function with fn and named parameters.",
                    )
                    .construct("anonymous-fn-shorthand")
                    .fix("(fn [x] ...)")
                    .help("fn"),
                );
                self.open("(", ')', span, false);
                if let Some(frame) = self.stack.last_mut() {
                    frame.poisoned = true;
                    frame.shorthand = true;
                }
            }
            Some('{') => {
                self.advance();
                let span = Span::new(start, self.position());
                self.diagnostics.push(
                    Diagnostic::new(
                        "habit",
                        span,
                        "Sx has no set literals. Use a list, and distinct-by to remove duplicates.",
                    )
                    .construct("set-literal")
                    .fix("[a b c]")
                    .help("distinct-by"),
                );
                self.open("#{", '}', span, true);
                if let Some(frame) = self.stack.last_mut() {
                    frame.poisoned = true;
                }
            }
            _ => {
                let span = Span::new(start, self.position());
                self.error(Diagnostic::new("bad-literal", span, "Sx has no # syntax."));
            }
        }
    }

    fn pattern_string(&mut self, start: Position) {
        self.advance();
        let mut text = String::new();
        loop {
            let Some(c) = self.advance() else {
                let marker = Span::new(start, Position { line: start.line, column: start.column + 2 });
                self.error(Diagnostic::new("unclosed", marker, "This pattern string is never closed."));
                return;
            };
            match c {
                '"' => {
                    let span = Span::new(start, self.position());
                    self.emit(Node { kind: NodeKind::String(text), span });
                    return;
                }
                '\\' => match self.advance() {
                    Some('"') => text.push('"'),
                    Some(escaped) => {
                        text.push('\\');
                        text.push(escaped);
                    }
                    None => text.push('\\'),
                },
                _ => text.push(c),
            }
        }
    }

    fn string(&mut self) {
        let start = self.position();
        self.advance();
        let mut text = String::new();
        loop {
            let Some(c) = self.advance() else {
                let quote = Span::new(start, Position { line: start.line, column: start.column + 1 });
                self.error(Diagnostic::new("unclosed", quote, "This string is never closed."));
                return;
            };
            if c == '"' {
                let span = Span::new(start, self.position());
                self.emit(Node { kind: NodeKind::String(text), span });
                return;
            }
            if c != '\\' {
                text.push(c);
                continue;
            }
            let escape_start = Position { line: self.line, column: self.column - 1 };
            let escaped = self.advance();
            match escaped {
                Some('"') => text.push('"'),
                Some('\\') => text.push('\\'),
                Some('n') => text.push('\n'),
                Some('t') => text.push('\t'),
                Some('r') => text.push('\r'),
                Some('u') if self.peek() == Some('{') => {
                    if let Some(decoded) = self.unicode_escape(escape_start) {
                        text.push(decoded);
                    }
                }
                _ => {
                    let shown: String = escaped.map(String::from).unwrap_or_default();
                    let detail = if escaped.is_some_and(is_pattern_escape) {
                        "Escapes: \\\" \\\\ \\n \\r \\t \\u{HEX}. For a regular expression, write #\"...\" so backslashes stay, as in #\"\\d+\"."
                    } else {
                        "Escapes: \\\" \\\\ \\n \\r \\t \\u{HEX}"
                    };
                    self.error(
                        Diagnostic::new(
                            "bad-escape",
                            Span::new(escape_start, self.position()),
                            format!("\\{shown} is not a string escape."),
                        )
                        .detail(detail),
                    );
                }
            }
        }
    }

    fn unicode_escape(&mut self, escape_start: Position) -> Option<char> {
        self.advance();
        let mut digits = String::new();
        while let Some(c) = self.peek() {
            if c == '}' || c == '"' {
                break;
            }
            digits.push(c);
            self.advance();
        }
        if self.peek() == Some('}') {
            self.advance();
        }
        let span = Span::new(escape_start, self.position());
        let count = digits.chars().count();
        if !(1..=6).contains(&count) || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            self.error(Diagnostic::new("bad-escape", span, format!("\\u{{{digits}}} needs 1 to 6 hexadecimal digits.")));
            return None;
        }
        let decoded = u32::from_str_radix(&digits, 16).ok().and_then(char::from_u32);
        if decoded.is_none() {
            self.error(Diagnostic::new("bad-escape", span, format!("\\u{{{digits}}} is not a Unicode scalar value.")));
        }
        decoded
    }

    fn atom(&mut self) {
        let start = self.position();
        let mut text = String::new();
        while let Some(c) = self.peek() {
            if WHITESPACE.contains(&c) || DELIMITERS.contains(&c) {
                break;
            }
            text.push(c);
            self.advance();
        }
        let span = Span::new(start, self.position());
        if let Some(node) = self.classify(&text, span) {
            self.emit(node);
        }
    }

    fn classify(&mut self, text: &str, span: Span) -> Option<Node> {
        let chars = chars_of(text);
        let node = |kind: NodeKind| Some(Node { kind, span });
        if text.starts_with('%') && self.stack.iter().any(|f| f.shorthand) {
            return None;
        }
        if is_int(&chars) {
            return match text.parse::<i64>() {
                Ok(value) => node(NodeKind::Int(value)),
                Err(_) => {
                    self.error(Diagnostic::new("int-range", span, format!("{text} does not fit in a signed 64-bit Int.")));
                    None
                }
            };
        }
        if let Some((amount, unit)) = duration(&chars) {
            let milliseconds = amount.parse::<i64>().ok().zip(unit_size(unit)).and_then(|(a, s)| a.checked_mul(s));
            return match milliseconds {
                Some(ms) => node(NodeKind::Duration(ms)),
                None => {
                    self.error(Diagnostic::new("int-range", span, format!("{text} is longer than the largest duration.")));
                    None
                }
            };
        }
        if let Some((amount, unit)) = duration_like(&chars) {
            self.bad_duration(&amount, &unit, span);
            return None;
        }
        if is_float(&chars) {
            let value = text.parse::<f64>().unwrap_or(f64::INFINITY);
            if !value.is_finite() {
                self.error(Diagnostic::new("float-range", span, format!("{text} does not fit in a finite 64-bit Float.")));
                return None;
            }
            return node(NodeKind::Float(value));
        }
        if is_unfinished_float(&chars) {
            self.error(
                Diagnostic::new(
                    "bad-literal",
                    span,
                    format!("{text} is not a number. A Float has digits after the point, as in 1.0."),
                )
                .fix(text.replacen('.', ".0", 1)),
            );
            return None;
        }
        let first = chars[0];
        if unicode::is_digit(first) || (first == '-' && chars.get(1).is_some_and(|c| unicode::is_digit(*c))) {
            self.error(Diagnostic::new(
                "bad-literal",
                span,
                format!("{text} is not a number. An Int is written in decimal digits."),
            ));
            return None;
        }
        if text == "true" || text == "false" {
            return node(NodeKind::Bool(text == "true"));
        }
        if text == "True" || text == "False" {
            self.error(
                Diagnostic::new("habit", span, "Booleans are lowercase.")
                    .construct("capitalized-boolean")
                    .fix(text.to_lowercase()),
            );
            return None;
        }
        if NIL_SPELLINGS.contains(&text) {
            self.error(
                Diagnostic::new("habit", span, format!("Sx has no {text}. A missing value is none, of type Option."))
                    .construct("nil")
                    .fix("none")
                    .help("Option"),
            );
            return None;
        }
        if let Some(name) = text.strip_prefix(':') {
            if is_keyword(&chars) {
                return node(NodeKind::Keyword(name.to_string()));
            }
            let suggestion = kebab(name);
            let candidate = format!(":{suggestion}");
            let fix = (!suggestion.is_empty() && is_keyword(&chars_of(&candidate))).then_some(candidate);
            self.error(
                Diagnostic::new(
                    "bad-literal",
                    span,
                    format!("{text} is not a valid keyword. A keyword is : and a field name, such as :repo-id."),
                )
                .maybe_fix(fix),
            );
            return None;
        }
        if chars.len() > 1
            && let Some(key) = text.strip_suffix(':')
        {
            let name = kebab(unicode::strip_chars(key, "'\""));
            self.error(
                Diagnostic::new(
                    "habit",
                    span,
                    "This looks like a JavaScript object key. Record fields are keywords followed by a value.",
                )
                .construct("js-object-key")
                .fix(format!("{{:{name} value}}")),
            );
            return None;
        }
        if OPERATORS.contains(&text) || is_name(&chars) || is_qualified(&chars) || is_variant(&chars) {
            return node(NodeKind::Symbol(text.to_string()));
        }
        self.bad_symbol(text, span);
        None
    }

    fn bad_duration(&mut self, amount: &str, unit: &str, span: Span) {
        let unit = UNIT_SPELLINGS.iter().find(|(spelling, _)| *spelling == unit).map_or(unit, |(_, u)| *u);
        let detail = "Durations are a whole number followed by ms, s, m, or h, for example 250ms or 5s.";
        let Some(size) = unit_size(unit) else {
            self.error(Diagnostic::new("bad-literal", span, format!("{amount}{unit} is not a duration.")).detail(detail));
            return;
        };
        if amount.starts_with('-') {
            self.error(Diagnostic::new("bad-literal", span, "A duration cannot be negative.").detail(detail));
            return;
        }
        let milliseconds = amount.parse::<f64>().unwrap_or(f64::INFINITY) * size as f64;
        let mut fix = float_is_integer(milliseconds).then(|| format!("{milliseconds:.0}ms"));
        if !amount.contains('.') {
            fix = Some(format!("{amount}{unit}"));
        }
        self.error(
            Diagnostic::new("bad-literal", span, "This is not a duration literal.").detail(detail).maybe_fix(fix),
        );
    }

    fn bad_symbol(&mut self, text: &str, span: Span) {
        if THREADING.contains(&text) {
            self.error(
                Diagnostic::new(
                    "habit",
                    span,
                    format!("Sx has no threading macros such as {text}. Nest the calls, or bind each step with let."),
                )
                .construct("threading")
                .help("let"),
            );
        } else if let Some((_, replacement)) = JS_OPERATORS.iter().find(|(op, _)| *op == text) {
            self.error(
                Diagnostic::new("habit", span, format!("Sx has no {text} operator."))
                    .construct("js-operator")
                    .fix(*replacement),
            );
        } else if text.starts_with('.') {
            self.error(
                Diagnostic::new("habit", span, "Sx has no methods. Call a function with the value as an argument.")
                    .construct("method-call"),
            );
        } else if text.contains('/') {
            self.error(
                Diagnostic::new("habit", span, "Sx qualifies names with a dot, as in text.join, not with a slash.")
                    .construct("namespace-slash"),
            );
        } else {
            let suggestion = kebab(text);
            let suggested = chars_of(&suggestion);
            let valid = !suggestion.is_empty() && (is_name(&suggested) || is_qualified(&suggested));
            self.error(
                Diagnostic::new(
                    "bad-symbol",
                    span,
                    format!(
                        "{text} is not a valid name. A name starts with a lowercase letter, then letters, digits, - and _."
                    ),
                )
                .maybe_fix(valid.then_some(suggestion)),
            );
        }
    }
}

pub fn read(source: &str) -> ReadResult {
    Reader::new(source).read()
}
