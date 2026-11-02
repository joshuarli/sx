use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use sx_cores::kernel::{Cores, core as builtin_core};
use sx_kernel::budgets::Budgets;
use sx_kernel::checker::{closest, compile_source};
use sx_kernel::diagnostics::{Diagnostic, render};
use sx_kernel::export::export_program;
use sx_kernel::help::{effect_label, guide, lookup, names, render_cores};
use sx_kernel::host::{FixtureHost, run};
use sx_kernel::ir::Program;
use sx_kernel::json::{Json, Layout, dumps, python_repr, repr_str};
use sx_kernel::machine::Outcome;
use sx_kernel::registry::{Registry, load_declarations};
use sx_kernel::syntax::Span;
use sx_kernel::types::{Type, show_type};
use sx_kernel::values::{Value, decode, show_value};

use crate::argparse::{CoresArgs, Format, HelpArgs, Invocation, ProgramArgs, ProgramKind};
use crate::explain::{RUNTIME, literal_calls};
use crate::pyfile::{path_text, read_text};
use crate::{EXIT_FAILED, EXIT_OK, EXIT_REJECTED, EXIT_USAGE, Exit, pyjson};

enum Stop {
    Usage(String),
    Internal(String),
}

impl Stop {
    fn usage(message: impl Into<String>) -> Stop {
        Stop::Usage(message.into())
    }
}

#[derive(Default)]
struct Output {
    stdout: String,
    stderr: String,
}

impl Output {
    fn line(&mut self, text: &str) {
        self.stdout.push_str(text);
        self.stdout.push('\n');
    }

    fn emit(&mut self, format: Format, payload: Json, text: &str) -> Result<(), Stop> {
        match format {
            Format::Json => {
                let shown = dumps(&payload, Layout::Indented).map_err(|e| Stop::Internal(e.to_string()))?;
                self.line(&shown);
            }
            Format::Text => self.line(text),
        }
        Ok(())
    }
}

pub fn execute(invocation: Invocation, registry: Registry) -> Exit {
    let mut output = Output::default();
    let code = match dispatch(invocation, registry, &mut output) {
        Ok(code) => code,
        Err(Stop::Usage(message)) => {
            output.stderr.push_str(&format!("sx: {message}\n"));
            EXIT_USAGE
        }
        Err(Stop::Internal(message)) => {
            output.stderr.push_str(&format!("sx: internal error: {message}\n"));
            EXIT_FAILED
        }
    };
    Exit { code, stdout: output.stdout, stderr: output.stderr }
}

fn dispatch(invocation: Invocation, registry: Registry, output: &mut Output) -> Result<i32, Stop> {
    match invocation {
        Invocation::Help(args) => command_help(&args, registry, output),
        Invocation::Cores(args) => command_cores(&args, &registry, output),
        Invocation::Program(args) => command_program(&args, registry, output),
    }
}

fn read_json(path: &str, what: &str) -> Result<Json, Stop> {
    let shown = path_text(path);
    let text = read_text(Path::new(path)).map_err(|e| Stop::usage(format!("cannot read {what} {shown}: {e}")))?;
    pyjson::parse(&text).map_err(|e| Stop::usage(format!("cannot read {what} {shown}: {e}")))
}

fn core_ids(declarations: &Json, shown: &str) -> Result<Vec<String>, Stop> {
    let listed = match declarations.get("cores") {
        None | Some(Json::Null) => return Ok(Vec::new()),
        Some(Json::Array(items)) => items,
        Some(other) => {
            return Err(Stop::usage(format!(
                "invalid declarations {shown}: cores must be a list of core names, not {}",
                python_repr(other)
            )));
        }
    };
    listed
        .iter()
        .map(|item| {
            item.as_str().map(str::to_string).ok_or_else(|| {
                Stop::usage(format!("invalid declarations {shown}: core names must be strings, not {}", python_repr(item)))
            })
        })
        .collect()
}

