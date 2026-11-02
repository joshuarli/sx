mod order;

use order::{Definition, order};

use crate::diagnostics::Diagnostic;
use crate::syntax::{Items, Node, NodeKind, Span, show};

const DEF_SHAPE: &str = "def takes a name and a value: (def name expr).";
const DEFN_SHAPE: &str = "defn takes a name, a parameter vector, and one body expression: (defn name [params] body).";

fn head(node: &Node) -> Option<&str> {
    let NodeKind::Form(items) = &node.kind else {
        return None;
    };
    match &items.first()?.kind {
        NodeKind::Symbol(name) => Some(name),
        _ => None,
    }
}

fn is_definition(node: &Node) -> bool {
    matches!(head(node), Some("def" | "defn"))
}

fn symbol(text: &str, span: Span) -> Node {
    Node { kind: NodeKind::Symbol(text.to_string()), span }
}

fn form(items: Vec<Node>, span: Span) -> Node {
    Node { kind: NodeKind::Form(Items::new(items)), span }
}

fn arity(span: Span, shape: &str, items: &[Node]) -> Diagnostic {
    let docstring = shape == DEFN_SHAPE && matches!(items.get(2).map(|n| &n.kind), Some(NodeKind::String(_)));
    let count = items.len() - 1;
    let found = format!("Found: {count} argument{}", if count == 1 { "" } else { "s" });
    let detail = if docstring { format!("{found}\nSx has no docstrings. Leave the text out.") } else { found };
    Diagnostic::new("arity", span, shape).detail(detail).help("let")
}

fn define(node: Node, diagnostics: &mut Vec<Diagnostic>) -> Option<(Node, Node)> {
    let span = node.span;
    let is_function = head(&node) == Some("defn");
    let NodeKind::Form(items) = node.kind else {
        return None;
    };
    let mut items = items.into_vec();
    if is_function {
        if items.len() != 4 {
            diagnostics.push(arity(span, DEFN_SHAPE, &items));
            return None;
        }
        let body = items.pop()?;
        let params = items.pop()?;
        let name = items.pop()?;
        if !matches!(params.kind, NodeKind::List(_)) {
            let message = "A defn takes its parameters as a vector: (defn name [params] body).";
            diagnostics.push(Diagnostic::new("bad-pattern", params.span, message).help("let"));
            return None;
        }
        let function = form(vec![symbol("fn", items.pop()?.span), params, body], span);
        return Some((name, function));
    }
    if items.len() != 3 {
        diagnostics.push(arity(span, DEF_SHAPE, &items));
        return None;
    }
    let value = items.pop()?;
    let name = items.pop()?;
    Some((name, value))
}

fn missing_result(definition: &Node) -> Diagnostic {
    let defined = match &definition.kind {
        NodeKind::Form(items) => match items.get(1).map(|name| &name.kind) {
            Some(NodeKind::Symbol(name)) if name != "_" => Some(name.as_str()),
            _ => None,
        },
        _ => None,
    };
    let detail = match defined {
        Some(name) => format!("Add the result after it, such as {name}."),
        None => "Add the result after it.".to_string(),
    };
    let message = "A program ends with an expression, its result, but this last form is a definition.";
    Diagnostic::new("missing-result", definition.span, message).detail(detail).help("let")
}

fn misplaced(form: &Node) -> Diagnostic {
    Diagnostic::new(
        "multiple-expressions",
        form.span,
        "Only (def ...) and (defn ...) forms come before the final expression.",
    )
    .detail("A step that runs only for its effect is a definition named _.")
    .fix(format!("(def _ {})", show(form)))
    .help("let")
}

pub fn assemble(mut forms: Vec<Node>) -> Result<Node, Vec<Diagnostic>> {
    let Some(result) = forms.pop() else {
        return Err(Vec::new());
    };
    let mut diagnostics = Vec::new();
    if is_definition(&result) {
        diagnostics.push(missing_result(&result));
    }
    let (Some(first), Some(last)) = (forms.first().map(|f| f.span), forms.last().map(|f| f.span)) else {
        return if diagnostics.is_empty() { Ok(result) } else { Err(diagnostics) };
    };
    let mut definitions = Vec::new();
    for form in forms {
        if !is_definition(&form) {
            diagnostics.push(misplaced(&form));
        } else {
            let is_function = head(&form) == Some("defn");
            if let Some((name, value)) = define(form, &mut diagnostics) {
                definitions.push(Definition::new(is_function, name, value));
            }
        }
    }
    let mut bindings = Vec::new();
    for definition in order(definitions, &mut diagnostics) {
        bindings.extend([definition.name, definition.value]);
    }
    if !diagnostics.is_empty() {
        diagnostics.sort_by_key(|d| (d.span.start.line, d.span.start.column));
        return Err(diagnostics);
    }
    let list = Node { kind: NodeKind::List(Items::new(bindings)), span: Span::new(first.start, last.end) };
    let wrap = |body: Node| {
        let span = Span::new(first.start, body.span.end);
        form(vec![symbol("let", Span::new(first.start, first.start)), list, body], span)
    };
    let is_script = head(&result) == Some("script");
    match result {
        Node { kind: NodeKind::Form(items), span } if is_script => {
            let mut items = items.into_vec();
            if items.len() == 3 {
                let body = items.pop().map(wrap);
                items.extend(body);
            }
            Ok(form(items, span))
        }
        other => Ok(wrap(other)),
    }
}
