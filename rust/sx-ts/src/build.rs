use sx_kernel::syntax::{Fields, Items, Keyword, Node, NodeKind, RecordField, Span};

pub fn sym(name: &str, span: Span) -> Node {
    Node { kind: NodeKind::Symbol(name.to_string()), span }
}

pub fn kw(name: &str, span: Span) -> Node {
    Node { kind: NodeKind::Keyword(name.to_string()), span }
}

pub fn int(value: i64, span: Span) -> Node {
    Node { kind: NodeKind::Int(value), span }
}

pub fn float(value: f64, span: Span) -> Node {
    Node { kind: NodeKind::Float(value), span }
}

pub fn duration(milliseconds: i64, span: Span) -> Node {
    Node { kind: NodeKind::Duration(milliseconds), span }
}

pub fn string(value: &str, span: Span) -> Node {
    Node { kind: NodeKind::String(value.to_string()), span }
}

pub fn boolean(value: bool, span: Span) -> Node {
    Node { kind: NodeKind::Bool(value), span }
}

pub fn list(items: Vec<Node>, span: Span) -> Node {
    Node { kind: NodeKind::List(Items::new(items)), span }
}

pub fn form(items: Vec<Node>, span: Span) -> Node {
    Node { kind: NodeKind::Form(Items::new(items)), span }
}

pub fn app(head: &str, args: Vec<Node>, span: Span) -> Node {
    let mut items = vec![sym(head, span)];
    items.extend(args);
    form(items, span)
}

pub fn record(fields: Vec<(String, Node)>, span: Span) -> Node {
    let fields = fields
        .into_iter()
        .map(|(name, value)| RecordField { key: Keyword { name, span }, value })
        .collect();
    Node { kind: NodeKind::Record(Fields::new(fields)), span }
}

pub fn unit(span: Span) -> Node {
    sym("unit", span)
}

pub fn none(span: Span) -> Node {
    sym("none", span)
}

pub fn some(value: Node, span: Span) -> Node {
    app("some", vec![value], span)
}

pub fn dup(node: &Node) -> Node {
    let kind = match &node.kind {
        NodeKind::Symbol(s) => NodeKind::Symbol(s.clone()),
        NodeKind::Keyword(s) => NodeKind::Keyword(s.clone()),
        NodeKind::Int(v) => NodeKind::Int(*v),
        NodeKind::Float(v) => NodeKind::Float(*v),
        NodeKind::Duration(v) => NodeKind::Duration(*v),
        NodeKind::String(s) => NodeKind::String(s.clone()),
        NodeKind::Bool(b) => NodeKind::Bool(*b),
        NodeKind::List(items) => NodeKind::List(Items::new(items.iter().map(dup).collect())),
        NodeKind::Form(items) => NodeKind::Form(Items::new(items.iter().map(dup).collect())),
        NodeKind::Record(fields) => NodeKind::Record(Fields::new(
            fields
                .iter()
                .map(|f| RecordField { key: f.key.clone(), value: dup(&f.value) })
                .collect(),
        )),
    };
    Node { kind, span: node.span }
}

pub fn head(node: &Node) -> Option<&str> {
    let NodeKind::Form(items) = &node.kind else {
        return None;
    };
    match &items.first()?.kind {
        NodeKind::Symbol(name) => Some(name),
        _ => None,
    }
}

pub fn let_(bindings: Vec<(Node, Node)>, body: Node, span: Span) -> Node {
    if bindings.is_empty() {
        return body;
    }
    let mut flat: Vec<Node> = Vec::new();
    for (pattern, value) in bindings {
        flat.push(pattern);
        flat.push(value);
    }
    let mut body = body;
    if head(&body) == Some("let")
        && let NodeKind::Form(items) = body.kind
    {
        let mut inner = items.into_vec();
        if inner.len() == 3 && matches!(inner[1].kind, NodeKind::List(_)) {
            let rest = inner.pop();
            let inner_bindings = inner.pop();
            if let (Some(rest), Some(Node { kind: NodeKind::List(more), .. })) = (rest, inner_bindings) {
                flat.extend(more.into_vec());
                let span = Span::new(span.start, rest.span.end.max(span.end));
                return form(vec![sym("let", span), list(flat, span), rest], span);
            }
            unreachable!("a let with three parts has a binding list")
        }
        body = form(inner, body.span);
    }
    form(vec![sym("let", span), list(flat, span), body], span)
}

pub fn seq(effect: Node, rest: Node, span: Span) -> Node {
    let_(vec![(sym("_", span), effect)], rest, span)
}
