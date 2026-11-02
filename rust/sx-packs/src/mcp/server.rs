use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;

use sx_kernel::authority::{Grant, Grants, Screen, Verdict};
use sx_kernel::json::{Json, Layout, dumps, parse};
use sx_kernel::registry::{Capability, Registry};
use sx_kernel::types::{CallErrorKind, Type};
use sx_kernel::values::{Value, decode, encode};

use super::client::{ACCEPTED_VERSIONS, LINE_LIMIT, PROTOCOL_VERSION, read_line};
use super::describe::describe;
use super::export::{Shape, WRAPPED_KEY, export};
use super::tools::filled;
use crate::declared::Declared;
use crate::{Failure, implemented, perform};

pub const CONCURRENT_CALLS: usize = 64;

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Clone, Debug)]
struct Served {
    capability: Capability,
    input: Type,
    shape: Shape,
    tool: Json,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotServed {
    pub id: String,
    pub reason: String,
}

pub struct Server {
    served: BTreeMap<String, Served>,
    not_served: Vec<NotServed>,
    grants: Grants,
    declared: Declared,
}

#[derive(Clone, Debug)]
pub enum Answer {
    Result(Json),
    Rejected { code: i64, message: String },
}

#[derive(Clone, Debug)]
enum Step {
    Reply(Json),
    Silent,
    Call { id: Json, params: Json },
    Cancel(Json),
}

fn text_block(text: &str) -> Json {
    Json::object([("type", Json::string("text")), ("text", Json::string(text))])
}

fn serialized(json: &Json) -> String {
    dumps(json, Layout::Spaced).unwrap_or_else(|error| error.0)
}

pub fn error_result(kind: CallErrorKind, message: &str) -> Json {
    Json::object([
        ("content", Json::Array(vec![text_block(&format!("{}: {message}", kind.name()))])),
        (
            "structuredContent",
            Json::object([("kind", Json::string(kind.name())), ("message", Json::string(message))]),
        ),
        ("isError", Json::Bool(true)),
    ])
}

fn success_result(shape: Shape, output: Json) -> Json {
    match (shape, output) {
        (Shape::Text, Json::String(text)) => {
            Json::object([("content", Json::Array(vec![text_block(&text)])), ("isError", Json::Bool(false))])
        }
        (Shape::Text, other) => error_result(CallErrorKind::Failed, &format!("a text result was {}", serialized(&other))),
        (Shape::Structured, output) => structured(output),
        (Shape::Wrapped, output) => structured(Json::object([(WRAPPED_KEY, output)])),
    }
}

fn structured(content: Json) -> Json {
    Json::object([
        ("content", Json::Array(vec![text_block(&serialized(&content))])),
        ("structuredContent", content),
        ("isError", Json::Bool(false)),
    ])
}

fn response(id: &Json, outcome: (&str, Json)) -> Json {
    Json::object([("jsonrpc", Json::string("2.0")), ("id", id.clone()), (outcome.0, outcome.1)])
}

fn rpc_error(id: &Json, code: i64, message: &str) -> Json {
    response(
        id,
        ("error", Json::object([("code", Json::Int(code)), ("message", Json::string(message))])),
    )
}

impl Server {
    pub fn new(registry: &Registry, grants: Grants, declared: Declared) -> Result<Server, String> {
        let ids: Vec<String> = grants.ids().map(str::to_string).collect();
        let mut served = BTreeMap::new();
        let mut not_served = Vec::new();
        for id in ids {
            let capability = registry.capability(&id).ok_or_else(|| format!("no capability is named {id}"))?;
            if !implemented(&declared, &id) {
                not_served.push(NotServed { id, reason: "the binary has no native implementation of it".to_string() });
                continue;
            }
            let exported = export(capability).map_err(|reason| format!("cannot serve {id}: {reason}"))?;
            served.insert(
                id,
                Served {
                    capability: capability.clone(),
                    input: capability.input.clone(),
                    shape: exported.shape,
                    tool: exported.tool,
                },
            );
        }
        Ok(Server { served, not_served, grants, declared })
    }

    pub fn not_served(&self) -> &[NotServed] {
        &self.not_served
    }

    pub fn tools(&self) -> Vec<Json> {
        self.served.values().map(|served| served.tool.clone()).collect()
    }