fn load_registry(path: Option<&str>, mut registry: Registry) -> Result<(Registry, Json), Stop> {
    let Some(path) = path else {
        return Ok((registry, Json::Object(Vec::new())));
    };
    let shown = path_text(path);
    let declarations = read_json(path, "declarations")?;
    if !matches!(declarations, Json::Object(_)) {
        return Err(Stop::usage(format!("declarations {shown} must be a JSON object")));
    }
    for id in core_ids(&declarations, &shown)? {
        if registry.core(&id).is_some() {
            continue;
        }
        let core = builtin_core(&id).ok_or_else(|| {
            Stop::usage(format!("invalid declarations {shown}: no built-in core is named {}", repr_str(&id)))
        })?;
        registry.add_core(core).map_err(|e| Stop::usage(format!("invalid declarations {shown}: {e}")))?;
    }
    load_declarations(&mut registry, &declarations)
        .map_err(|e| Stop::usage(format!("invalid declarations {shown}: {e}")))?;
    Ok((registry, declarations))
}

fn command_cores(args: &CoresArgs, registry: &Registry, output: &mut Output) -> Result<i32, Stop> {
    let payload = Json::Object(
        registry
            .cores()
            .map(|core| {
                let assurance = core
                    .assurance()
                    .into_iter()
                    .map(|(level, references)| {
                        (level.name().to_string(), Json::Array(references.into_iter().map(Json::String).collect()))
                    })
                    .collect();
                (
                    core.id.clone(),
                    Json::object([
                        ("version", Json::string(core.version.as_str())),
                        ("assurance", Json::Object(assurance)),
                    ]),
                )
            })
            .collect(),
    );
    output.emit(args.format, payload, render_cores(registry).trim_end_matches('\n'))?;
    Ok(EXIT_OK)
}

fn command_help(args: &HelpArgs, base: Registry, output: &mut Output) -> Result<i32, Stop> {
    let (registry, _) = load_registry(args.declarations.as_deref(), base)?;
    let Some(name) = &args.name else {
        output.stdout.push_str(&guide(args.level));
        return Ok(EXIT_OK);
    };
    if let Some(found) = lookup(name, &registry) {
        output.stdout.push_str(&found);
        return Ok(EXIT_OK);
    }
    let hint = closest(name, &names(&registry)).map(|suggestion| format!(" Did you mean {suggestion}?"));
    output.stderr.push_str(&format!("sx help: nothing is named {name}.{}\n", hint.unwrap_or_default()));
    Ok(EXIT_REJECTED)
}

fn declared_inputs(registry: &Registry, declarations: &Json) -> Result<BTreeMap<String, Type>, Stop> {
    let raw = match declarations.get("inputs") {
        None => return Ok(BTreeMap::new()),
        Some(Json::Object(entries)) => entries,
        Some(_) => return Err(Stop::usage("declarations: inputs must map names to types")),
    };
    let mut inputs = BTreeMap::new();
    for (name, written) in raw {
        let text = match written {
            Json::String(s) => s.clone(),
            other => python_repr(other),
        };
        let parsed = registry.parse(&text).map_err(|e| Stop::usage(format!("declarations: {e}")))?;
        inputs.insert(name.clone(), parsed);
    }
    Ok(inputs)
}

fn span_json(span: Option<Span>) -> Json {
    let Some(span) = span else {
        return Json::Null;
    };
    let point = |line: usize, column: usize| {
        Json::object([("line", Json::Int(line as i64)), ("column", Json::Int(column as i64))])
    };
    Json::object([
        ("start", point(span.start.line, span.start.column)),
        ("end", point(span.end.line, span.end.column)),
    ])
}

fn diagnostic_json(diagnostic: &Diagnostic) -> Json {
    let optional = |text: &Option<String>| text.as_deref().map_or(Json::Null, Json::string);
    Json::object([
        ("code", Json::string(diagnostic.code)),
        ("construct", diagnostic.construct.map_or(Json::Null, Json::string)),
        ("span", span_json(Some(diagnostic.span))),
        ("message", Json::string(diagnostic.message.as_str())),
        ("detail", optional(&diagnostic.detail)),
        ("fix", optional(&diagnostic.fix)),
        ("help", optional(&diagnostic.help)),
    ])
}

