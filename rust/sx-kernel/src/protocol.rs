use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::io::{self, BufRead, Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};

use crate::budgets::Budgets;
use crate::checker::compile_source;
use crate::diagnostics::{Diagnostic, render};
use crate::ir::{Node, NodeKind, Program};
use crate::json::{Json, Layout, dumps, parse, repr_str};
use crate::machine::{CoreImplementations, Failure, Machine, Outcome, Request, Response};
use crate::registry::{Core, Effect, Registry, load_declarations};
use crate::syntax::Span;
use crate::types::{Type, show_type};
use crate::values::{Value, decode};

pub const VERSION: i64 = 1;
pub const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;
pub const RUNTIME_INPUT: &str = "computed at run time";

pub trait CoreLibrary {
    fn core(&self, id: &str) -> Option<Core>;
    fn implementations(&self) -> &dyn CoreImplementations;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    MalformedJson,
    LineTooLong,
    InvalidMessage,
    UnsupportedVersion,
    UnexpectedMessage,
    InvalidDeclarations,
    UnknownCapability,
    InvalidInput,
    InvalidBudget,
    EndOfInput,
    Internal,
}

impl ErrorCode {
    pub fn name(self) -> &'static str {
        match self {
            ErrorCode::MalformedJson => "malformed-json",
            ErrorCode::LineTooLong => "line-too-long",
            ErrorCode::InvalidMessage => "invalid-message",
            ErrorCode::UnsupportedVersion => "unsupported-version",
            ErrorCode::UnexpectedMessage => "unexpected-message",
            ErrorCode::InvalidDeclarations => "invalid-declarations",
            ErrorCode::UnknownCapability => "unknown-capability",
            ErrorCode::InvalidInput => "invalid-input",
            ErrorCode::InvalidBudget => "invalid-budget",
            ErrorCode::EndOfInput => "end-of-input",
            ErrorCode::Internal => "internal",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtocolError {
    pub code: ErrorCode,
    pub message: String,
}

impl ProtocolError {
    fn new(code: ErrorCode, message: impl Into<String>) -> ProtocolError {
        ProtocolError { code, message: message.into() }
    }

    fn invalid(message: impl Into<String>) -> ProtocolError {
        ProtocolError::new(ErrorCode::InvalidMessage, message)
    }

    fn json(&self) -> Json {
        Json::object([
            ("type", Json::string("error")),
            ("code", Json::string(self.code.name())),
            ("message", Json::string(self.message.as_str())),
        ])
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.name(), self.message)
    }
}

impl std::error::Error for ProtocolError {}

#[derive(Debug)]
struct Source {
    text: String,
    declarations: Json,
    allow: Option<Vec<String>>,
}

#[derive(Debug)]
struct Start {
    source: Source,
    inputs: Vec<(String, Json)>,
    budgets: Budgets,
}

#[derive(Debug)]
enum Message {
    Start(Start),
    Explain(Source),
    Respond(Vec<(String, Response)>),
    Cancel,
}

fn object<'a>(json: &'a Json, what: &str, known: &[&str]) -> Result<&'a [(String, Json)], ProtocolError> {
    let Json::Object(entries) = json else {
        return Err(ProtocolError::invalid(format!("{what} must be a JSON object")));
    };
    for (key, _) in entries {
        if !known.contains(&key.as_str()) {
            return Err(ProtocolError::invalid(format!("{what} has an unknown field {}", repr_str(key))));
        }
    }
    Ok(entries)
}

fn required<'a>(json: &'a Json, what: &str, key: &str) -> Result<&'a Json, ProtocolError> {
    json.get(key).ok_or_else(|| ProtocolError::invalid(format!("{what} needs a {} field", repr_str(key))))
}

fn string_of<'a>(json: &'a Json, what: &str, key: &str) -> Result<&'a str, ProtocolError> {
    required(json, what, key)?
        .as_str()
        .ok_or_else(|| ProtocolError::invalid(format!("{what}: {} must be a string", repr_str(key))))
}

