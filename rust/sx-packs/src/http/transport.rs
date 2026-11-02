use std::io::{self, Read};
use std::time::{Duration, Instant};

use h12tiny::client_sync::http::header::{HeaderMap, HeaderName, HeaderValue, LOCATION};
use h12tiny::client_sync::http::{Method, Request, StatusCode, Uri};
use h12tiny::client_sync::{Client, Error, ErrorKind};
use sx_kernel::types::CallErrorKind;

use super::origin::Url;
use super::scope::Scope;
use crate::Failure;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub timeout: Duration,
    pub connect_timeout: Duration,
    pub max_body: usize,
    pub max_redirects: usize,
}

impl Limits {
    pub const DEFAULT: Limits = Limits {
        timeout: Duration::from_secs(30),
        connect_timeout: Duration::from_secs(10),
        max_body: 8 * 1024 * 1024,
        max_redirects: 5,
    };
}

pub const USER_AGENT: &str = concat!("sx-packs/", env!("CARGO_PKG_VERSION"));

const FRAMING_HEADERS: [&str; 7] =
    ["host", "content-length", "transfer-encoding", "connection", "upgrade", "te", "trailer"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Headers(Vec<(HeaderName, HeaderValue)>);

impl Headers {
    pub fn new() -> Headers {
        Headers(Vec::new())
    }

    pub fn insert(&mut self, name: &str, value: &str) -> Result<(), Failure> {
        let invalid = |what: &str| Failure::new(CallErrorKind::Invalid, format!("header {name:?}: {what}"));
        let header = HeaderName::from_bytes(name.as_bytes()).map_err(|_| invalid("not a valid header name"))?;
        if FRAMING_HEADERS.contains(&header.as_str()) {
            return Err(invalid("the client sets this header itself"));
        }
        let content = HeaderValue::from_str(value).map_err(|_| invalid("the value is not a valid header value"))?;
        if self.0.iter().any(|(existing, _)| *existing == header) {
            return Err(invalid("the name appears twice, ignoring case"));
        }
        self.0.push((header, content));
        Ok(())
    }

    pub fn insert_secret(&mut self, name: &str, value: &str) -> Result<(), Failure> {
        if value.is_empty() {
            return Err(Failure::new(CallErrorKind::Invalid, format!("header {name:?}: the value is empty")));
        }
        self.insert(name, value)?;
        if let Some((_, content)) = self.0.last_mut() {
            content.set_sensitive(true);
        }
        Ok(())
    }

    pub fn has(&self, name: &str) -> bool {
        self.0.iter().any(|(existing, _)| existing.as_str() == name)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(name, _)| name.as_str())
    }

    pub fn values(&self) -> impl Iterator<Item = &[u8]> {
        self.0.iter().map(|(_, value)| value.as_bytes())
    }
}

impl Default for Headers {
    fn default() -> Headers {
        Headers::new()
    }
}

#[derive(Clone, Debug)]
pub struct Outgoing {
    pub method: String,
    pub url: Url,
    pub headers: Headers,
    pub body: Option<Vec<u8>>,
}

#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub url: Url,
    pub body: Result<Vec<u8>, Failure>,
}

fn timed_out(what: &str) -> Failure {
    Failure::new(CallErrorKind::Timeout, format!("{what} exceeded the request timeout"))
}

fn transport_failure(url: &Url, error: &Error) -> Failure {
    let kind = match error.kind() {
        ErrorKind::Timeout => CallErrorKind::Timeout,
        ErrorKind::Connect => CallErrorKind::Unavailable,
        _ => CallErrorKind::Failed,
    };
    let source = std::error::Error::source(error).map(|cause| format!(": {cause}")).unwrap_or_default();
    Failure::new(kind, format!("{}: {error}{source}", url.shown()))
}

fn request_of(outgoing: &Outgoing, scope: &Scope) -> Result<Request<Vec<u8>>, Failure> {
    let invalid = |what: String| Failure::new(CallErrorKind::Invalid, what);
    let method = Method::from_bytes(outgoing.method.as_bytes())
        .map_err(|_| invalid(format!("{:?} is not an HTTP method", outgoing.method)))?;
    let uri: Uri = outgoing.url.uri().parse().map_err(|e| invalid(format!("{}: {e}", outgoing.url.shown())))?;
    let mut request = Request::new(outgoing.body.clone().unwrap_or_default());
    *request.method_mut() = method;
    *request.uri_mut() = uri;
    for (name, value) in &outgoing.headers.0 {
        request.headers_mut().append(name.clone(), value.clone());
    }
    let injected = scope.injected_for(&outgoing.url.origin);
    for (name, value) in injected.iter().flat_map(|headers| &headers.0) {
        request.headers_mut().append(name.clone(), value.clone());
    }
    if !outgoing.headers.has("user-agent") && !injected.is_some_and(|headers| headers.has("user-agent")) {
        request.headers_mut().append(HeaderName::from_static("user-agent"), HeaderValue::from_static(USER_AGENT));
    }
    Ok(request)
}

fn redirect(current: &Outgoing, status: StatusCode, headers: &HeaderMap, scope: &Scope) -> Option<Outgoing> {
    let safe = current.method == "GET" || current.method == "HEAD";
    let (method, body) = match status.as_u16() {
        303 => (if current.method == "HEAD" { "HEAD" } else { "GET" }.to_string(), None),
        301 | 302 if safe => (current.method.clone(), None),
        307 | 308 => (current.method.clone(), current.body.clone()),
        _ => return None,
    };
    let mut locations = headers.get_all(LOCATION).iter();
    let location = locations.next()?.to_str().ok()?;
    if locations.next().is_some() {
        return None;
    }
    let url = current.url.resolve(location).ok()?;
    scope.permits(&url.origin, &method).ok()?;
    let headers = if url.origin == current.url.origin {
        let mut kept = current.headers.clone();
        if body.is_none() {
            kept.0.retain(|(name, _)| name.as_str() != "content-type");
        }
        kept
    } else {
        Headers::new()
    };
    Some(Outgoing { method, url, headers, body })
}

