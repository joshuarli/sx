use sx_kernel::diagnostics::Diagnostic;
use sx_kernel::syntax::{Position, Span};

use crate::diag;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Float(f64),
    BigInt,
    Str(String),
    Template(Vec<String>, Vec<Vec<Token>>),
    Regex,
    Punct(&'static str),
    Eof,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
    pub newline_before: bool,
}

const PUNCTUATORS: &[&str] = &[
    ">>>=", "...", "===", "!==", "**=", "<<=", ">>=", ">>>", "&&=", "||=", "??=", "=>", "==", "!=", "<=", ">=", "&&", "||",
    "??", "?.", "++", "--", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "**", "<<", ">>", "{", "}", "(", ")", "[", "]",
    ";", ",", "<", ">", "+", "-", "*", "/", "%", "&", "|", "^", "!",
];

const SINGLE: &[&str] = &["~", "?", ":", "=", ".", "@"];

const SAFE_INTEGER: f64 = 9007199254740992.0;

pub struct Lexer {
    chars: Vec<char>,
    at: usize,
    line: usize,
    column: usize,
    newline: bool,
}

type LexResult<T> = Result<T, Diagnostic>;

impl Lexer {
    pub fn new(source: &str) -> Lexer {
        Lexer { chars: source.chars().collect(), at: 0, line: 1, column: 1, newline: false }
    }

