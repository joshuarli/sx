use sx_kernel::diagnostics::Diagnostic;
use sx_kernel::registry::Registry;

pub use sx_kernel::types::type_name;

pub fn tool_name(id: &str) -> String {
    match id.split_once('.') {
        Some((namespace, name)) => format!("tools.{}.{}", crate::lower::camel(namespace), crate::lower::camel(name)),
        None => id.to_string(),
    }
}

pub(crate) fn in_typescript_terms(diagnostics: Vec<Diagnostic>, registry: &Registry) -> Vec<Diagnostic> {
    diagnostics
        .into_iter()
        .map(|mut diagnostic| {
            if diagnostic.help.as_deref() == Some("typescript") {
                return diagnostic;
            }
            let (message, detail) = in_prose(diagnostic.code, &diagnostic.message, diagnostic.detail.as_deref());
            diagnostic.message = rewrite(&message, registry);
            diagnostic.detail = detail.map(|detail| rewrite(&detail, registry));
            diagnostic.fix = diagnostic.fix.filter(|fix| !is_sx_form(fix)).map(|fix| rewrite(&fix, registry));
            diagnostic.help = Some("typescript".to_string());
            diagnostic
        })
        .collect()
}

const NULL_ADVICE: &str = "This value may be null. Test it first: x != null ? x.f : d, or write x ?? d.";

const SX_HEADS: [&str; 24] = [
    "match", "some", "get", "call", "fn", "if", "when", "let", "do", "loop", "recur", "map", "filter", "reduce", "cond", "str", "float",
    "round", "floor", "ceil", "quot", "rem", "keep", "sort-by",
];

fn method_name(function: &str) -> String {
    match function {
        "js.length" => ".length".to_string(),
        "js.stringify" => "JSON.stringify".to_string(),
        "js.parse" => "JSON.parse".to_string(),
        other => format!(".{}()", other.strip_prefix("js.").unwrap_or(other)),
    }
}

fn in_prose(code: &str, message: &str, detail: Option<&str>) -> (String, Option<String>) {
    let message = prose_message(code, message);
    let detail = detail.and_then(|detail| {
        let lines: Vec<String> = detail.split('\n').filter_map(|line| prose_line(code, line)).collect();
        (!lines.is_empty()).then(|| lines.join("\n"))
    });
    (message, detail)
}

fn prose_message(code: &str, message: &str) -> String {
    if let Some(found) = message.strip_prefix("get reads a field from a record, but this is ") {
        let found = found.trim_end_matches('.');
        return if found.contains("| null") {
            format!("This is {found}, so a field cannot be read from it before it is tested for null.")
        } else {
            format!("This is {found}, which has no fields to read.")
        };
    }
    if let Some(rest) = message.strip_prefix("js.")
        && let Some((name, tail)) = rest.split_once(" takes ")
    {
        let tail = tail.replace("a String or a list", "a string or an array").replace("a list", "an array");
        return format!("{} takes {tail}", method_name(&format!("js.{name}")));
    }
    if message.starts_with("sort-by needs a key") {
        return "A sort comparator must compare numbers or strings (or tuples of them).".to_string();
    }
    if code == "js-equality" {
        return "=== on arrays and objects compares references, and a program cannot rely on that.".to_string();
    }
    if message.starts_with("This loop body always ends in recur") {
        return "This loop body always ends in next(...), so the loop never finishes and can only fail with loop-limit.".to_string();
    }
    if message.starts_with("A tuple's elements have their own types") {
        return "A tuple's elements have their own types, so index it with a literal such as t[0].".to_string();
    }
    if message.starts_with("Whether ") && message.ends_with("which Sx cannot test here.") {
        return message.replace("which Sx cannot test here.", "which this checker cannot decide.");
    }
    message.to_string()
}

fn prose_line(code: &str, line: &str) -> Option<String> {
    if line.starts_with("This value is optional.") || line.starts_with("Match on an optional value before reading") {
        return Some(NULL_ADVICE.to_string());
    }
    if line.starts_with("Sx does not narrow a field by a flag") {
        return Some("A flag does not make the field present: test the field itself.".to_string());
    }
    if line.starts_with("Match on it, or give get a default") {
        return Some("Test it with r.f != null, or give it a default with r.f ?? d.".to_string());
    }
    if line.starts_with("Give the body a branch that does not recur") {
        return Some("Give the body a path that returns done(value).".to_string());
    }
    if line.starts_with("Take it apart instead: (let [") {
        return Some("Take it apart with a pattern instead: const [a, b] = t;".to_string());
    }
    if line.starts_with("A record has fixed fields. Read one with get") {
        return Some("A record has fixed fields: read one with r.f or r[\"f\"].".to_string());
    }
    if line.starts_with("Every branch of an if or match must return the same type") {
        return Some("Every branch of an if or ?: must return the same type.".to_string());
    }
    if line.starts_with("Sx does not convert between number types. (round x)") {
        return Some("Round it first: Math.floor(x), Math.ceil(x), or Math.round(x).".to_string());
    }
    if line.starts_with("A loop binding that starts as") {
        return Some(
            "A loop state field that starts as [], {}, or null gets its type from the values next({...}) gives it, and no next(...) here does. Give next a value of the right shape, or start the field with one."
                .to_string(),
        );
    }
    if code == "type-mismatch" && line == "Expected: a sort key" {
        return None;
    }
    (!speaks_sx(line)).then(|| line.to_string())
}

fn speaks_sx(text: &str) -> bool {
    let opens_a_form = text.match_indices('(').any(|(at, _)| {
        let head = text[at + 1..].split([' ', ')']).next().unwrap_or("");
        SX_HEADS.contains(&head)
    });
    opens_a_form || text.contains("[:") || text.contains("{:") || text.split_whitespace().any(|word| word == "Sx")
}

fn is_sx_form(fix: &str) -> bool {
    fix.starts_with('(') || fix.contains(" :") || fix.contains("(call ") || fix.contains("(get ") || fix.contains("[:")
}

pub fn rewrite(text: &str, registry: &Registry) -> String {
    let mut out = String::new();
    let mut word = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
            word.push(c);
            continue;
        }
        flush(&mut out, &mut word, registry);
        let keyword = c == ':' && chars.peek().is_some_and(char::is_ascii_alphabetic) && out.ends_with([' ', '\n']);
        if !keyword {
            out.push(c);
        }
    }
    flush(&mut out, &mut word, registry);
    out
}

fn flush(out: &mut String, word: &mut String, registry: &Registry) {
    let trimmed = word.trim_end_matches(['.', '-']);
    if registry.capability(trimmed).is_some() {
        out.push_str(&tool_name(trimmed));
        out.push_str(&word[trimmed.len()..]);
    } else {
        out.push_str(word);
    }
    word.clear();
}
