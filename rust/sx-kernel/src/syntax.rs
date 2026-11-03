use std::fmt;
use std::ops::Deref;

use crate::json::python_float_repr;
use crate::unicode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: Position,
    pub end: Position,
}

impl Span {
    pub fn new(start: Position, end: Position) -> Span {
        Span { start, end }
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}-{}:{}", self.start.line, self.start.column, self.end.line, self.end.column)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Keyword {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, PartialEq)]
pub struct RecordField {
    pub key: Keyword,
    pub value: Node,
}

#[derive(Debug, PartialEq)]
pub enum NodeKind {
    Symbol(String),
    Keyword(String),
    Int(i64),
    Float(f64),
    Duration(i64),
    String(String),
    Bool(bool),
    List(Items),
    Record(Fields),
    Form(Items),
}

#[derive(Debug, PartialEq)]
pub struct Node {
    pub kind: NodeKind,
    pub span: Span,
}

#[derive(Debug, Default, PartialEq)]
pub struct Items(Vec<Node>);

#[derive(Debug, Default, PartialEq)]
pub struct Fields(Vec<RecordField>);

impl Items {
    pub fn new(items: Vec<Node>) -> Items {
        Items(items)
    }

    pub fn into_vec(mut self) -> Vec<Node> {
        std::mem::take(&mut self.0)
    }
}

impl Fields {
    pub fn new(fields: Vec<RecordField>) -> Fields {
        Fields(fields)
    }

    pub fn into_vec(mut self) -> Vec<RecordField> {
        std::mem::take(&mut self.0)
    }
}

impl Node {
    pub fn respan(&mut self, span: Span) {
        let mut pending: Vec<&mut Node> = vec![self];
        while let Some(node) = pending.pop() {
            node.span = span;
            match &mut node.kind {
                NodeKind::List(items) | NodeKind::Form(items) => pending.extend(items.0.iter_mut()),
                NodeKind::Record(fields) => {
                    for field in fields.0.iter_mut() {
                        field.key.span = span;
                        pending.push(&mut field.value);
                    }
                }
                _ => {}
            }
        }
    }
}

impl Deref for Items {
    type Target = [Node];

    fn deref(&self) -> &[Node] {
        &self.0
    }
}

impl Deref for Fields {
    type Target = [RecordField];

    fn deref(&self) -> &[RecordField] {
        &self.0
    }
}

fn dismantle(mut pending: Vec<Node>) {
    while let Some(node) = pending.pop() {
        match node.kind {
            NodeKind::List(items) | NodeKind::Form(items) => pending.extend(items.into_vec()),
            NodeKind::Record(fields) => pending.extend(fields.into_vec().into_iter().map(|f| f.value)),
            _ => {}
        }
    }
}

impl Drop for Items {
    fn drop(&mut self) {
        if self.0.iter().any(|n| matches!(n.kind, NodeKind::List(_) | NodeKind::Form(_) | NodeKind::Record(_))) {
            dismantle(std::mem::take(&mut self.0));
        }
    }
}

impl Drop for Fields {
    fn drop(&mut self) {
        let values: Vec<Node> = std::mem::take(&mut self.0).into_iter().map(|f| f.value).collect();
        dismantle(values);
    }
}

pub const DURATION_UNITS: [(&str, i64); 4] = [("h", 3_600_000), ("m", 60_000), ("s", 1_000), ("ms", 1)];

pub fn show_duration(milliseconds: i64) -> String {
    for (unit, size) in DURATION_UNITS {
        if milliseconds % size == 0 && (milliseconds != 0 || unit == "ms") {
            return format!("{}{unit}", milliseconds / size);
        }
    }
    format!("{milliseconds}ms")
}

pub fn show_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if !unicode::is_printable(c) => out.push_str(&format!("\\u{{{:X}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

enum Piece<'a> {
    Node(&'a Node),
    Text(&'static str),
    Field(&'a RecordField),
}

pub fn show(node: &Node) -> String {
    let mut out = String::new();
    let mut pending = vec![Piece::Node(node)];
    while let Some(piece) = pending.pop() {
        match piece {
            Piece::Text(text) => out.push_str(text),
            Piece::Field(field) => {
                out.push(':');
                out.push_str(&field.key.name);
                out.push(' ');
                pending.push(Piece::Node(&field.value));
            }
            Piece::Node(node) => match &node.kind {
                NodeKind::Symbol(name) => out.push_str(name),
                NodeKind::Keyword(name) => {
                    out.push(':');
                    out.push_str(name);
                }
                NodeKind::Int(value) => out.push_str(&value.to_string()),
                NodeKind::Float(value) => out.push_str(&python_float_repr(*value)),
                NodeKind::Duration(ms) => out.push_str(&show_duration(*ms)),
                NodeKind::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
                NodeKind::String(value) => out.push_str(&show_string(value)),
                NodeKind::List(items) => push_sequence(&mut out, &mut pending, "[", "]", items),
                NodeKind::Form(items) => push_sequence(&mut out, &mut pending, "(", ")", items),
                NodeKind::Record(fields) => {
                    out.push('{');
                    pending.push(Piece::Text("}"));
                    for (i, field) in fields.iter().enumerate().rev() {
                        pending.push(Piece::Field(field));
                        if i > 0 {
                            pending.push(Piece::Text(" "));
                        }
                    }
                }
            },
        }
    }
    out
}

fn push_sequence<'a>(
    out: &mut String,
    pending: &mut Vec<Piece<'a>>,
    open: &'static str,
    close: &'static str,
    items: &'a [Node],
) {
    out.push_str(open);
    pending.push(Piece::Text(close));
    for (i, item) in items.iter().enumerate().rev() {
        pending.push(Piece::Node(item));
        if i > 0 {
            pending.push(Piece::Text(" "));
        }
    }
}
