use sx_kernel::syntax::{Node, NodeKind, show};

const WIDTH: usize = 100;

pub fn pretty(node: &Node) -> String {
    let mut out = String::new();
    write(node, 0, &mut out);
    out.push('\n');
    out
}

fn fits(text: &str, indent: usize) -> bool {
    indent + text.chars().count() <= WIDTH && !text.contains('\n')
}

fn pad(out: &mut String, indent: usize) {
    out.push('\n');
    out.push_str(&" ".repeat(indent));
}

fn write(node: &Node, indent: usize, out: &mut String) {
    let flat = show(node);
    if fits(&flat, indent) {
        out.push_str(&flat);
        return;
    }
    match &node.kind {
        NodeKind::Form(items) => write_form(items, indent, out),
        NodeKind::List(items) => write_sequence('[', ']', items, indent, out),
        NodeKind::Record(fields) => {
            out.push('{');
            for (i, field) in fields.iter().enumerate() {
                if i > 0 {
                    pad(out, indent + 1);
                }
                let key = format!(":{} ", field.key.name);
                out.push_str(&key);
                write(&field.value, indent + 1 + key.chars().count(), out);
            }
            out.push('}');
        }
        _ => out.push_str(&flat),
    }
}

fn write_sequence(open: char, close: char, items: &[Node], indent: usize, out: &mut String) {
    out.push(open);
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            pad(out, indent + 1);
        }
        write(item, indent + 1, out);
    }
    out.push(close);
}

fn write_form(items: &[Node], indent: usize, out: &mut String) {
    let head = match items.first().map(|n| &n.kind) {
        Some(NodeKind::Symbol(name)) => Some(name.as_str()),
        _ => None,
    };
    if head == Some("let")
        && let [_, bindings, body] = items
        && let NodeKind::List(pairs) = &bindings.kind
    {
        out.push_str("(let [");
        for (i, pair) in pairs.chunks(2).enumerate() {
            if i > 0 {
                pad(out, indent + 6);
            }
            let pattern = show(&pair[0]);
            out.push_str(&pattern);
            if let Some(value) = pair.get(1) {
                out.push(' ');
                write(value, indent + 6 + pattern.chars().count() + 1, out);
            }
        }
        out.push(']');
        pad(out, indent + 2);
        write(body, indent + 2, out);
        out.push(')');
        return;
    }
    out.push('(');
    let mut column = indent + 1;
    for (i, item) in items.iter().enumerate() {
        if i == 0 {
            let name = show(item);
            column += name.chars().count();
            out.push_str(&name);
        } else if i == 1 && head.is_some() && matches!(head, Some("fn" | "if" | "match" | "loop" | "reduce" | "map" | "filter" | "attempt" | "retry" | "wait-until" | "par-map")) {
            out.push(' ');
            write(item, column + 1, out);
        } else {
            pad(out, indent + 2);
            write(item, indent + 2, out);
        }
    }
    out.push(')');
}
