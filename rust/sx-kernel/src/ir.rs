use std::collections::BTreeMap;
use std::sync::Arc;

use crate::registry::Effect;
use crate::syntax::Span;
use crate::types::{CallErrorKind, EnumType, Type};
use crate::values::Value;

#[derive(Clone, Debug, PartialEq)]
pub enum Binder {
    Name(String),
    Discard,
    Tuple(Vec<Binder>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum MatchPattern {
    Some(Box<MatchPattern>),
    None,
    Ok(Box<MatchPattern>),
    Err(Box<MatchPattern>),
    Variant { variant: String, inner: Option<Box<MatchPattern>> },
    Literal(Value),
    Any(Binder),
}

#[derive(Clone, Debug, PartialEq)]
pub struct WidenedField {
    pub name: String,
    pub present: bool,
    pub conversion: Option<Conversion>,
    pub field_type: Type,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Conversion {
    ToSome(Option<Box<Conversion>>),
    EachField(Vec<(String, Option<Conversion>)>),
    WidenRecord(Vec<WidenedField>),
    EachItem(Vec<Option<Conversion>>),
    EachElement(Box<Conversion>),
    VariantName,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WrapKind {
    Some,
    Ok,
    Err,
}

impl WrapKind {
    pub fn name(self) -> &'static str {
        match self {
            WrapKind::Some => "some",
            WrapKind::Ok => "ok",
            WrapKind::Err => "err",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HigherOrder {
    Map,
    Filter,
    Keep,
    Any,
    All,
    SortBy,
    DistinctBy,
    GroupBy,
    Reduce,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreludeOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Quot,
    Rem,
    Float,
    Round,
    Floor,
    Ceil,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Not,
    And,
    Or,
    Str,
    Count,
    First,
    Rest,
    TupleItem,
    Nth,
    Take,
    Concat,
    Flatten,
    Contains,
    MapGet,
    Assoc,
    Dissoc,
    Keys,
    Vals,
    Entries,
    IntoMap,
    HigherOrder(HigherOrder),
}

const PRELUDE_NAMES: &[(&str, PreludeOp)] = &[
    ("+", PreludeOp::Add),
    ("-", PreludeOp::Subtract),
    ("*", PreludeOp::Multiply),
    ("/", PreludeOp::Divide),
    ("quot", PreludeOp::Quot),
    ("rem", PreludeOp::Rem),
    ("float", PreludeOp::Float),
    ("round", PreludeOp::Round),
    ("floor", PreludeOp::Floor),
    ("ceil", PreludeOp::Ceil),
    ("=", PreludeOp::Equal),
    ("not=", PreludeOp::NotEqual),
    ("<", PreludeOp::Less),
    ("<=", PreludeOp::LessEqual),
    (">", PreludeOp::Greater),
    (">=", PreludeOp::GreaterEqual),
    ("not", PreludeOp::Not),
    ("and", PreludeOp::And),
    ("or", PreludeOp::Or),
    ("str", PreludeOp::Str),
    ("count", PreludeOp::Count),
    ("first", PreludeOp::First),
    ("rest", PreludeOp::Rest),
    ("tuple-item", PreludeOp::TupleItem),
    ("nth", PreludeOp::Nth),
    ("take", PreludeOp::Take),
    ("concat", PreludeOp::Concat),
    ("flatten", PreludeOp::Flatten),
    ("contains?", PreludeOp::Contains),
    ("map-get", PreludeOp::MapGet),
    ("assoc", PreludeOp::Assoc),
    ("dissoc", PreludeOp::Dissoc),
    ("keys", PreludeOp::Keys),
    ("vals", PreludeOp::Vals),
    ("entries", PreludeOp::Entries),
    ("into-map", PreludeOp::IntoMap),
    ("map", PreludeOp::HigherOrder(HigherOrder::Map)),
    ("filter", PreludeOp::HigherOrder(HigherOrder::Filter)),
    ("keep", PreludeOp::HigherOrder(HigherOrder::Keep)),
    ("any?", PreludeOp::HigherOrder(HigherOrder::Any)),
    ("all?", PreludeOp::HigherOrder(HigherOrder::All)),
    ("sort-by", PreludeOp::HigherOrder(HigherOrder::SortBy)),
    ("distinct-by", PreludeOp::HigherOrder(HigherOrder::DistinctBy)),
    ("group-by", PreludeOp::HigherOrder(HigherOrder::GroupBy)),
    ("reduce", PreludeOp::HigherOrder(HigherOrder::Reduce)),
];

impl PreludeOp {
    pub fn from_name(name: &str) -> Option<PreludeOp> {
        PRELUDE_NAMES.iter().find(|(n, _)| *n == name).map(|(_, op)| *op)
    }

    pub fn name(self) -> &'static str {
        PRELUDE_NAMES.iter().find(|(_, op)| *op == self).map_or("?", |(n, _)| n)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lambda {
    pub params: Vec<Binder>,
    pub param_types: Vec<Type>,
    pub result: Type,
    pub body: Box<Node>,
    pub captured: Option<String>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Let {
    pub binder: Binder,
    pub value: Box<Node>,
    pub body: Box<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct If {
    pub condition: Box<Node>,
    pub then: Box<Node>,
    pub otherwise: Box<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Match {
    pub scrutinee: Box<Node>,
    pub clauses: Vec<(MatchPattern, Node)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Retry {
    pub delays: Vec<i64>,
    pub kinds: Vec<CallErrorKind>,
    pub body: Box<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WaitUntil {
    pub intervals: Vec<i64>,
    pub predicate: Lambda,
    pub body: Box<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Loop {
    pub maximum: i64,
    pub binders: Vec<Binder>,
    pub initial: Vec<Node>,
    pub body: Box<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    Literal { value: Value, value_type: Type },
    Variable { name: String },
    Let(Let),
    LocalFunction { name: String, body: Box<Node> },
    If(If),
    Match(Match),
    Lambda(Lambda),
    ApplyLocal { name: String, function: Lambda, args: Vec<Node> },
    Prelude { op: PreludeOp, args: Vec<Node> },
    CoreCall { function: String, args: Vec<Node> },
    Call { capability: String, argument: Box<Node> },
    Get { record: Box<Node>, field: String },
    MakeRecord { fields: Vec<(String, Node)> },
    MakeList { items: Vec<Node>, element: Type },
    MakeTuple { items: Vec<Node> },
    MakeBracket { items: Vec<Node>, shape: Type },
    MakeEmptyBrace { shape: Type },
    Wrap { kind: WrapKind, value: Box<Node>, wrapped_type: Type },
    MakeVariant { enum_name: String, variant: String, payload: Option<Box<Node>> },
    Coerce { value: Box<Node>, conversion: Conversion },
    Attempt { body: Box<Node> },
    Retry(Retry),
    WaitUntil(WaitUntil),
    Sleep { milliseconds: i64 },
    Loop(Loop),
    Recur { args: Vec<Node> },
    Par { branches: Vec<Node> },
    ParMap { limit: i64, function: Lambda, items: Box<Node> },
}

impl NodeKind {
    pub fn name(&self) -> &'static str {
        match self {
            NodeKind::Literal { .. } => "Literal",
            NodeKind::Variable { .. } => "Variable",
            NodeKind::Let(_) => "Let",
            NodeKind::LocalFunction { .. } => "LocalFunction",
            NodeKind::If(_) => "If",
            NodeKind::Match(_) => "Match",
            NodeKind::Lambda(_) => "Lambda",
            NodeKind::ApplyLocal { .. } => "ApplyLocal",
            NodeKind::Prelude { .. } => "Prelude",
            NodeKind::CoreCall { .. } => "CoreCall",
            NodeKind::Call { .. } => "Call",
            NodeKind::Get { .. } => "Get",
            NodeKind::MakeRecord { .. } => "MakeRecord",
            NodeKind::MakeList { .. } => "MakeList",
            NodeKind::MakeTuple { .. } => "MakeTuple",
            NodeKind::MakeBracket { .. } => "MakeBracket",
            NodeKind::MakeEmptyBrace { .. } => "MakeEmptyBrace",
            NodeKind::Wrap { .. } => "Wrap",
            NodeKind::MakeVariant { .. } => "MakeVariant",
            NodeKind::Coerce { .. } => "Coerce",
            NodeKind::Attempt { .. } => "Attempt",
            NodeKind::Retry(_) => "Retry",
            NodeKind::WaitUntil(_) => "WaitUntil",
            NodeKind::Sleep { .. } => "Sleep",
            NodeKind::Loop(_) => "Loop",
            NodeKind::Recur { .. } => "Recur",
            NodeKind::Par { .. } => "Par",
            NodeKind::ParMap { .. } => "ParMap",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub kind: NodeKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CapabilitySignature {
    pub effect: Effect,
    pub idempotent: bool,
    pub input: Type,
    pub output: Type,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FunctionSignature {
    pub params: Vec<Type>,
    pub output: Type,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Signatures {
    pub capabilities: BTreeMap<String, CapabilitySignature>,
    pub functions: BTreeMap<String, FunctionSignature>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub body: Node,
    pub result: Type,
    pub capabilities: Vec<String>,
    pub inputs: BTreeMap<String, Type>,
    pub enums: BTreeMap<String, Arc<EnumType>>,
    pub signatures: Signatures,
}
