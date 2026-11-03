use sx_kernel::diagnostics::Diagnostic;
use sx_kernel::registry::Registry;

pub fn type_name(name: &str) -> String {
    name.split(['.', '-'])
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| first.to_uppercase().chain(chars).collect())
        })
        .collect()
}

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

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')
}

pub fn rewrite(text: &str, registry: &Registry) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        if !c.is_ascii_alphabetic() || (at > 0 && is_name_char(chars[at - 1])) {
            out.push(c);
            at += 1;
            continue;
        }
        let mut reader = Reader { chars: &chars, at, registry };
        if reader.structured_type_starts()
            && let Some(converted) = reader.ty()
        {
            out.push_str(&converted);
            at = reader.at;
            continue;
        }
        let mut end = at;
        while end < chars.len() && is_name_char(chars[end]) {
            end += 1;
        }
        let mut word: String = chars[at..end].iter().collect();
        while word.ends_with(['.', '-']) {
            word.pop();
            end -= 1;
        }
        if registry.capability(&word).is_some() {
            out.push_str(&tool_name(&word));
        } else if registry.types().any(|(name, _)| name == word) {
            out.push_str(&type_name(&word));
        } else {
            out.push_str(&word);
        }
        at = end.max(at + 1);
    }
    out
}

struct Reader<'a> {
    chars: &'a [char],
    at: usize,
    registry: &'a Registry,
}

impl Reader<'_> {
    fn structured_type_starts(&self) -> bool {
        let rest: String = self.chars[self.at..].iter().take(10).collect();
        ["List<", "Option<", "Map<", "Tuple<", "Result<", "Record {", "Record open {"].iter().any(|head| rest.starts_with(head))
    }

    fn eat(&mut self, expected: &str) -> Option<()> {
        for c in expected.chars() {
            if self.chars.get(self.at) != Some(&c) {
                return None;
            }
            self.at += 1;
        }
        Some(())
    }

    fn skip_spaces(&mut self) {
        while self.chars.get(self.at) == Some(&' ') {
            self.at += 1;
        }
    }

    fn name(&mut self) -> Option<String> {
        let start = self.at;
        while self.at < self.chars.len() && is_name_char(self.chars[self.at]) {
            self.at += 1;
        }
        (self.at > start).then(|| self.chars[start..self.at].iter().collect())
    }

    fn arguments(&mut self) -> Option<Vec<String>> {
        self.eat("<")?;
        let mut found = vec![self.ty()?];
        loop {
            self.skip_spaces();
            if self.eat(",").is_some() {
                self.skip_spaces();
                found.push(self.ty()?);
            } else {
                self.eat(">")?;
                return Some(found);
            }
        }
    }

    fn ty(&mut self) -> Option<String> {
        if self.chars.get(self.at) == Some(&'?') {
            self.at += 1;
            return Some("unknown".to_string());
        }
        let name = self.name()?;
        Some(match name.as_str() {
            "Bool" => "boolean".to_string(),
            "String" | "Bytes" => "string".to_string(),
            "Unit" => "null".to_string(),
            "Never" => "never".to_string(),
            "Int" | "Float" | "CallError" => name,
            "List" => {
                let [element] = <[String; 1]>::try_from(self.arguments()?).ok()?;
                if element.contains(' ') && !element.starts_with(['{', '[']) { format!("({element})[]") } else { format!("{element}[]") }
            }
            "Option" => {
                let [inner] = <[String; 1]>::try_from(self.arguments()?).ok()?;
                format!("{inner} | null")
            }
            "Map" => {
                let [_, value] = <[String; 2]>::try_from(self.arguments()?).ok()?;
                format!("Record<string, {value}>")
            }
            "Tuple" => format!("[{}]", self.arguments()?.join(", ")),
            "Result" => {
                let [ok, err] = <[String; 2]>::try_from(self.arguments()?).ok()?;
                format!("Settled<{ok}, {err}>")
            }
            "Record" => {
                self.skip_spaces();
                if self.chars[self.at..].starts_with(&['o', 'p', 'e', 'n']) {
                    self.eat("open")?;
                    self.skip_spaces();
                }
                self.eat("{")?;
                let mut fields = Vec::new();
                loop {
                    self.skip_spaces();
                    if self.eat("}").is_some() {
                        break;
                    }
                    let field = self.name()?;
                    self.eat(":")?;
                    self.skip_spaces();
                    fields.push(format!("{field}: {}", self.ty()?));
                    self.skip_spaces();
                    if self.eat(",").is_none() {
                        self.skip_spaces();
                        self.eat("}")?;
                        break;
                    }
                }
                if fields.is_empty() { "Record<string, never>".to_string() } else { format!("{{ {} }}", fields.join("; ")) }
            }
            other if self.registry.types().any(|(known, _)| known == other) => type_name(other),
            _ => return None,
        })
    }
}