fn command_program(args: &ProgramArgs, base: Registry, output: &mut Output) -> Result<i32, Stop> {
    let (registry, declarations) = load_registry(args.declarations.as_deref(), base)?;
    let shown = path_text(&args.file);
    let source = read_text(Path::new(&args.file)).map_err(|e| Stop::usage(format!("cannot read {shown}: {e}")))?;
    let allowed: BTreeSet<String> = args.allow.iter().cloned().collect();
    if let Some(unknown) = allowed.iter().find(|id| registry.capability(id).is_none()) {
        return Err(Stop::usage(format!("--allow names a tool the declarations do not define: {unknown}")));
    }
    let inputs = declared_inputs(&registry, &declarations)?;
    let checking: BTreeSet<String> = match args.kind {
        ProgramKind::Explain => registry.capabilities().map(|c| c.id.clone()).collect(),
        _ => allowed.clone(),
    };
    let checked = compile_source(&source, &registry, &checking, &inputs, Budgets::default());
    let Some(program) = checked.program else {
        let payload = Json::object([
            ("ok", Json::Bool(false)),
            ("diagnostics", Json::Array(checked.diagnostics.iter().map(diagnostic_json).collect())),
        ]);
        output.emit(args.format, payload, render(&source, &checked.diagnostics).trim_end_matches('\n'))?;
        return Ok(EXIT_REJECTED);
    };
    match &args.kind {
        ProgramKind::Check { emit_ir: true } => {
            let shown = dumps(&export_program(&program), Layout::Spaced).map_err(|e| Stop::Internal(e.to_string()))?;
            output.line(&shown);
            Ok(EXIT_OK)
        }
        ProgramKind::Check { emit_ir: false } => {
            let result = show_type(&program.result);
            let payload = Json::object([("ok", Json::Bool(true)), ("result", Json::string(result.as_str()))]);
            output.emit(args.format, payload, &format!("ok: {result}"))?;
            Ok(EXIT_OK)
        }
        ProgramKind::Explain => command_explain(args, &program, &allowed, output),
        ProgramKind::Run { responses, inputs: pairs } => {
            let values = input_values(pairs, &inputs)?;
            let responses = load_responses(responses.as_deref())?;
            command_run(args.format, &program, &responses, &values, output)
        }
    }
}

fn command_explain(
    args: &ProgramArgs,
    program: &Program,
    allowed: &BTreeSet<String>,
    output: &mut Output,
) -> Result<i32, Stop> {
    let calls = literal_calls(&program.body);
    let mut capabilities = Vec::new();
    let mut lines = vec!["tools:".to_string()];
    let mut ids = program.capabilities.clone();
    ids.sort();
    for id in &ids {
        let signature = program
            .signatures
            .capabilities
            .get(id)
            .ok_or_else(|| Stop::Internal(format!("the program calls {id} without a signature")))?;
        let effect = effect_label(signature.effect, signature.idempotent);
        let known: Vec<Json> = calls
            .iter()
            .filter(|(called, _)| called == id)
            .map(|(_, input)| input.clone().unwrap_or_else(|| Json::string(RUNTIME)))
            .collect();
        let granted = allowed.contains(id);
        lines.push(format!("  {id}  [{effect}]  {}", if granted { "allowed" } else { "not allowed" }));
        for input in &known {
            let text = match input {
                Json::String(s) if s == RUNTIME => RUNTIME.to_string(),
                other => dumps(other, Layout::Spaced).map_err(|e| Stop::Internal(e.to_string()))?,
            };
            lines.push(format!("    input: {text}"));
        }
        capabilities.push(Json::object([
            ("id", Json::string(id.as_str())),
            ("effect", Json::string(effect)),
            ("allowed", Json::Bool(granted)),
            ("inputs", Json::Array(known)),
        ]));
    }
    let names: Vec<&str> = program.inputs.keys().map(String::as_str).collect();
    let listed = if names.is_empty() { "none".to_string() } else { names.join(", ") };
    let result = show_type(&program.result);
    lines.push(format!("inputs: {listed}"));
    lines.push(format!("result: {result}"));
    let payload = Json::object([
        ("tools", Json::Array(capabilities)),
        ("inputs", Json::Array(names.iter().map(|n| Json::string(*n)).collect())),
        ("result", Json::string(result.as_str())),
    ]);
    output.emit(args.format, payload, &lines.join("\n"))?;
    Ok(EXIT_OK)
}

