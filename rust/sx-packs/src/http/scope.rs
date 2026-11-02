use std::sync::Arc;

use sx_kernel::authority::Verdict;
use sx_kernel::types::{STRING, Type};
use sx_kernel::values::{RecordValue, Value};

use super::origin::{Origin, Url};
use super::record;

pub fn scope_type() -> Type {
    record(None, vec![("origins", Type::List(Box::new(STRING))), ("methods", Type::List(Box::new(STRING)))])
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    origins: Vec<Origin>,
    methods: Vec<String>,
}

fn strings<'v>(scope: &'v Value, name: &str) -> Option<Vec<&'v str>> {
    let Value::Record(record) = scope else {
        return None;
    };
    match record.get(name)? {
        Value::List(items) => items
            .iter()
            .map(|item| match item {
                Value::String(s) => Some(&**s),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

pub fn is_method(text: &str) -> bool {
    !text.is_empty()
        && text.bytes().all(|b| {
            b.is_ascii_uppercase() || b.is_ascii_digit() || matches!(b, b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~')
        })
}

pub fn prepare(scope: &Value) -> Result<Value, String> {
    let origins = strings(scope, "origins").ok_or("an http scope must be {:origins [ORIGIN ...] :methods [METHOD ...]}")?;
    let methods = strings(scope, "methods").ok_or("an http scope must be {:origins [ORIGIN ...] :methods [METHOD ...]}")?;
    if origins.is_empty() {
        return Err("an http scope needs at least one origin".to_string());
    }
    if methods.is_empty() {
        return Err("an http scope needs at least one method".to_string());
    }
    let mut normalized: Vec<String> = Vec::new();
    for origin in origins {
        let parsed = Origin::parse(origin).map_err(|reason| format!("origin {origin:?}: {reason}"))?;
        let text = parsed.to_string();
        if !normalized.contains(&text) {
            normalized.push(text);
        }
    }
    let mut accepted: Vec<&str> = Vec::new();
    for method in methods {
        if !is_method(method) {
            return Err(format!("method {method:?} is not an uppercase HTTP method token"));
        }
        if !accepted.contains(&method) {
            accepted.push(method);
        }
    }
    Ok(Value::Record(RecordValue::new(vec![
        (Arc::from("origins"), Value::list(normalized.iter().map(|o| Value::string(o)).collect())),
        (Arc::from("methods"), Value::list(accepted.into_iter().map(Value::string).collect())),
    ])))
}

impl Scope {
    pub fn of(prepared: &Value) -> Result<Scope, String> {
        let origins = strings(prepared, "origins").ok_or("the http scope has no origins")?;
        let methods = strings(prepared, "methods").ok_or("the http scope has no methods")?;
        Ok(Scope {
            origins: origins.into_iter().map(Origin::parse).collect::<Result<_, _>>()?,
            methods: methods.into_iter().map(str::to_string).collect(),
        })
    }

    pub fn permits(&self, origin: &Origin, method: &str) -> Result<(), String> {
        if !self.origins.contains(origin) {
            return Err(format!("{origin} is outside every granted origin"));
        }
        if !self.methods.iter().any(|granted| granted == method) {
            return Err(format!("method {method} is not granted"));
        }
        Ok(())
    }

    pub fn permits_url(&self, url: &str, method: &str) -> Result<Url, String> {
        let parsed = Url::parse(url)?;
        self.permits(&parsed.origin, method)?;
        Ok(parsed)
    }
}

fn text_field<'v>(value: &'v Value, name: &str) -> Option<&'v str> {
    match value {
        Value::Record(record) => match record.get(name)? {
            Value::String(s) => Some(s),
            _ => None,
        },
        _ => None,
    }
}

pub fn verdict_of(scope: &Value, request: Result<(&str, &str), &'static str>) -> Verdict {
    let checked = request.map_err(str::to_string).and_then(|(url, method)| {
        let scope = Scope::of(scope)?;
        scope.permits_url(url, method)
    });
    match checked {
        Ok(_) => Verdict::Allowed,
        Err(reason) => Verdict::Denied(reason),
    }
}

pub fn check_request(scope: &Value, input: &Value) -> Verdict {
    let request = text_field(input, "url")
        .zip(text_field(input, "method"))
        .ok_or("the input has no method and url");
    verdict_of(scope, request)
}

pub fn check_get(scope: &Value, input: &Value) -> Verdict {
    let request = text_field(input, "url").map(|url| (url, "GET")).ok_or("the input has no url");
    verdict_of(scope, request)
}

pub fn check_endpoint(scope: &Value, origin: &Origin, method: &str) -> Verdict {
    match Scope::of(scope).and_then(|scope| scope.permits(origin, method)) {
        Ok(()) => Verdict::Allowed,
        Err(reason) => Verdict::Denied(reason),
    }
}