fn string_list(json: &Json, what: &str, key: &str) -> Result<Vec<String>, ProtocolError> {
    let items = required(json, what, key)?
        .as_array()
        .ok_or_else(|| ProtocolError::invalid(format!("{what}: {} must be a list of strings", repr_str(key))))?;
    items
        .iter()
        .map(|item| {
            item.as_str().map(str::to_string).ok_or_else(|| {
                ProtocolError::invalid(format!("{what}: {} must be a list of strings", repr_str(key)))
            })
        })
        .collect()
}

fn source_of(json: &Json, what: &str, allow_required: bool) -> Result<Source, ProtocolError> {
    let text = string_of(json, what, "program")?.to_string();
    let declarations = json.get("declarations").cloned().unwrap_or_else(|| Json::Object(Vec::new()));
    let allow = match (json.get("allow"), allow_required) {
        (Some(_), _) => Some(string_list(json, what, "allow")?),
        (None, true) => return Err(ProtocolError::invalid(format!("{what} needs an \"allow\" field"))),
        (None, false) => None,
    };
    Ok(Source { text, declarations, allow })
}

fn budgets_of(json: Option<&Json>) -> Result<Budgets, ProtocolError> {
    let mut budgets = Budgets::default();
    let Some(json) = json else {
        return Ok(budgets);
    };
    let Json::Object(entries) = json else {
        return Err(ProtocolError::new(ErrorCode::InvalidBudget, "budgets must be a JSON object"));
    };
    for (name, value) in entries {
        let count = value.as_i64().and_then(|n| u64::try_from(n).ok()).ok_or_else(|| {
            ProtocolError::new(ErrorCode::InvalidBudget, format!("budget {} must be a non-negative integer", repr_str(name)))
        })?;
        budgets.set(name, count).map_err(|e| ProtocolError::new(ErrorCode::InvalidBudget, e.to_string()))?;
    }
    Ok(budgets)
}

fn response_of(json: &Json) -> Result<(String, Response), ProtocolError> {
    let what = "a response";
    let entries = object(json, what, &["id", "output", "error"])?;
    let id = string_of(json, what, "id")?.to_string();
    let response = match (json.get("output"), json.get("error")) {
        (Some(output), None) => Response::Output(output.clone()),
        (None, Some(error)) => {
            object(error, "a response error", &["kind", "message"])?;
            Response::Refusal {
                kind: string_of(error, "a response error", "kind")?.to_string(),
                message: string_of(error, "a response error", "message")?.to_string(),
            }
        }
        _ => {
            let count = entries.iter().filter(|(k, _)| k == "output" || k == "error").count();
            let problem = if count == 0 { "needs" } else { "takes only one of" };
            return Err(ProtocolError::invalid(format!("a response {problem} an \"output\" or an \"error\"")));
        }
    };
    Ok((id, response))
}

fn message_of(json: &Json) -> Result<Message, ProtocolError> {
    let Json::Object(_) = json else {
        return Err(ProtocolError::invalid("a message must be a JSON object"));
    };
    let kind = json
        .get("type")
        .ok_or_else(|| ProtocolError::invalid("a message needs a \"type\" field"))?
        .as_str()
        .ok_or_else(|| ProtocolError::invalid("a message's \"type\" must be a string"))?;
    match kind {
        "start" => {
            object(json, "start", &["type", "version", "program", "declarations", "allow", "inputs", "budgets"])?;
            let source = source_of(json, "start", true)?;
            let inputs = match json.get("inputs") {
                None => Vec::new(),
                Some(Json::Object(entries)) => entries.clone(),
                Some(_) => return Err(ProtocolError::new(ErrorCode::InvalidInput, "inputs must map names to values")),
            };
            Ok(Message::Start(Start { source, inputs, budgets: budgets_of(json.get("budgets"))? }))
        }
        "explain" => {
            object(json, "explain", &["type", "version", "program", "declarations", "allow"])?;
            Ok(Message::Explain(source_of(json, "explain", false)?))
        }
        "respond" => {
            object(json, "respond", &["type", "version", "responses"])?;
            let listed = required(json, "respond", "responses")?
                .as_array()
                .ok_or_else(|| ProtocolError::invalid("respond: \"responses\" must be a list"))?;
            Ok(Message::Respond(listed.iter().map(response_of).collect::<Result<_, _>>()?))
        }
        "cancel" => {
            object(json, "cancel", &["type", "version"])?;
            Ok(Message::Cancel)
        }
        other => Err(ProtocolError::invalid(format!("unknown message type {}", repr_str(other)))),
    }
}

