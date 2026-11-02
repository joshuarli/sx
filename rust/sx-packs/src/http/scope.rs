use std::sync::Arc;

use sx_kernel::authority::Verdict;
use sx_kernel::json::Json;
use sx_kernel::types::{STRING, Type};
use sx_kernel::values::{MapValue, RecordValue, Value};

use super::origin::{Origin, Url};
use super::record;
use super::transport::Headers;

pub fn scope_type() -> Type {
    record(
        None,
        vec![
            ("origins", Type::List(Box::new(STRING))),
            ("methods", Type::List(Box::new(STRING))),
            ("headers", Type::Map(Box::new(Type::Map(Box::new(STRING))))),
        ],
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    origins: Vec<Origin>,
    methods: Vec<String>,
    injected: Vec<(Origin, Headers)>,
}

pub fn hide_secrets(scope: &Value) -> Value {
    let Value::Record(record) = scope else {
        return scope.clone();
    };
    let hidden = |headers: &Value| match headers {
        Value::Map(by_origin) => Value::Map(MapValue::from_pairs(by_origin.entries().iter().map(|(origin, pairs)| {
            let masked = match pairs {
                Value::Map(pairs) => Value::Map(MapValue::from_pairs(
                    pairs.entries().iter().map(|(name, _)| (name.clone(), Value::string("[hidden]"))),
                )),
                other => other.clone(),
            };
            (origin.clone(), masked)
        }))),
        other => other.clone(),
    };
    Value::Record(RecordValue::new(
        record
            .fields()
            .iter()
            .map(|(name, value)| (name.clone(), if &**name == "headers" { hidden(value) } else { value.clone() }))
            .collect(),
    ))
}

pub fn resolve(written: &Json) -> Result<Json, String> {
    let Json::Object(entries) = written else {
        return Ok(written.clone());
    };
    let mut resolved: Vec<(String, Json)> = Vec::with_capacity(entries.len() + 1);
    for (key, value) in entries {
        resolved.push((key.clone(), if key == "headers" { resolve_headers(value)? } else { value.clone() }));
    }
    if !resolved.iter().any(|(key, _)| key == "headers") {
        resolved.push(("headers".to_string(), Json::Object(Vec::new())));
    }
    Ok(Json::Object(resolved))
}

fn resolve_headers(written: &Json) -> Result<Json, String> {
    let Json::Object(origins) = written else {
        return Ok(written.clone());
    };
    let mut resolved = Vec::with_capacity(origins.len());
    for (origin, headers) in origins {
        let Json::Object(headers) = headers else {
            resolved.push((origin.clone(), headers.clone()));
            continue;
        };
        let mut values = Vec::with_capacity(headers.len());
        for (name, value) in headers {
            values.push((name.clone(), resolve_secret(name, value)?));
        }
        resolved.push((origin.clone(), Json::Object(values)));
    }
    Ok(Json::Object(resolved))
}

fn resolve_secret(header: &str, written: &Json) -> Result<Json, String> {
    let Json::Object(entries) = written else {
        return Ok(written.clone());
    };
    let [(key, Json::String(variable))] = entries.as_slice() else {
        return Ok(written.clone());
    };
    if key != "env" {
        return Ok(written.clone());
    }
    if variable.is_empty() || variable.contains(['=', '\0']) {
        return Err(format!("header {header:?}: {variable:?} is not a valid environment variable name"));
    }
    match std::env::var(variable) {
        Ok(value) => Ok(Json::String(value)),
        Err(std::env::VarError::NotPresent) => {
            Err(format!("header {header:?}: the environment variable {variable} is not set"))
        }
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(format!("header {header:?}: the environment variable {variable} is not valid UTF-8"))
        }
    }
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
    let injected = prepare_headers(scope, &normalized)?;
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
        (Arc::from("headers"), injected),
    ])))
}

fn prepare_headers(scope: &Value, granted: &[String]) -> Result<Value, String> {
    let Value::Record(record) = scope else {
        return Err("an http scope must be a record".to_string());
    };
    let Some(Value::Map(by_origin)) = record.get("headers") else {
        return Err("an http scope needs a headers map".to_string());
    };
    let mut prepared: Vec<(Arc<str>, Value)> = Vec::new();
    for (written, headers) in by_origin.entries() {
        let origin = Origin::parse(written).map_err(|reason| format!("headers for origin {written:?}: {reason}"))?.to_string();
        if !granted.contains(&origin) {
            return Err(format!("headers are given for {origin}, which is not one of the granted origins"));
        }
        if prepared.iter().any(|(known, _)| **known == *origin) {
            return Err(format!("headers for {origin} are given twice"));
        }
        let Value::Map(pairs) = headers else {
            return Err(format!("headers for {origin} must map names to values"));
        };
        let mut checked = Headers::new();
        let mut lowered: Vec<(Arc<str>, Value)> = Vec::new();
        for (name, value) in pairs.entries() {
            let Value::String(value) = value else {
                return Err(format!("header {name:?} for {origin} must be a string"));
            };
            checked.insert_secret(name, value).map_err(|failure| format!("headers for {origin}: {}", failure.message))?;
            lowered.push((Arc::from(name.to_ascii_lowercase().as_str()), Value::string(value)));
        }
        prepared.push((Arc::from(origin.as_str()), Value::Map(MapValue::from_pairs(lowered))));
    }
    Ok(Value::Map(MapValue::from_pairs(prepared)))
}