    fn position(&self) -> Position {
        Position { line: self.line, column: self.column }
    }

    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.at + ahead).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.chars.get(self.at).copied()?;
        self.at += 1;
        if c == '\n' {
            self.line += 1;
            self.column = 1;
            self.newline = true;
        } else {
            self.column += 1;
        }
        Some(c)
    }

    fn error(&self, start: Position, message: &str) -> Diagnostic {
        diag("ts-syntax", Span::new(start, self.position()), message)
    }

    fn starts_with(&self, text: &str) -> bool {
        text.chars().enumerate().all(|(i, c)| self.peek(i) == Some(c))
    }

    pub fn tokenize(mut self) -> LexResult<Vec<Token>> {
        let mut tokens = self.run(false)?;
        let at = self.position();
        tokens.push(Token { tok: Tok::Eof, span: Span::new(at, at), newline_before: true });
        Ok(tokens)
    }

    fn skip_trivia(&mut self) -> LexResult<()> {
        loop {
            match (self.peek(0), self.peek(1)) {
                (Some(c), _) if c.is_whitespace() => {
                    self.bump();
                }
                (Some('/'), Some('/')) => {
                    while self.peek(0).is_some_and(|c| c != '\n') {
                        self.bump();
                    }
                }
                (Some('/'), Some('*')) => {
                    let start = self.position();
                    self.bump();
                    self.bump();
                    loop {
                        if self.starts_with("*/") {
                            self.bump();
                            self.bump();
                            break;
                        }
                        if self.bump().is_none() {
                            return Err(self.error(start, "Unterminated comment"));
                        }
                    }
                }
                _ => return Ok(()),
            }
        }
    }

    fn run(&mut self, in_template: bool) -> LexResult<Vec<Token>> {
        let mut tokens: Vec<Token> = Vec::new();
        let mut depth = 0usize;
        loop {
            self.newline = false;
            self.skip_trivia()?;
            let newline_before = self.newline || tokens.is_empty();
            let start = self.position();
            let Some(c) = self.peek(0) else {
                if in_template {
                    return Err(self.error(start, "Unterminated template literal"));
                }
                return Ok(tokens);
            };
            let tok = if c.is_ascii_digit() || (c == '.' && self.peek(1).is_some_and(|d| d.is_ascii_digit())) {
                self.number(start)?
            } else if is_ident_start(c) {
                let mut name = String::new();
                while let Some(c) = self.peek(0).filter(|c| is_ident_part(*c)) {
                    name.push(c);
                    self.bump();
                }
                Tok::Ident(name)
            } else if c == '"' || c == '\'' {
                self.string(c, start)?
            } else if c == '`' {
                self.template(start)?
            } else if c == '#' {
                self.bump();
                let mut name = String::from("#");
                while let Some(c) = self.peek(0).filter(|c| is_ident_part(*c)) {
                    name.push(c);
                    self.bump();
                }
                Tok::Ident(name)
            } else if c == '/' && regex_allowed(tokens.last()) {
                self.regex(start)?
            } else {
                if in_template && c == '}' && depth == 0 {
                    return Ok(tokens);
                }
                let punct = PUNCTUATORS.iter().chain(SINGLE.iter()).find(|p| self.starts_with(p)).copied();
                let Some(punct) = punct else {
                    self.bump();
                    return Err(self.error(start, &format!("Unexpected character {c:?}")));
                };
                if punct == "?." && self.peek(2).is_some_and(|d| d.is_ascii_digit()) {
                    self.bump();
                    Tok::Punct("?")
                } else {
                    for _ in punct.chars() {
                        self.bump();
                    }
                    match punct {
                        "{" => depth += 1,
                        "}" => depth = depth.saturating_sub(1),
                        _ => {}
                    }
                    Tok::Punct(punct)
                }
            };
            tokens.push(Token { tok, span: Span::new(start, self.position()), newline_before });
        }
    }

    fn number(&mut self, start: Position) -> LexResult<Tok> {
        let mut text = String::new();
        let radix = match (self.peek(0), self.peek(1)) {
            (Some('0'), Some('x' | 'X')) => 16,
            (Some('0'), Some('o' | 'O')) => 8,
            (Some('0'), Some('b' | 'B')) => 2,
            _ => 10,
        };
        if radix != 10 {
            self.bump();
            self.bump();
            while let Some(c) = self.peek(0).filter(|c| c.is_ascii_alphanumeric() || *c == '_') {
                self.bump();
                if c != '_' {
                    text.push(c);
                }
            }
            return match u128::from_str_radix(&text, radix) {
                Ok(value) if (value as f64) <= SAFE_INTEGER => Ok(Tok::Int(value as i64)),
                Ok(_) => Ok(Tok::Float(f64::INFINITY)),
                Err(_) => Err(self.error(start, "Malformed number")),
            };
        }
        let mut is_float = false;
        while let Some(c) = self.peek(0) {
            match c {
                '0'..='9' => text.push(c),
                '_' => {}
                '.' if !is_float && !text.contains(['e', 'E']) => {
                    is_float = true;
                    text.push(c);
                }
                'e' | 'E' if !text.contains(['e', 'E']) => {
                    let sign = matches!(self.peek(1), Some('+' | '-'));
                    if !self.peek(if sign { 2 } else { 1 }).is_some_and(|d| d.is_ascii_digit()) {
                        break;
                    }
                    is_float = true;
                    text.push('e');
                    self.bump();
                    if sign {
                        text.push(self.peek(0).unwrap_or('+'));
                        self.bump();
                    }
                    continue;
                }
                'n' => {
                    self.bump();
                    return Ok(Tok::BigInt);
                }
                _ => break,
            }
            self.bump();
        }
        if text.starts_with('.') {
            text.insert(0, '0');
        }
        if text.ends_with('.') {
            text.push('0');
        }
        if is_float {
            return text.parse::<f64>().map(Tok::Float).map_err(|_| self.error(start, "Malformed number"));
        }
        match text.parse::<u128>() {
            Ok(value) if value <= SAFE_INTEGER as u128 => Ok(Tok::Int(value as i64)),
            Ok(_) | Err(_) => Ok(Tok::Float(f64::INFINITY)),
        }
    }

    fn escape(&mut self, start: Position, out: &mut String) -> LexResult<()> {
        let Some(c) = self.bump() else {
            return Err(self.error(start, "Unterminated string"));
        };
        match c {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'v' => out.push('\u{b}'),
            '0' if !self.peek(0).is_some_and(|d| d.is_ascii_digit()) => out.push('\0'),
            '\n' => {}
            'x' => {
                let hex: String = (0..2).filter_map(|_| self.bump()).collect();
                let value = u32::from_str_radix(&hex, 16).map_err(|_| self.error(start, "Malformed escape"))?;
                out.push(char::from_u32(value).ok_or_else(|| self.error(start, "Malformed escape"))?);
            }
            'u' => {
                let value = if self.peek(0) == Some('{') {
                    self.bump();
                    let mut hex = String::new();
                    while let Some(d) = self.bump() {
                        if d == '}' {
                            break;
                        }
                        hex.push(d);
                    }
                    u32::from_str_radix(&hex, 16).map_err(|_| self.error(start, "Malformed escape"))?
                } else {
                    let hex: String = (0..4).filter_map(|_| self.bump()).collect();
                    let value = u32::from_str_radix(&hex, 16).map_err(|_| self.error(start, "Malformed escape"))?;
                    if (0xD800..0xDC00).contains(&value) && self.starts_with("\\u") {
                        let save = (self.at, self.line, self.column);
                        self.bump();
                        self.bump();
                        let low: String = (0..4).filter_map(|_| self.bump()).collect();
                        match u32::from_str_radix(&low, 16) {
                            Ok(low) if (0xDC00..0xE000).contains(&low) => {
                                0x10000 + ((value - 0xD800) << 10) + (low - 0xDC00)
                            }
                            _ => {
                                (self.at, self.line, self.column) = save;
                                value
                            }
                        }
                    } else {
                        value
                    }
                };
                out.push(char::from_u32(value).ok_or_else(|| self.error(start, "A lone surrogate cannot be written"))?);
            }
            other => out.push(other),
        }
        Ok(())
    }

    fn string(&mut self, quote: char, start: Position) -> LexResult<Tok> {
        self.bump();
        let mut out = String::new();
        loop {
            match self.bump() {
                None | Some('\n') => return Err(self.error(start, "Unterminated string")),
                Some(c) if c == quote => return Ok(Tok::Str(out)),
                Some('\\') => self.escape(start, &mut out)?,
                Some(c) => out.push(c),
            }
        }
    }

    fn template(&mut self, start: Position) -> LexResult<Tok> {
        self.bump();
        let mut quasis = Vec::new();
        let mut expressions = Vec::new();
        let mut current = String::new();
        loop {
            match self.bump() {
                None => return Err(self.error(start, "Unterminated template literal")),
                Some('`') => {
                    quasis.push(current);
                    return Ok(Tok::Template(quasis, expressions));
                }
                Some('\\') => self.escape(start, &mut current)?,
                Some('$') if self.peek(0) == Some('{') => {
                    self.bump();
                    quasis.push(std::mem::take(&mut current));
                    let mut inner = self.run(true)?;
                    let at = self.position();
                    inner.push(Token { tok: Tok::Eof, span: Span::new(at, at), newline_before: true });
                    self.bump();
                    expressions.push(inner);
                }
                Some(c) => current.push(c),
            }
        }
    }

    fn regex(&mut self, start: Position) -> LexResult<Tok> {
        self.bump();
        let mut in_class = false;
        loop {
            match self.bump() {
                None | Some('\n') => return Err(self.error(start, "Unterminated regular expression")),
                Some('\\') => {
                    self.bump();
                }
                Some('[') => in_class = true,
                Some(']') => in_class = false,
                Some('/') if !in_class => break,
                Some(_) => {}
            }
        }
        while self.peek(0).is_some_and(is_ident_part) {
            self.bump();
        }
        Ok(Tok::Regex)
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$'
}

fn is_ident_part(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

const VALUE_KEYWORDS: [&str; 5] = ["this", "super", "null", "true", "false"];

fn regex_allowed(previous: Option<&Token>) -> bool {
    match previous.map(|t| &t.tok) {
        None => true,
        Some(Tok::Int(_) | Tok::Float(_) | Tok::Str(_) | Tok::Template(..) | Tok::Regex | Tok::BigInt) => false,
        Some(Tok::Ident(name)) => {
            !VALUE_KEYWORDS.contains(&name.as_str())
                && matches!(
                    name.as_str(),
                    "return" | "typeof" | "case" | "do" | "else" | "in" | "of" | "instanceof" | "new" | "delete" | "void"
                        | "throw" | "yield" | "await"
                )
        }
        Some(Tok::Punct(p)) => !matches!(*p, ")" | "]" | "}" | "++" | "--"),
        Some(Tok::Eof) => true,
    }
}