fn span_json(span: Span) -> Json {
    let position = |line: usize, column: usize| {
        let n = |v: usize| Json::Int(i64::try_from(v).unwrap_or(i64::MAX));
        Json::object([("line", n(line)), ("column", n(column))])
    };
    Json::object([
        ("start", position(span.start.line, span.start.column)),
        ("end", position(span.end.line, span.end.column)),
    ])
}

fn optional_text(text: Option<&str>) -> Json {
    text.map_or(Json::Null, Json::string)
}

fn diagnostic_json(diagnostic: &Diagnostic) -> Json {
    Json::object([
        ("code", Json::string(diagnostic.code)),
        ("construct", optional_text(diagnostic.construct)),
        ("span", span_json(diagnostic.span)),
        ("message", Json::string(diagnostic.message.as_str())),
        ("detail", optional_text(diagnostic.detail.as_deref())),
        ("fix", optional_text(diagnostic.fix.as_deref())),
        ("help", optional_text(diagnostic.help.as_deref())),
    ])
}

fn failure_json(failure: &Failure) -> Json {
    Json::object([
        ("kind", Json::string(failure.kind.name())),
        ("message", Json::string(failure.message.as_str())),
        ("span", failure.span.map_or(Json::Null, span_json)),
        ("request", optional_text(failure.request.as_deref())),
    ])
}

fn request_json(request: &Request) -> Json {
    match request {
        Request::Call { id, capability, input } => Json::object([
            ("id", Json::string(id.as_str())),
            ("kind", Json::string("call")),
            ("capability", Json::string(capability.as_str())),
            ("input", input.clone()),
        ]),
        Request::Delay { id, milliseconds } => Json::object([
            ("id", Json::string(id.as_str())),
            ("kind", Json::string("delay")),
            ("milliseconds", Json::Int(*milliseconds)),
        ]),
    }
}

fn literal_json(node: &Node) -> Option<Json> {
    match &node.kind {
        NodeKind::Literal { value, .. } => match value {
            Value::Bool(b) => Some(Json::Bool(*b)),
            Value::Int(n) => Some(Json::Int(*n)),
            Value::Float(f) => Some(Json::Float(f.0)),
            Value::String(s) => Some(Json::string(&**s)),
            _ => None,
        },
        NodeKind::MakeRecord { fields } => {
            let parts: Option<Vec<(String, Json)>> =
                fields.iter().map(|(name, part)| literal_json(part).map(|j| (name.clone(), j))).collect();
            parts.map(Json::Object)
        }
        NodeKind::MakeList { items, .. } => items.iter().map(literal_json).collect::<Option<Vec<_>>>().map(Json::Array),
        _ => None,
    }
}

fn lambda_body<'a>(lambda: &'a crate::ir::Lambda, out: &mut Vec<&'a Node>) {
    out.push(&lambda.body);
}

