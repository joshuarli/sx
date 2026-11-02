use std::collections::BTreeMap;
use std::sync::Arc;

use crate::json::Json;
use crate::registry::Effect;
use crate::syntax::Span;
use crate::types::{CallErrorKind, EnumType, Type};
use crate::values::{RecordValue, Value};

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
    ToJson,
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
    Range,
    Enumerate,
    SafeInt,
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
    ("to-json", PreludeOp::ToJson),
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
    ("range", PreludeOp::Range),
    ("enumerate", PreludeOp::Enumerate),
    ("safe-int", PreludeOp::SafeInt),
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
    Fail { message: Box<Node> },
    Log { args: Vec<Node> },
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
            NodeKind::Fail { .. } => "Fail",
            NodeKind::Log { .. } => "Log",
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

#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(Arc<str>),
    Record(Vec<(String, Literal)>),
    List(Vec<Literal>),
    Absent,
    Present(Box<Literal>),
}

impl Literal {
    pub fn of(node: &Node) -> Option<Literal> {
        match &node.kind {
            NodeKind::Literal { value, .. } => match value {
                Value::Bool(b) => Some(Literal::Bool(*b)),
                Value::Int(n) => Some(Literal::Int(*n)),
                Value::Float(f) => Some(Literal::Float(f.0)),
                Value::String(s) => Some(Literal::Text(s.clone())),
                Value::None => Some(Literal::Absent),
                _ => None,
            },
            NodeKind::Wrap { kind: WrapKind::Some, value, .. } => Literal::of(value).map(|inner| Literal::Present(Box::new(inner))),
            NodeKind::MakeRecord { fields } => fields
                .iter()
                .map(|(name, part)| Literal::of(part).map(|l| (name.clone(), l)))
                .collect::<Option<Vec<_>>>()
                .map(Literal::Record),
            NodeKind::MakeList { items, .. } => items.iter().map(Literal::of).collect::<Option<Vec<_>>>().map(Literal::List),
            _ => None,
        }
    }

    pub fn json(&self) -> Json {
        match self {
            Literal::Bool(b) => Json::Bool(*b),
            Literal::Int(n) => Json::Int(*n),
            Literal::Float(f) => Json::Float(*f),
            Literal::Text(s) => Json::string(&**s),
            Literal::Record(fields) => Json::Object(fields.iter().map(|(n, v)| (n.clone(), v.json())).collect()),
            Literal::List(items) => Json::Array(items.iter().map(Literal::json).collect()),
            Literal::Absent => Json::Null,
            Literal::Present(inner) => inner.json(),
        }
    }

    pub fn value(&self) -> Value {
        match self {
            Literal::Bool(b) => Value::Bool(*b),
            Literal::Int(n) => Value::Int(*n),
            Literal::Float(f) => Value::float(*f),
            Literal::Text(s) => Value::String(s.clone()),
            Literal::Record(fields) => {
                Value::Record(RecordValue::new(fields.iter().map(|(n, v)| (Arc::from(n.as_str()), v.value())).collect()))
            }
            Literal::List(items) => Value::list(items.iter().map(Literal::value).collect()),
            Literal::Absent => Value::None,
            Literal::Present(inner) => Value::some(inner.value()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CallSite<'p> {
    pub capability: &'p str,
    pub span: Span,
    pub input: Option<Literal>,
}

fn children(node: &Node) -> Vec<&Node> {
    let mut out: Vec<&Node> = Vec::new();
    match &node.kind {
        NodeKind::Literal { .. } | NodeKind::Variable { .. } | NodeKind::Sleep { .. } | NodeKind::MakeEmptyBrace { .. } => {}
        NodeKind::Let(binding) => out.extend([&*binding.value, &*binding.body]),
        NodeKind::LocalFunction { body, .. } | NodeKind::Attempt { body } | NodeKind::Fail { message: body } => out.push(body),
        NodeKind::If(branch) => out.extend([&*branch.condition, &*branch.then, &*branch.otherwise]),
        NodeKind::Match(arms) => {
            out.push(&arms.scrutinee);
            out.extend(arms.clauses.iter().map(|(_, body)| body));
        }
        NodeKind::Lambda(lambda) => out.push(&lambda.body),
        NodeKind::ApplyLocal { function, args, .. } => {
            out.push(&function.body);
            out.extend(args);
        }
        NodeKind::Prelude { args, .. }
        | NodeKind::CoreCall { args, .. }
        | NodeKind::Recur { args }
        | NodeKind::Log { args } => out.extend(args),
        NodeKind::Call { argument, .. } => out.push(argument),
        NodeKind::Get { record, .. } => out.push(record),
        NodeKind::MakeRecord { fields } => out.extend(fields.iter().map(|(_, part)| part)),
        NodeKind::MakeList { items, .. } | NodeKind::MakeTuple { items } | NodeKind::MakeBracket { items, .. } => {
            out.extend(items);
        }
        NodeKind::Wrap { value, .. } | NodeKind::Coerce { value, .. } => out.push(value),
        NodeKind::MakeVariant { payload, .. } => out.extend(payload.as_deref()),
        NodeKind::Retry(retry) => out.push(&retry.body),
        NodeKind::WaitUntil(wait) => out.extend([&*wait.predicate.body, &*wait.body]),
        NodeKind::Loop(repeat) => {
            out.extend(&repeat.initial);
            out.push(&repeat.body);
        }
        NodeKind::Par { branches } => out.extend(branches),
        NodeKind::ParMap { function, items, .. } => out.extend([&*function.body, &**items]),
    }
    out
}

pub fn call_sites(root: &Node) -> Vec<CallSite<'_>> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if let NodeKind::Call { capability, argument } = &node.kind {
            found.push(CallSite { capability, span: node.span, input: Literal::of(argument) });
        }
        stack.extend(children(node).into_iter().rev());
    }
    found
}