    pub fn describe(&self) -> Json {
        describe(self.served.values().map(|served| &served.capability))
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.served.keys().map(String::as_str)
    }

    fn invoke(&self, id: &str, served: &Served, arguments: &Json) -> Result<Json, Failure> {
        let input = filled(arguments, &served.input);
        let value = decode(&input, &served.input, "$").map_err(|error| {
            Failure::new(CallErrorKind::Invalid, format!("{id}: the arguments do not match the input type: {error}"))
        })?;
        match self.grants.screen(id, &value) {
            Screen::NotGranted => {
                return Err(Failure::new(CallErrorKind::PermissionDenied, format!("{id} is not granted")));
            }
            Screen::Verdict(Verdict::Denied(reason)) => {
                return Err(Failure::new(CallErrorKind::PermissionDenied, reason));
            }
            Screen::Unscoped | Screen::Unchecked | Screen::Verdict(Verdict::Allowed) => {}
        }
        let scope = match self.grants.get(id) {
            Some(Grant::Scoped { scope, .. }) => scope.clone(),
            _ => Value::Unit,
        };
        match perform(&self.declared, id, &scope, &value) {
            None => Err(Failure::new(CallErrorKind::Failed, format!("{id} has no native implementation"))),
            Some(performed) => performed.map(|output| encode(&output)),
        }
    }

    pub fn call(&self, params: &Json) -> Answer {
        let rejected = |message: String| Answer::Rejected { code: INVALID_PARAMS, message };
        let Some(name) = params.get("name").and_then(Json::as_str) else {
            return rejected("tools/call needs a string name".to_string());
        };
        let Some(served) = self.served.get(name) else {
            return rejected(format!("unknown tool {name}"));
        };
        let none = Json::Object(Vec::new());
        let arguments = match params.get("arguments") {
            None | Some(Json::Null) => &none,
            Some(object @ Json::Object(_)) => object,
            Some(_) => return rejected("arguments must be an object".to_string()),
        };
        let performed = catch_unwind(AssertUnwindSafe(|| self.invoke(name, served, arguments)));
        Answer::Result(match performed {
            Ok(Ok(output)) => success_result(served.shape, output),
            Ok(Err(failure)) => error_result(failure.kind, &failure.message),
            Err(_) => error_result(CallErrorKind::Failed, &format!("performing {name} panicked")),
        })
    }
}

pub trait ToolHost: Sync {
    fn tools(&self) -> Vec<Json>;
    fn call(&self, params: &Json) -> Answer;
}

impl ToolHost for Server {
    fn tools(&self) -> Vec<Json> {
        Server::tools(self)
    }

    fn call(&self, params: &Json) -> Answer {
        Server::call(self, params)
    }
}

fn initialize(params: &Json) -> Json {
    let asked = params.get("protocolVersion").and_then(Json::as_str).unwrap_or("");
    let version = if ACCEPTED_VERSIONS.contains(&asked) { asked } else { PROTOCOL_VERSION };
    Json::object([
        ("protocolVersion", Json::string(version)),
        ("capabilities", Json::object([("tools", Json::object([("listChanged", Json::Bool(false))]))])),
        (
            "serverInfo",
            Json::object([("name", Json::string("sx")), ("version", Json::string(env!("CARGO_PKG_VERSION")))]),
        ),
    ])
}

fn route(host: &impl ToolHost, message: &Json) -> Step {
    if !matches!(message, Json::Object(_)) {
        return Step::Reply(rpc_error(&Json::Null, INVALID_REQUEST, "a message must be a JSON object"));
    }
    let id = message.get("id").filter(|id| !id.is_null());
    let Some(method) = message.get("method").and_then(Json::as_str) else {
        return match (id, message.get("result").or_else(|| message.get("error"))) {
            (_, Some(_)) | (None, None) => Step::Silent,
            (Some(id), None) => Step::Reply(rpc_error(id, INVALID_REQUEST, "a request needs a method")),
        };
    };
    let params = message.get("params").cloned().unwrap_or(Json::Null);
    let reply = |outcome: Json| id.map_or(Step::Silent, |id| Step::Reply(response(id, ("result", outcome))));
    match method {
        "initialize" => reply(initialize(&params)),
        "ping" => reply(Json::Object(Vec::new())),
        "tools/list" => match params.get("cursor") {
            None | Some(Json::Null) => reply(Json::object([("tools", Json::Array(host.tools()))])),
            Some(_) => id.map_or(Step::Silent, |id| {
                Step::Reply(rpc_error(id, INVALID_PARAMS, "this server issues no cursors"))
            }),
        },
        "tools/call" => id.map_or(Step::Silent, |id| Step::Call { id: id.clone(), params }),
        "notifications/cancelled" => {
            params.get("requestId").map_or(Step::Silent, |request| Step::Cancel(request.clone()))
        }
        other => id.map_or(Step::Silent, |id| {
            Step::Reply(rpc_error(id, METHOD_NOT_FOUND, &format!("sx does not implement {other}")))
        }),
    }
}

struct Output<W> {
    sink: Mutex<W>,
    broken: AtomicBool,
}

impl<W: Write> Output<W> {
    fn send(&self, message: &Json) {
        let mut line = serialized(message);
        line.push('\n');
        let mut sink = locked(&self.sink);
        if sink.write_all(line.as_bytes()).and_then(|()| sink.flush()).is_err() {
            self.broken.store(true, Ordering::Relaxed);
        }
    }

