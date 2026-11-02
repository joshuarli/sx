use sx_kernel::ir::{Node, NodeKind};
use sx_kernel::json::Json;
use sx_kernel::values::Value;

pub const RUNTIME: &str = "computed at run time";

enum Literal {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Record(Vec<(String, Literal)>),
    List(Vec<Literal>),
}

impl Literal {
    fn json(&self) -> Json {
        match self {
            Literal::Bool(b) => Json::Bool(*b),
            Literal::Int(n) => Json::Int(*n),
            Literal::Float(f) => Json::Float(*f),
            Literal::Text(s) => Json::String(s.clone()),
            Literal::Record(fields) => Json::Object(fields.iter().map(|(n, v)| (n.clone(), v.json())).collect()),
            Literal::List(items) => Json::Array(items.iter().map(Literal::json).collect()),
        }
    }
}

fn literal(node: &Node) -> Option<Literal> {
    match &node.kind {
        NodeKind::Literal { value, .. } => match value {
            Value::Bool(b) => Some(Literal::Bool(*b)),
            Value::Int(n) => Some(Literal::Int(*n)),
            Value::Float(f) => Some(Literal::Float(f.0)),
            Value::String(s) => Some(Literal::Text(s.to_string())),
            _ => None,
        },
        NodeKind::MakeRecord { fields } => {
            let parts: Option<Vec<(String, Literal)>> =
                fields.iter().map(|(name, part)| literal(part).map(|l| (name.clone(), l))).collect();
            parts.map(Literal::Record)
        }
        NodeKind::MakeList { items, .. } => items.iter().map(literal).collect::<Option<Vec<_>>>().map(Literal::List),
        _ => None,
    }
}

fn push_children<'a>(node: &'a Node, stack: &mut Vec<&'a Node>) {
    let mut ordered: Vec<&'a Node> = Vec::new();
    match &node.kind {
        NodeKind::Literal { .. }
        | NodeKind::Variable { .. }
        | NodeKind::Sleep { .. }
        | NodeKind::MakeEmptyBrace { .. } => {}
        NodeKind::Let(binding) => ordered.extend([&*binding.value, &*binding.body]),
        NodeKind::LocalFunction { body, .. } | NodeKind::Attempt { body } => ordered.push(body),
        NodeKind::If(branch) => ordered.extend([&*branch.condition, &*branch.then, &*branch.otherwise]),
        NodeKind::Match(matched) => {
            ordered.push(&matched.scrutinee);
            ordered.extend(matched.clauses.iter().map(|(_, result)| result));
        }
        NodeKind::Lambda(lambda) => ordered.push(&lambda.body),
        NodeKind::ApplyLocal { function, args, .. } => {
            ordered.push(&function.body);
            ordered.extend(args);
        }
        NodeKind::Prelude { args, .. } | NodeKind::CoreCall { args, .. } | NodeKind::Recur { args } => ordered.extend(args),
        NodeKind::Call { argument, .. } => ordered.push(argument),
        NodeKind::Get { record, .. } => ordered.push(record),
        NodeKind::MakeRecord { fields } => ordered.extend(fields.iter().map(|(_, value)| value)),
        NodeKind::MakeList { items, .. } | NodeKind::MakeTuple { items } | NodeKind::MakeBracket { items, .. } => {
            ordered.extend(items);
        }
        NodeKind::Wrap { value, .. } | NodeKind::Coerce { value, .. } => ordered.push(value),
        NodeKind::MakeVariant { payload, .. } => ordered.extend(payload.as_deref()),
        NodeKind::Retry(retry) => ordered.push(&retry.body),
        NodeKind::WaitUntil(wait) => ordered.extend([&*wait.predicate.body, &*wait.body]),
        NodeKind::Loop(looped) => {
            ordered.extend(&looped.initial);
            ordered.push(&looped.body);
        }
        NodeKind::Par { branches } => ordered.extend(branches),
        NodeKind::ParMap { function, items, .. } => ordered.extend([&*function.body, &**items]),
    }
    stack.extend(ordered.into_iter().rev());
}

pub fn literal_calls(root: &Node) -> Vec<(String, Option<Json>)> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(current) = stack.pop() {
        if let NodeKind::Call { capability, argument } = &current.kind {
            found.push((capability.clone(), literal(argument).map(|l| l.json())));
        }
        push_children(current, &mut stack);
    }
    found
}