fn children<'a>(node: &'a Node) -> Vec<&'a Node> {
    let mut out: Vec<&'a Node> = Vec::new();
    match &node.kind {
        NodeKind::Literal { .. } | NodeKind::Variable { .. } | NodeKind::Sleep { .. } | NodeKind::MakeEmptyBrace { .. } => {}
        NodeKind::Let(binding) => out.extend([&*binding.value, &*binding.body]),
        NodeKind::LocalFunction { body, .. } => out.push(body),
        NodeKind::If(branch) => out.extend([&*branch.condition, &*branch.then, &*branch.otherwise]),
        NodeKind::Match(arms) => {
            out.push(&arms.scrutinee);
            out.extend(arms.clauses.iter().map(|(_, body)| body));
        }
        NodeKind::Lambda(lambda) => lambda_body(lambda, &mut out),
        NodeKind::ApplyLocal { function, args, .. } => {
            lambda_body(function, &mut out);
            out.extend(args);
        }
        NodeKind::Prelude { args, .. } | NodeKind::CoreCall { args, .. } | NodeKind::Recur { args } => out.extend(args),
        NodeKind::Call { argument, .. } => out.push(argument),
        NodeKind::Get { record, .. } => out.push(record),
        NodeKind::MakeRecord { fields } => out.extend(fields.iter().map(|(_, part)| part)),
        NodeKind::MakeList { items, .. } | NodeKind::MakeTuple { items } | NodeKind::MakeBracket { items, .. } => {
            out.extend(items);
        }
        NodeKind::Wrap { value, .. } | NodeKind::Coerce { value, .. } => out.push(value),
        NodeKind::MakeVariant { payload, .. } => out.extend(payload.as_deref()),
        NodeKind::Attempt { body } => out.push(body),
        NodeKind::Retry(retry) => out.push(&retry.body),
        NodeKind::WaitUntil(wait) => {
            lambda_body(&wait.predicate, &mut out);
            out.push(&wait.body);
        }
        NodeKind::Loop(repeat) => {
            out.extend(&repeat.initial);
            out.push(&repeat.body);
        }
        NodeKind::Par { branches } => out.extend(branches),
        NodeKind::ParMap { function, items, .. } => {
            lambda_body(function, &mut out);
            out.push(items);
        }
    }
    out
}

fn literal_calls(root: &Node) -> Vec<(&str, Option<Json>)> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if let NodeKind::Call { capability, argument } = &node.kind {
            found.push((capability.as_str(), literal_json(argument)));
        }
        stack.extend(children(node).into_iter().rev());
    }
    found
}

pub fn explain_report(program: &Program, allowed: &BTreeSet<String>) -> Json {
    let calls = literal_calls(&program.body);
    let mut tools = Vec::new();
    for id in &program.capabilities {
        let effect = match program.signatures.capabilities.get(id) {
            Some(signature) if signature.effect == Effect::Write => {
                format!("write, {}", if signature.idempotent { "idempotent" } else { "not idempotent" })
            }
            Some(signature) => signature.effect.name().to_string(),
            None => String::new(),
        };
        let inputs: Vec<Json> = calls
            .iter()
            .filter(|(call, _)| call == id)
            .map(|(_, input)| input.clone().unwrap_or_else(|| Json::string(RUNTIME_INPUT)))
            .collect();
        tools.push(Json::object([
            ("id", Json::string(id.as_str())),
            ("effect", Json::string(effect)),
            ("allowed", Json::Bool(allowed.contains(id))),
            ("inputs", Json::Array(inputs)),
        ]));
    }
    Json::object([
        ("type", Json::string("explained")),
        ("tools", Json::Array(tools)),
        ("inputs", Json::Array(program.inputs.keys().map(|name| Json::string(name.as_str())).collect())),
        ("result", Json::string(show_type(&program.result))),
    ])
}

struct Declared {
    registry: Registry,
    inputs: BTreeMap<String, Type>,
}

fn invalid_declarations(message: impl Into<String>) -> ProtocolError {
    ProtocolError::new(ErrorCode::InvalidDeclarations, message)
}