fn read_body(
    mut body: h12tiny::client_sync::ResponseBody,
    declared: Option<usize>,
    limits: &Limits,
    deadline: Instant,
    url: &Url,
) -> Result<Vec<u8>, Failure> {
    let oversized = || {
        Failure::new(
            CallErrorKind::Failed,
            format!("{}: the response body is larger than {} bytes", url.shown(), limits.max_body),
        )
    };
    if declared.is_some_and(|length| length > limits.max_body) {
        return Err(oversized());
    }
    let mut collected = Vec::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        let remaining = deadline.checked_duration_since(Instant::now()).filter(|left| !left.is_zero());
        let Some(remaining) = remaining else {
            return Err(timed_out(&url.shown()));
        };
        body.set_read_timeout(Some(remaining)).map_err(|e| Failure::new(CallErrorKind::Failed, format!("{}: {e}", url.shown())))?;
        match body.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                let received = chunk.get(..count).unwrap_or_default();
                if collected.len() + received.len() > limits.max_body {
                    return Err(oversized());
                }
                collected.extend_from_slice(received);
            }
            Err(error) if matches!(error.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) => {
                return Err(timed_out(&url.shown()));
            }
            Err(error) => {
                return Err(Failure::new(CallErrorKind::Failed, format!("{}: reading the body failed: {error}", url.shown())));
            }
        }
    }
    if !body.is_complete() {
        return Err(Failure::new(
            CallErrorKind::Failed,
            format!("{}: the connection closed before the whole body arrived", url.shown()),
        ));
    }
    Ok(collected)
}

fn joined_headers(headers: &HeaderMap, url: &Url) -> Result<Vec<(String, String)>, Failure> {
    let mut joined: Vec<(String, String)> = Vec::new();
    for (name, value) in headers {
        let text = std::str::from_utf8(value.as_bytes()).map_err(|_| {
            Failure::new(CallErrorKind::Invalid, format!("{}: header {} is not valid UTF-8", url.shown(), name.as_str()))
        })?;
        match joined.iter_mut().find(|(existing, _)| existing == name.as_str()) {
            Some((_, value)) => {
                value.push_str(", ");
                value.push_str(text);
            }
            None => joined.push((name.as_str().to_string(), text.to_string())),
        }
    }
    Ok(joined)
}

pub fn send(limits: &Limits, scope: &Scope, request: Outgoing) -> Result<Reply, Failure> {
    let deadline = Instant::now() + limits.timeout;
    let connect_timeout = limits.connect_timeout.min(limits.timeout);
    let client = Client::builder()
        .connect_timeout(connect_timeout)
        .map_err(|e| Failure::new(CallErrorKind::Failed, e.to_string()))?
        .build();
    let mut current = request;
    let mut followed = 0;
    loop {
        let remaining = deadline.checked_duration_since(Instant::now()).filter(|left| !left.is_zero());
        let Some(remaining) = remaining else {
            return Err(timed_out(&current.url.shown()));
        };
        let response = client
            .request_with_timeout(request_of(&current, scope)?, Some(remaining))
            .map_err(|e| transport_failure(&current.url, &e))?;
        let (head, body) = response.into_parts();
        if followed < limits.max_redirects
            && let Some(next) = redirect(&current, head.status, &head.headers, scope)
        {
            current = next;
            followed += 1;
            continue;
        }
        let declared = head
            .headers
            .get(h12tiny::client_sync::http::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|text| text.trim().parse::<usize>().ok());
        let secrets = scope.secrets();
        let headers = joined_headers(&head.headers, &current.url)?
            .into_iter()
            .map(|(name, value)| (name, redact(value.into_bytes(), &secrets)))
            .map(|(name, value)| (name, String::from_utf8_lossy(&value).into_owned()))
            .collect();
        let body = if current.method == "HEAD" {
            Ok(Vec::new())
        } else {
            read_body(body, declared, limits, deadline, &current.url).map(|bytes| redact(bytes, &secrets))
        };
        return Ok(Reply { status: head.status.as_u16(), headers, url: current.url, body });
    }
}

const REDACTED: &[u8] = b"[redacted]";

fn redact(mut bytes: Vec<u8>, secrets: &[Vec<u8>]) -> Vec<u8> {
    for secret in secrets {
        let mut at = 0;
        while let Some(found) = bytes.get(at..).and_then(|rest| rest.windows(secret.len()).position(|window| window == secret.as_slice())) {
            let start = at + found;
            bytes.splice(start..start + secret.len(), REDACTED.iter().copied());
            at = start + REDACTED.len();
        }
    }
    bytes
}

pub fn status_kind(status: u16) -> Option<CallErrorKind> {
    match status {
        200..=299 => None,
        404 => Some(CallErrorKind::NotFound),
        401 | 403 => Some(CallErrorKind::PermissionDenied),
        400 | 422 => Some(CallErrorKind::Invalid),
        409 | 412 => Some(CallErrorKind::Conflict),
        429 => Some(CallErrorKind::RateLimited),
        501 => Some(CallErrorKind::Failed),
        500..=599 => Some(CallErrorKind::Unavailable),
        _ => Some(CallErrorKind::Failed),
    }
}
