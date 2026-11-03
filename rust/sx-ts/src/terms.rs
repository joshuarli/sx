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
            diagnostic.message = rewrite(&diagnostic.message, registry);
            diagnostic.detail = diagnostic.detail.map(|detail| rewrite(&detail, registry));
            diagnostic.fix = diagnostic.fix.filter(|fix| !is_sx_form(fix)).map(|fix| rewrite(&fix, registry));
            diagnostic.help = Some("typescript".to_string());
            diagnostic
        })
        .collect()
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