fn declare(declarations: &Json, library: &dyn CoreLibrary) -> Result<Declared, ProtocolError> {
    let mut registry = Registry::new();
    match declarations.get("cores") {
        None => {}
        Some(Json::Array(ids)) => {
            for id in ids {
                let id = id.as_str().ok_or_else(|| invalid_declarations("declarations: cores must be a list of ids"))?;
                let core = library.core(id).ok_or_else(|| invalid_declarations(format!("no core is named {id:?}")))?;
                registry.add_core(core).map_err(|e| invalid_declarations(e.0))?;
            }
        }
        Some(_) => return Err(invalid_declarations("declarations: cores must be a list of ids")),
    }
    load_declarations(&mut registry, declarations).map_err(|e| invalid_declarations(e.to_string()))?;
    let mut inputs = BTreeMap::new();
    match declarations.get("inputs") {
        None => {}
        Some(Json::Object(entries)) => {
            for (name, declared) in entries {
                let text = declared
                    .as_str()
                    .ok_or_else(|| invalid_declarations("declarations: inputs must map names to types"))?;
                let parsed = registry.parse(text).map_err(|e| invalid_declarations(format!("declarations: {}", e.0)))?;
                inputs.insert(name.clone(), parsed);
            }
        }
        Some(_) => return Err(invalid_declarations("declarations: inputs must map names to types")),
    }
    Ok(Declared { registry, inputs })
}

enum Compiled {
    Rejected(Json),
    Ready { program: Box<Program>, declared: BTreeMap<String, Type> },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Run,
    Explain,
}

fn compile(
    source: &Source,
    purpose: Purpose,
    budgets: Budgets,
    library: &dyn CoreLibrary,
) -> Result<Compiled, ProtocolError> {
    let Declared { registry, inputs } = declare(&source.declarations, library)?;
    let allow: BTreeSet<String> = source.allow.iter().flatten().cloned().collect();
    if let Some(unknown) = allow.iter().find(|id| registry.capability(id).is_none()) {
        let message = format!("allow names a capability the declarations do not define: {unknown}");
        return Err(ProtocolError::new(ErrorCode::UnknownCapability, message));
    }
    let checking: BTreeSet<String> = match purpose {
        Purpose::Run => allow.clone(),
        Purpose::Explain => registry.capabilities().map(|c| c.id.clone()).collect(),
    };
    let checked = compile_source(&source.text, &registry, &checking, &inputs, budgets);
    match checked.program {
        Some(program) => Ok(Compiled::Ready { program: Box::new(program), declared: inputs }),
        None => Ok(Compiled::Rejected(Json::object([
            ("type", Json::string("rejected")),
            ("diagnostics", Json::Array(checked.diagnostics.iter().map(diagnostic_json).collect())),
            ("rendered", Json::string(render(&source.text, &checked.diagnostics))),
        ]))),
    }
}

fn values_of(declared: &BTreeMap<String, Type>, supplied: &[(String, Json)]) -> Result<Vec<(String, Value)>, ProtocolError> {
    let invalid = |message: String| ProtocolError::new(ErrorCode::InvalidInput, message);
    if let Some((name, _)) = supplied.iter().find(|(name, _)| !declared.contains_key(name)) {
        return Err(invalid(format!("input {name} is not declared in the declarations' inputs")));
    }
    let mut values = Vec::new();
    for (name, t) in declared {
        let given = supplied
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, json)| json)
            .ok_or_else(|| invalid(format!("the program needs a value for input {name}")))?;
        let value = decode(given, t, &format!("${name}")).map_err(|e| invalid(format!("input {name}: {e}")))?;
        values.push((name.clone(), value));
    }
    Ok(values)
}

fn guarded<T>(work: impl FnOnce() -> T) -> Result<T, ProtocolError> {
    catch_unwind(AssertUnwindSafe(work))
        .map_err(|_| ProtocolError::new(ErrorCode::Internal, "the kernel hit an internal error; the execution was abandoned"))
}

enum Line {
    End,
    Blank,
    Text(String),
    TooLong,
    NotUtf8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    Idle,
    Abandoned,
}

enum Incoming {
    End,
    Message(Message),
    Invalid(ProtocolError),
}

enum Collected {
    Resume(Vec<(String, Response)>),
    Abandoned,
}

struct Session<'l, R, W> {
    input: R,
    output: W,
    library: &'l dyn CoreLibrary,
    negotiated: bool,
}

pub fn serve<R: BufRead, W: Write>(input: R, output: W, library: &dyn CoreLibrary) -> io::Result<Ending> {
    Session { input, output, library, negotiated: false }.run()
}