impl Scope {
    pub fn of(prepared: &Value) -> Result<Scope, String> {
        let origins = strings(prepared, "origins").ok_or("the http scope has no origins")?;
        let methods = strings(prepared, "methods").ok_or("the http scope has no methods")?;
        let Value::Record(record) = prepared else {
            return Err("the http scope is not a record".to_string());
        };
        let Some(Value::Map(by_origin)) = record.get("headers") else {
            return Err("the http scope has no headers map".to_string());
        };
        let mut injected = Vec::new();
        for (origin, headers) in by_origin.entries() {
            let Value::Map(pairs) = headers else {
                return Err("the http scope holds malformed headers".to_string());
            };
            let mut collected = Headers::new();
            for (name, value) in pairs.entries() {
                let Value::String(value) = value else {
                    return Err("the http scope holds a malformed header".to_string());
                };
                collected.insert_secret(name, value).map_err(|failure| failure.message)?;
            }
            injected.push((Origin::parse(origin)?, collected));
        }
        Ok(Scope {
            origins: origins.into_iter().map(Origin::parse).collect::<Result<_, _>>()?,
            methods: methods.into_iter().map(str::to_string).collect(),
            injected,
        })
    }

    pub fn injected_for(&self, origin: &Origin) -> Option<&Headers> {
        self.injected.iter().find(|(known, _)| known == origin).map(|(_, headers)| headers)
    }

    pub fn secrets(&self) -> Vec<Vec<u8>> {
        let mut found: Vec<Vec<u8>> = Vec::new();
        for value in self.injected.iter().flat_map(|(_, headers)| headers.values()) {
            found.push(value.to_vec());
            if let Some(space) = value.iter().position(|b| *b == b' ')
                && let Some(token) = value.get(space + 1..)
                && token.len() >= 8
            {
                found.push(token.to_vec());
            }
        }
        found.retain(|secret| !secret.is_empty());
        found
    }

    pub fn permits_headers<'n>(&self, origin: &Origin, names: impl Iterator<Item = &'n str>) -> Result<(), String> {
        let Some(injected) = self.injected_for(origin) else {
            return Ok(());
        };
        for name in names {
            let lowered = name.to_ascii_lowercase();
            if injected.has(&lowered) {
                return Err(format!("the grant sets the header {lowered} for {origin}, so a request cannot set it"));
            }
        }
        Ok(())
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

pub struct Checked<'a> {
    pub url: &'a str,
    pub method: &'a str,
    pub header_names: Vec<String>,
}

pub fn verdict_of(scope: &Value, request: Result<Checked<'_>, &'static str>) -> Verdict {
    let checked = request.map_err(str::to_string).and_then(|request| {
        let scope = Scope::of(scope)?;
        let url = scope.permits_url(request.url, request.method)?;
        scope.permits_headers(&url.origin, request.header_names.iter().map(String::as_str))
    });
    match checked {
        Ok(()) => Verdict::Allowed,
        Err(reason) => Verdict::Denied(reason),
    }
}

fn header_names(input: &Value) -> Vec<String> {
    match input {
        Value::Record(record) => match record.get("headers") {
            Some(Value::Map(map)) => map.entries().iter().map(|(name, _)| name.to_string()).collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

pub fn check_request(scope: &Value, input: &Value) -> Verdict {
    let request = text_field(input, "url")
        .zip(text_field(input, "method"))
        .map(|(url, method)| Checked { url, method, header_names: header_names(input) })
        .ok_or("the input has no method and url");
    verdict_of(scope, request)
}

pub fn check_get(scope: &Value, input: &Value) -> Verdict {
    let request = text_field(input, "url")
        .map(|url| Checked { url, method: "GET", header_names: header_names(input) })
        .ok_or("the input has no url");
    verdict_of(scope, request)
}

pub fn check_endpoint(scope: &Value, origin: &Origin, method: &str, headers: &Headers) -> Verdict {
    match Scope::of(scope).and_then(|scope| {
        scope.permits(origin, method)?;
        scope.permits_headers(origin, headers.names())
    }) {
        Ok(()) => Verdict::Allowed,
        Err(reason) => Verdict::Denied(reason),
    }
}