fn input_values(pairs: &[String], inputs: &BTreeMap<String, Type>) -> Result<Vec<(String, Value)>, Stop> {
    let mut values: Vec<(String, Value)> = Vec::new();
    for pair in pairs {
        let Some((name, text)) = pair.split_once('=') else {
            return Err(Stop::usage(format!("--input takes NAME=JSON, not {}", repr_str(pair))));
        };
        let Some(declared) = inputs.get(name) else {
            return Err(Stop::usage(format!("--input {name} is not declared in the declarations' inputs")));
        };
        let parsed = pyjson::parse(text).map_err(|e| Stop::usage(format!("--input {name}: {e}")))?;
        let value = decode(&parsed, declared, &format!("${name}"))
            .map_err(|e| Stop::usage(format!("--input {name}: {e}")))?;
        match values.iter_mut().find(|(held, _)| held == name) {
            Some(slot) => slot.1 = value,
            None => values.push((name.to_string(), value)),
        }
    }
    if let Some(missing) = inputs.keys().find(|name| !values.iter().any(|(held, _)| held == *name)) {
        return Err(Stop::usage(format!("the program needs a value for input {missing}: --input {missing}=JSON")));
    }
    Ok(values)
}

fn load_responses(path: Option<&str>) -> Result<Json, Stop> {
    let Some(path) = path else {
        return Ok(Json::Array(Vec::new()));
    };
    let responses = read_json(path, "responses")?;
    if !matches!(responses, Json::Array(_)) {
        return Err(Stop::usage(format!("responses {} must be a JSON list", path_text(path))));
    }
    Ok(responses)
}

fn command_run(
    format: Format,
    program: &Program,
    responses: &Json,
    values: &[(String, Value)],
    output: &mut Output,
) -> Result<i32, Stop> {
    let mut host = FixtureHost::from_responses(&program.signatures.capabilities, responses)
        .map_err(|e| Stop::usage(e.to_string()))?;
    let finished = match run(program, &mut host, values, Budgets::default(), &Cores) {
        Ok(finished) => finished,
        Err(error) => {
            let calls = Json::Array(host.calls.clone());
            let payload = Json::object([
                ("outcome", Json::string("unexpected-call")),
                ("message", Json::string(error.to_string())),
                ("calls", calls),
            ]);
            output.emit(format, payload, &format!("sx run: {error}"))?;
            return Ok(EXIT_FAILED);
        }
    };
    let trace = Json::Array(finished.machine.trace().to_vec());
    match finished.outcome {
        Outcome::Completed { value, json } => {
            let payload =
                Json::object([("outcome", Json::string("completed")), ("result", json), ("trace", trace)]);
            output.emit(format, payload, &show_value(&value))?;
            Ok(EXIT_OK)
        }
        Outcome::Failed(failure) => {
            let payload = Json::object([
                ("outcome", Json::string("failed")),
                (
                    "failure",
                    Json::object([
                        ("kind", Json::string(failure.kind.name())),
                        ("message", Json::string(failure.message.as_str())),
                        ("request", failure.request.as_deref().map_or(Json::Null, Json::string)),
                        ("span", span_json(failure.span)),
                    ]),
                ),
                ("trace", trace),
            ]);
            let place = failure.span.map(|span| format!(" at {span}")).unwrap_or_default();
            output.emit(format, payload, &format!("failed[{}]{place}: {}", failure.kind.name(), failure.message))?;
            Ok(EXIT_FAILED)
        }
        Outcome::Suspended(_) => Err(Stop::Internal("the run ended suspended".to_string())),
    }
}
