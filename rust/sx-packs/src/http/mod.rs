mod endpoint;
mod origin;
mod scope;
mod transport;

use std::sync::Arc;

use sx_kernel::authority::ScopeCheck;
use sx_kernel::registry::{Capability, Effect, Pack, PackCapability};
use sx_kernel::types::{CallErrorKind, INT, RecordType, STRING, Type};
use sx_kernel::values::{MapValue, RecordValue, Value};

use crate::{Failure, Performer};

pub use endpoint::{Endpoint, EndpointLoader, Endpoints};
pub use origin::{Origin, Scheme, Url};
pub use scope::{Scope, prepare, scope_type};
pub use transport::{Headers, Limits, Outgoing, Reply, USER_AGENT, send, status_kind};

pub const REQUEST: &str = "http.request";
pub const GET: &str = "http.get";

const RESPONSE: &str = "http.Response";

pub(crate) fn record(name: Option<&str>, fields: Vec<(&str, Type)>) -> Type {
    Type::Record(RecordType {
        fields: fields.into_iter().map(|(n, t)| (n.to_string(), t)).collect(),
        name: name.map(str::to_string),
        open: false,
    })
}

fn headers_type() -> Type {
    Type::Map(Box::new(STRING))
}

fn response_type() -> Type {
    record(Some(RESPONSE), vec![("status", INT), ("headers", headers_type()), ("body", STRING)])
}

fn capability(id: &str, input: Type, description: &str, effect: Effect, check: ScopeCheck) -> PackCapability {
    PackCapability {
        capability: Capability {
            id: id.to_string(),
            effect,
            input,
            output: response_type(),
            description: description.to_string(),
            idempotent: false,
            scope: Some(scope_type()),
        },
        check: Some(check),
    }
}

pub fn pack() -> Pack {
    let request_input = record(
        None,
        vec![
            ("method", STRING),
            ("url", STRING),
            ("headers", headers_type()),
            ("body", Type::Option(Box::new(STRING))),
        ],
    );
    let get_input = record(None, vec![("url", STRING), ("headers", headers_type())]);
    Pack {
        id: "http".to_string(),
        types: vec![(RESPONSE.to_string(), response_type())],
        capabilities: vec![
            capability(
                REQUEST,
                request_input,
                "Sends an HTTP request and returns its status, headers, and UTF-8 body.",
                Effect::Write,
                ScopeCheck::new(prepare, scope::check_request),
            ),
            capability(
                GET,
                get_input,
                "Sends an HTTP GET request and returns its status, headers, and UTF-8 body.",
                Effect::Read,
                ScopeCheck::new(prepare, scope::check_get),
            ),
        ],
    }
}

pub fn performer(capability: &str) -> Option<Performer> {
    match capability {
        REQUEST => Some(request),
        GET => Some(get),
        _ => None,
    }
}

fn field<'v>(value: &'v Value, name: &str) -> Option<&'v Value> {
    match value {
        Value::Record(record) => record.get(name),
        _ => None,
    }
}

fn text<'v>(value: &'v Value, name: &str) -> Option<&'v str> {
    match field(value, name)? {
        Value::String(s) => Some(s),
        _ => None,
    }
}

fn malformed(capability: &str) -> Failure {
    Failure::new(CallErrorKind::Failed, format!("{capability} received an input that does not match its type"))
}

fn denied(reason: String) -> Failure {
    Failure::new(CallErrorKind::PermissionDenied, reason)
}

fn headers_of(value: &Value, capability: &str) -> Result<Headers, Failure> {
    let Some(Value::Map(map)) = field(value, "headers") else {
        return Err(malformed(capability));
    };
    let mut headers = Headers::new();
    for (name, content) in map.entries() {
        let Value::String(content) = content else {
            return Err(malformed(capability));
        };
        headers.insert(name, content)?;
    }
    Ok(headers)
}

pub(crate) fn is_safe(method: &str) -> bool {
    matches!(method, "GET" | "HEAD" | "OPTIONS" | "TRACE")
}

fn response_value(reply: Reply, safe: bool) -> Result<Value, Failure> {
    let body = reply.body?;
    let body = String::from_utf8(body).map_err(|e| {
        let kind = if safe { CallErrorKind::Invalid } else { CallErrorKind::Failed };
        Failure::new(kind, format!("{}: the response body is not valid UTF-8: {e}", reply.url.shown()))
    })?;
    let headers = MapValue::from_pairs(reply.headers.iter().map(|(n, v)| (Arc::from(n.as_str()), Value::string(v))));
    Ok(Value::Record(RecordValue::new(vec![
        (Arc::from("status"), Value::Int(i64::from(reply.status))),
        (Arc::from("headers"), Value::Map(headers)),
        (Arc::from("body"), Value::string(&body)),
    ])))
}

fn perform(
    capability: &str,
    scope_value: &Value,
    input: &Value,
    method: &str,
    url: &str,
    body: Option<Vec<u8>>,
    limits: &Limits,
) -> Result<Value, Failure> {
    let scope = Scope::of(scope_value).map_err(denied)?;
    let url = scope.permits_url(url, method).map_err(denied)?;
    let headers = headers_of(input, capability)?;
    let reply = send(limits, &scope, Outgoing { method: method.to_string(), url, headers, body })?;
    response_value(reply, is_safe(method))
}

pub fn request_with(limits: &Limits, scope: &Value, input: &Value) -> Result<Value, Failure> {
    let (Some(method), Some(url)) = (text(input, "method"), text(input, "url")) else {
        return Err(malformed(REQUEST));
    };
    let body = match field(input, "body") {
        Some(Value::None) => None,
        Some(Value::Some(inner)) => match &**inner {
            Value::String(s) => Some(s.as_bytes().to_vec()),
            _ => return Err(malformed(REQUEST)),
        },
        _ => return Err(malformed(REQUEST)),
    };
    perform(REQUEST, scope, input, method, url, body, limits)
}

pub fn get_with(limits: &Limits, scope: &Value, input: &Value) -> Result<Value, Failure> {
    let Some(url) = text(input, "url") else {
        return Err(malformed(GET));
    };
    perform(GET, scope, input, "GET", url, None, limits)
}

fn request(scope: &Value, input: &Value) -> Result<Value, Failure> {
    request_with(&Limits::DEFAULT, scope, input)
}

fn get(scope: &Value, input: &Value) -> Result<Value, Failure> {
    get_with(&Limits::DEFAULT, scope, input)
}