    fn is_broken(&self) -> bool {
        self.broken.load(Ordering::Relaxed)
    }
}

struct Slots {
    used: Mutex<usize>,
    freed: Condvar,
}

impl Slots {
    fn acquire(&self) {
        let mut used = locked(&self.used);
        while *used >= CONCURRENT_CALLS {
            used = self.freed.wait(used).unwrap_or_else(PoisonError::into_inner);
        }
        *used += 1;
    }

    fn release(&self) {
        let mut used = locked(&self.used);
        *used = used.saturating_sub(1);
        self.freed.notify_one();
    }
}

fn key(id: &Json) -> String {
    serialized(id)
}

pub fn serve<R: BufRead, W: Write + Send>(server: &impl ToolHost, mut input: R, output: W) -> Result<(), String> {
    let output = Output { sink: Mutex::new(output), broken: AtomicBool::new(false) };
    let slots = Slots { used: Mutex::new(0), freed: Condvar::new() };
    let in_flight: Mutex<HashMap<String, bool>> = Mutex::new(HashMap::new());
    thread::scope(|scope| {
        while !output.is_broken() {
            let line = match read_line(&mut input) {
                Ok(Some(line)) => line,
                Ok(None) => return Ok(()),
                Err(error) => return Err(format!("cannot read a request (the limit is {LINE_LIMIT} bytes a line): {error}")),
            };
            let Ok(text) = String::from_utf8(line) else {
                output.send(&rpc_error(&Json::Null, PARSE_ERROR, "a line is not UTF-8"));
                continue;
            };
            if text.trim().is_empty() {
                continue;
            }
            let message = match parse(&text) {
                Ok(message) => message,
                Err(error) => {
                    output.send(&rpc_error(&Json::Null, PARSE_ERROR, &format!("a line is not JSON: {error}")));
                    continue;
                }
            };
            match route(server, &message) {
                Step::Silent => {}
                Step::Reply(reply) => output.send(&reply),
                Step::Cancel(request) => {
                    if let Some(cancelled) = locked(&in_flight).get_mut(&key(&request)) {
                        *cancelled = true;
                    }
                }
                Step::Call { id, params } => {
                    slots.acquire();
                    locked(&in_flight).insert(key(&id), false);
                    let (output, slots, in_flight) = (&output, &slots, &in_flight);
                    let started = thread::Builder::new().name("mcp-call".to_string()).spawn_scoped(scope, {
                        let id = id.clone();
                        move || {
                            let answer = server.call(&params);
                            let cancelled = locked(in_flight).remove(&key(&id)).unwrap_or(false);
                            if !cancelled {
                                output.send(&match answer {
                                    Answer::Result(result) => response(&id, ("result", result)),
                                    Answer::Rejected { code, message } => rpc_error(&id, code, &message),
                                });
                            }
                            slots.release();
                        }
                    });
                    if let Err(error) = started {
                        locked(in_flight).remove(&key(&id));
                        slots.release();
                        let result = error_result(CallErrorKind::Failed, &format!("cannot start a worker: {error}"));
                        output.send(&response(&id, ("result", result)));
                    }
                }
            }
        }
        Ok(())
    })
}