impl<R: BufRead, W: Write> Session<'_, R, W> {
    fn send(&mut self, message: &Json) -> io::Result<()> {
        let text = dumps(message, Layout::Spaced).map_err(|e| io::Error::other(e.0))?;
        self.output.write_all(text.as_bytes())?;
        self.output.write_all(b"\n")?;
        self.output.flush()
    }

    fn reject(&mut self, error: &ProtocolError) -> io::Result<()> {
        self.send(&error.json())
    }

    fn read_line(&mut self) -> io::Result<Line> {
        let mut bytes = Vec::new();
        let limit = MAX_LINE_BYTES as u64 + 1;
        (&mut self.input).take(limit).read_until(b'\n', &mut bytes)?;
        if bytes.is_empty() {
            return Ok(Line::End);
        }
        if bytes.len() > MAX_LINE_BYTES && bytes.last() != Some(&b'\n') {
            loop {
                let (consumed, found) = {
                    let buffered = self.input.fill_buf()?;
                    match buffered.iter().position(|b| *b == b'\n') {
                        Some(at) => (at + 1, true),
                        None => (buffered.len(), buffered.is_empty()),
                    }
                };
                self.input.consume(consumed);
                if found {
                    break;
                }
            }
            return Ok(Line::TooLong);
        }
        let Ok(text) = String::from_utf8(bytes) else {
            return Ok(Line::NotUtf8);
        };
        Ok(if text.trim().is_empty() { Line::Blank } else { Line::Text(text) })
    }

    fn negotiate(&mut self, json: &Json) -> Result<(), ProtocolError> {
        if !matches!(json, Json::Object(_)) {
            return Err(ProtocolError::invalid("a message must be a JSON object"));
        }
        match json.get("version") {
            None if self.negotiated => Ok(()),
            None => Err(ProtocolError::invalid(format!("the first message must carry \"version\": {VERSION}"))),
            Some(Json::Int(version)) if *version == VERSION => {
                self.negotiated = true;
                Ok(())
            }
            Some(Json::Int(version)) => Err(ProtocolError::new(
                ErrorCode::UnsupportedVersion,
                format!("protocol version {version} is not supported; this kernel speaks version {VERSION}"),
            )),
            Some(_) => Err(ProtocolError::invalid("\"version\" must be an integer")),
        }
    }

    fn next(&mut self) -> io::Result<Incoming> {
        loop {
            let text = match self.read_line()? {
                Line::End => return Ok(Incoming::End),
                Line::Blank => continue,
                Line::TooLong => {
                    let message = format!("a message line is longer than {MAX_LINE_BYTES} bytes");
                    return Ok(Incoming::Invalid(ProtocolError::new(ErrorCode::LineTooLong, message)));
                }
                Line::NotUtf8 => {
                    return Ok(Incoming::Invalid(ProtocolError::new(ErrorCode::MalformedJson, "the line is not valid UTF-8")));
                }
                Line::Text(text) => text,
            };
            let parsed = parse(&text)
                .map_err(|e| ProtocolError::new(ErrorCode::MalformedJson, e.0))
                .and_then(|json| self.negotiate(&json).and_then(|()| message_of(&json)));
            return Ok(match parsed {
                Ok(message) => Incoming::Message(message),
                Err(error) => Incoming::Invalid(error),
            });
        }
    }

    fn run(&mut self) -> io::Result<Ending> {
        loop {
            match self.next()? {
                Incoming::End => return Ok(Ending::Idle),
                Incoming::Invalid(error) => self.reject(&error)?,
                Incoming::Message(Message::Start(start)) => {
                    if self.execute(start)? == Ending::Abandoned {
                        return Ok(Ending::Abandoned);
                    }
                }
                Incoming::Message(Message::Explain(source)) => self.explain(&source)?,
                Incoming::Message(Message::Respond(_) | Message::Cancel) => {
                    let error = ProtocolError::new(ErrorCode::UnexpectedMessage, "no execution is waiting for responses");
                    self.reject(&error)?;
                }
            }
        }
    }

    fn explain(&mut self, source: &Source) -> io::Result<()> {
        let library = self.library;
        let reply = guarded(|| match compile(source, Purpose::Explain, Budgets::default(), library)? {
            Compiled::Rejected(rejection) => Ok(rejection),
            Compiled::Ready { program, .. } => {
                let allowed: BTreeSet<String> = source.allow.iter().flatten().cloned().collect();
                Ok(explain_report(&program, &allowed))
            }
        });
        match reply.and_then(|inner| inner) {
            Ok(reply) => self.send(&reply),
            Err(error) => self.reject(&error),
        }
    }

    fn execute(&mut self, start: Start) -> io::Result<Ending> {
        let library = self.library;
        let compiled = guarded(|| {
            let compiled = compile(&start.source, Purpose::Run, start.budgets, library)?;
            match compiled {
                Compiled::Rejected(rejection) => Ok(Err(rejection)),
                Compiled::Ready { program, declared } => {
                    let values = values_of(&declared, &start.inputs)?;
                    Ok(Ok((program, values)))
                }
            }
        })
        .and_then(|inner| inner);
        let (program, values) = match compiled {
            Err(error) => {
                self.reject(&error)?;
                return Ok(Ending::Idle);
            }
            Ok(Err(rejection)) => {
                self.send(&rejection)?;
                return Ok(Ending::Idle);
            }
            Ok(Ok(ready)) => ready,
        };
        let mut machine = Machine::new(&program, &values, start.budgets, library.implementations());
        let mut outcome = guarded(|| machine.start());
        loop {
            let requests = match outcome {
                Err(error) => {
                    self.reject(&error)?;
                    return Ok(Ending::Idle);
                }
                Ok(Outcome::Completed { json, .. }) => {
                    let reply = Json::object([
                        ("type", Json::string("completed")),
                        ("value", json),
                        ("trace", Json::Array(machine.trace().to_vec())),
                    ]);
                    self.send(&reply)?;
                    return Ok(Ending::Idle);
                }
                Ok(Outcome::Failed(failure)) => {
                    let reply = Json::object([
                        ("type", Json::string("failed")),
                        ("failure", failure_json(&failure)),
                        ("trace", Json::Array(machine.trace().to_vec())),
                    ]);
                    self.send(&reply)?;
                    return Ok(Ending::Idle);
                }
                Ok(Outcome::Suspended(requests)) => requests,
            };
            let listed = Json::object([
                ("type", Json::string("requests")),
                ("requests", Json::Array(requests.iter().map(request_json).collect())),
            ]);
            self.send(&listed)?;
            match self.collect(&requests)? {
                Collected::Abandoned => return Ok(Ending::Abandoned),
                Collected::Resume(responses) => outcome = guarded(|| machine.resume(responses)),
            }
        }
    }

    fn collect(&mut self, requests: &[Request]) -> io::Result<Collected> {
        let outstanding: HashSet<&str> = requests.iter().map(Request::id).collect();
        let mut answered: Vec<(String, Response)> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        loop {
            match self.next()? {
                Incoming::End => {
                    let message = "the input ended while an execution waited for responses";
                    self.reject(&ProtocolError::new(ErrorCode::EndOfInput, message))?;
                    return Ok(Collected::Abandoned);
                }
                Incoming::Invalid(error) => self.reject(&error)?,
                Incoming::Message(Message::Respond(responses)) => {
                    let mut faulty = false;
                    for (id, response) in responses {
                        faulty |= !outstanding.contains(id.as_str()) || !seen.insert(id.clone());
                        answered.push((id, response));
                    }
                    if faulty || answered.len() == outstanding.len() {
                        return Ok(Collected::Resume(answered));
                    }
                }
                Incoming::Message(Message::Cancel) => return Ok(Collected::Resume(answered)),
                Incoming::Message(Message::Explain(source)) => self.explain(&source)?,
                Incoming::Message(Message::Start(_)) => {
                    let message = "an execution is waiting for responses; answer or cancel it before starting another";
                    self.reject(&ProtocolError::new(ErrorCode::UnexpectedMessage, message))?;
                }
            }
        }
    }
}
