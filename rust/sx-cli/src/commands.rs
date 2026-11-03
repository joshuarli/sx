use std::collections::BTreeMap;
use std::path::Path;

use sx_cores::kernel::{Cores, core as builtin_core};
use sx_kernel::authority::{Authority, GrantError, Grants};
use sx_kernel::budgets::Budgets;
use sx_kernel::checker::{closest, input_disagreement};
use sx_kernel::closers::{CloserEdit, Compiled, compile_source_inferring, edits_json, edits_note};
use sx_kernel::diagnostics::{Diagnostic, render};
use sx_kernel::explain::{ExplainedTool, RUNTIME_INPUT, explain_tools};
use sx_kernel::export::export_program;
use sx_kernel::help::{guide, lookup, names, render_cores};
use sx_kernel::host::{FixtureHost, run};
use sx_kernel::inputs::{InputProblem, supply};
use sx_kernel::ir::Program;
use sx_kernel::json::{Json, Layout, dumps, python_repr, repr_str};
use sx_kernel::machine::{Machine, Outcome};
use sx_kernel::registry::{Registry, load_declarations_with};
use sx_packs::{Declared, DeclaredLoader};
use sx_packs::mcp::{Bindings, Session};
use sx_kernel::syntax::Span;
use sx_kernel::types::{Type, show_type};
use sx_kernel::values::{Value, decode, show_value};

use crate::argparse::{CoresArgs, Emit, Format, HelpArgs, Invocation, Language, ProgramArgs, ProgramKind};
use crate::mcp::Host;
use crate::pyfile::{path_text, read_text};
use crate::{EXIT_FAILED, EXIT_OK, EXIT_REJECTED, EXIT_USAGE, Exit, pyjson};

pub(crate) enum Stop {
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
    inferred: Vec<CloserEdit>,
}

impl Output {
    fn line(&mut self, text: &str) {
        self.stdout.push_str(text);
        self.stdout.push('\n');
    }

    fn emit(&mut self, format: Format, mut payload: Json, text: &str) -> Result<(), Stop> {
        match format {
            Format::Json => {
                if let (Json::Object(fields), false) = (&mut payload, self.inferred.is_empty()) {
                    fields.push(("inferred_closers".to_string(), edits_json(&self.inferred)));
                }
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
    let result = dispatch(invocation, registry, &mut output);
    conclude(result, output)
}

pub(crate) fn execute_on_mcp(args: &ProgramArgs, registry: Registry, host: &Host) -> Exit {
    let mut output = Output::default();
    let result = command_program(args, registry, &mut output, Some(host));
    conclude(result, output)
}

fn conclude(result: Result<i32, Stop>, mut output: Output) -> Exit {
    let code = match result {
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
        Invocation::Program(args) => command_program(&args, registry, output, None),
    }
}

pub(crate) fn read_json(path: &str, what: &str) -> Result<Json, Stop> {
    let shown = path_text(path);
    let text = read_text(Path::new(path)).map_err(|e| Stop::usage(format!("cannot read {what} {shown}: {e}")))?;
    pyjson::parse(&text).map_err(|e| Stop::usage(format!("cannot read {what} {shown}: {e}")))
}

fn listed_ids(declarations: &Json, shown: &str, key: &str, what: &str) -> Result<Vec<String>, Stop> {
    let listed = match declarations.get(key) {
        None | Some(Json::Null) => return Ok(Vec::new()),
        Some(Json::Array(items)) => items,
        Some(other) => {
            return Err(Stop::usage(format!(
                "invalid declarations {shown}: {key} must be a list of {what} names, not {}",
                python_repr(other)
            )));
        }
    };
    listed
        .iter()
        .map(|item| {
            item.as_str().map(str::to_string).ok_or_else(|| {
                Stop::usage(format!("invalid declarations {shown}: {what} names must be strings, not {}", python_repr(item)))
            })
        })
        .collect()
}

pub(crate) fn load_registry(path: Option<&str>, mut registry: Registry) -> Result<(Registry, Json, Declared), Stop> {
    let Some(path) = path else {
        return Ok((registry, Json::Object(Vec::new()), Declared::default()));
    };
    let shown = path_text(path);
    let declarations = read_json(path, "declarations")?;
    if !matches!(declarations, Json::Object(_)) {
        return Err(Stop::usage(format!("declarations {shown} must be a JSON object")));
    }
    for id in listed_ids(&declarations, &shown, "cores", "core")? {
        if registry.core(&id).is_some() {
            continue;
        }
        let core = builtin_core(&id).ok_or_else(|| {
            Stop::usage(format!("invalid declarations {shown}: no built-in core is named {}", repr_str(&id)))
        })?;
        registry.add_core(core).map_err(|e| Stop::usage(format!("invalid declarations {shown}: {e}")))?;
    }
    for id in listed_ids(&declarations, &shown, "packs", "pack")? {
        if registry.pack_ids().contains(&id) {
            continue;
        }
        let pack = sx_packs::pack(&id).ok_or_else(|| {
            Stop::usage(format!("invalid declarations {shown}: no built-in pack is named {}", repr_str(&id)))
        })?;
        registry.add_pack(pack).map_err(|e| Stop::usage(format!("invalid declarations {shown}: {e}")))?;
    }
    let mut loader = DeclaredLoader::new();
    load_declarations_with(&mut registry, &declarations, &mut loader)
        .map_err(|e| Stop::usage(format!("invalid declarations {shown}: {e}")))?;
    Ok((registry, declarations, loader.finish()))
}

const TYPESCRIPT_CORES: [&str; 2] = ["js", "text"];

pub(crate) fn add_typescript_cores(registry: &mut Registry) -> Result<(), Stop> {
    for id in TYPESCRIPT_CORES {
        if registry.core(id).is_some() {
            continue;
        }
        let core = builtin_core(id).ok_or_else(|| Stop::Internal(format!("the built-in core {id} is missing")))?;
        registry.add_core(core).map_err(|e| Stop::usage(format!("the {id} core that TypeScript programs use cannot be added: {e}")))?;
    }
    Ok(())
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
    let (registry, _, _) = load_registry(args.declarations.as_deref(), base)?;
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

pub(crate) fn declared_inputs(registry: &Registry, declarations: &Json) -> Result<BTreeMap<String, Type>, Stop> {
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

pub(crate) fn span_json(span: Option<Span>) -> Json {
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

pub(crate) fn diagnostic_json(diagnostic: &Diagnostic) -> Json {
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

fn command_program(args: &ProgramArgs, base: Registry, output: &mut Output, mcp: Option<&Host>) -> Result<i32, Stop> {
    let (mut registry, declarations, declared) = load_registry(args.declarations.as_deref(), base)?;
    if args.language == Language::TypeScript {
        add_typescript_cores(&mut registry)?;
    }
    let shown = path_text(&args.file);
    let source = read_text(Path::new(&args.file)).map_err(|e| Stop::usage(format!("cannot read {shown}: {e}")))?;
    let mut allowed: Vec<&String> = args.allow.iter().collect();
    allowed.sort();
    if let Some(unknown) = allowed.iter().find(|id| registry.capability(id).is_none()) {
        return Err(Stop::usage(format!("--allow names a tool the declarations do not define: {unknown}")));
    }
    let grants = load_grants(args, &registry)?;
    let inputs = declared_inputs(&registry, &declarations)?;
    let authority = match args.kind {
        ProgramKind::Explain => Authority::Unrestricted,
        _ => Authority::Granted(&grants),
    };
    if args.language == Language::TypeScript && matches!(args.kind, ProgramKind::Check { emit: Some(Emit::Sx) }) {
        return emit_lowered(&source, &registry, args.format, output);
    }
    if args.language == Language::Sx && matches!(args.kind, ProgramKind::Check { emit: Some(Emit::Sx) }) {
        return Err(Stop::usage("--emit sx prints the lowering of a TypeScript program; pass --language ts"));
    }
    let compiled = match args.language {
        Language::Sx => compile_source_inferring(&source, &registry, authority, &inputs, Budgets::default()),
        Language::TypeScript => Compiled {
            check: sx_ts::compile_source(&source, &registry, authority, &inputs, Budgets::default()),
            source: source.clone(),
            edits: Vec::new(),
        },
    };
    let source = compiled.source;
    let checked = compiled.check;
    output.inferred = compiled.edits;
    let emits_ir = matches!(args.kind, ProgramKind::Check { emit: Some(Emit::Ir) });
    if !output.inferred.is_empty() && (args.format == Format::Text || emits_ir) {
        output.stderr.push_str(&format!("{}\n", edits_note(&output.inferred)));
    }
    let Some(program) = checked.program else {
        let payload = Json::object([
            ("ok", Json::Bool(false)),
            ("diagnostics", Json::Array(checked.diagnostics.iter().map(diagnostic_json).collect())),
        ]);
        output.emit(args.format, payload, render(&source, &checked.diagnostics).trim_end_matches('\n'))?;
        return Ok(EXIT_REJECTED);
    };
    let script = checked.header_inputs;
    if let Some(header) = &script
        && let Some(difference) = input_disagreement(header, &inputs)
    {
        return Err(Stop::usage(difference));
    }
    match &args.kind {
        ProgramKind::Check { emit: Some(Emit::Ir) } => {
            let shown = dumps(&export_program(&program), Layout::Spaced).map_err(|e| Stop::Internal(e.to_string()))?;
            output.line(&shown);
            Ok(EXIT_OK)
        }
        ProgramKind::Check { emit: Some(Emit::Sx) } => Err(Stop::Internal("--emit sx is handled before checking".to_string())),
        ProgramKind::Check { emit: None } => {
            let result = show_type(&program.result);
            let payload = Json::object([("ok", Json::Bool(true)), ("result", Json::string(result.as_str()))]);
            output.emit(args.format, payload, &format!("ok: {result}"))?;
            Ok(EXIT_OK)
        }
        ProgramKind::Explain => command_explain(args, &program, &registry, &grants, script.as_ref(), output),
        ProgramKind::Run { responses, inputs: pairs } => {
            let values = match &script {
                Some(header) => script_input_values(pairs, header)?,
                None => input_values(pairs, &inputs)?,
            };
            if let Some(host) = mcp {
                return command_run_mcp(args.format, &program, &grants, &declared, &values, (host, &registry, &declarations), output);
            }
            match (responses.as_deref(), &args.grants) {
                (None, Some(_)) => command_run_native(args.format, &program, &grants, &declared, None, &values, output),
                (responses, _) => {
                    let responses = load_responses(responses)?;
                    command_run(args.format, &program, &grants, &responses, &values, output)
                }
            }
        }
    }
}

fn emit_lowered(source: &str, registry: &Registry, format: Format, output: &mut Output) -> Result<i32, Stop> {
    match sx_ts::lower_source(source, registry) {
        Ok(program) => {
            output.stdout.push_str(&sx_ts::pretty::pretty(&program));
            Ok(EXIT_OK)
        }
        Err(diagnostics) => {
            let payload = Json::object([
                ("ok", Json::Bool(false)),
                ("diagnostics", Json::Array(diagnostics.iter().map(diagnostic_json).collect())),
            ]);
            output.emit(format, payload, render(source, &diagnostics).trim_end_matches('\n'))?;
            Ok(EXIT_REJECTED)
        }
    }
}

fn load_grants(args: &ProgramArgs, registry: &Registry) -> Result<Grants, Stop> {
    let mut grants = match &args.grants {
        None => Grants::new(),
        Some(path) => {
            let json = read_json(path, "grants")?;
            Grants::load(registry, &json)
                .map_err(|e| Stop::usage(format!("invalid grants {}: {e}", path_text(path))))?
        }
    };
    for id in &args.allow {
        grants.grant_unscoped(registry, id).map_err(|e| match e {
            GrantError::MissingScope { .. } => Stop::usage(format!("--allow {id}: {e}; grant it with --grants")),
            other => Stop::usage(format!("--allow {id}: {other}")),
        })?;
    }
    Ok(grants)
}

fn command_explain(
    args: &ProgramArgs,
    program: &Program,
    registry: &Registry,
    grants: &Grants,
    script: Option<&BTreeMap<String, Type>>,
    output: &mut Output,
) -> Result<i32, Stop> {
    let tools = explain_tools(program, registry, grants).map_err(Stop::Internal)?;
    let (json, text) = explain_report(program, &tools, script, args.language.into())?;
    output.emit(args.format, json, &text)?;
    Ok(EXIT_OK)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Naming {
    Id,
    TypeScript,
}

impl Naming {
    pub(crate) fn tool(self, id: &str) -> String {
        match self {
            Naming::Id => id.to_string(),
            Naming::TypeScript => sx_ts::terms::tool_name(id),
        }
    }
}

impl From<Language> for Naming {
    fn from(language: Language) -> Naming {
        match language {
            Language::Sx => Naming::Id,
            Language::TypeScript => Naming::TypeScript,
        }
    }
}

pub(crate) fn explain_report(
    program: &Program,
    tools: &[ExplainedTool],
    script: Option<&BTreeMap<String, Type>>,
    naming: Naming,
) -> Result<(Json, String), Stop> {
    let mut lines = vec!["tools:".to_string()];
    for tool in tools {
        lines.push(format!("  {}  [{}]  {}", naming.tool(&tool.id), tool.effect, if tool.allowed { "allowed" } else { "not allowed" }));
        for call in &tool.calls {
            let text = match &call.input {
                None => RUNTIME_INPUT.to_string(),
                Some(literal) => dumps(&literal.json(), Layout::Spaced).map_err(|e| Stop::Internal(e.to_string()))?,
            };
            match &call.scope {
                None => lines.push(format!("    input: {text}")),
                Some(scope) => lines.push(format!("    input: {text}  scope: {}", scope.text())),
            }
        }
    }
    let names: Vec<&str> = script.unwrap_or(&program.inputs).keys().map(String::as_str).collect();
    let listed = match script {
        _ if names.is_empty() => "none".to_string(),
        Some(declared) => declared.iter().map(|(name, t)| format!("{name}: {}", show_type(t))).collect::<Vec<_>>().join(", "),
        None => names.join(", "),
    };
    let result = show_type(&program.result);
    lines.push(format!("inputs: {listed}"));
    lines.push(format!("result: {result}"));
    let mut fields = vec![
        ("tools", Json::Array(tools.iter().map(|tool| tool.json()).collect())),
        ("inputs", Json::Array(names.iter().map(|n| Json::string(*n)).collect())),
    ];
    if let Some(declared) = script {
        let types = declared.iter().map(|(name, t)| (name.clone(), Json::string(show_type(t)))).collect();
        fields.push(("input_types", Json::Object(types)));
    }
    fields.push(("result", Json::string(result.as_str())));
    Ok((Json::object(fields), lines.join("\n")))
}

pub(crate) fn script_input_values(pairs: &[String], header: &BTreeMap<String, Type>) -> Result<Vec<(String, Value)>, Stop> {
    let mut supplied: Vec<(String, Json)> = Vec::new();
    for pair in pairs {
        let Some((name, text)) = pair.split_once('=') else {
            return Err(Stop::usage(format!("--input takes NAME=JSON, not {}", repr_str(pair))));
        };
        let parsed = pyjson::parse(text).map_err(|e| Stop::usage(format!("--input {name}: {e}")))?;
        supplied.push((name.to_string(), parsed));
    }
    supply(header, &supplied).map_err(|problem| match &problem {
        InputProblem::Unknown { .. } => Stop::usage(format!("--input {problem}")),
        InputProblem::Missing { .. } => Stop::usage(format!("{problem}; pass each as --input NAME=JSON")),
        InputProblem::Mismatch { name, error } => Stop::usage(format!("--input {name}: {error}")),
    })
}

pub(crate) fn input_values(pairs: &[String], inputs: &BTreeMap<String, Type>) -> Result<Vec<(String, Value)>, Stop> {
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

fn command_run_mcp(
    format: Format,
    program: &Program,
    grants: &Grants,
    declared: &Declared,
    values: &[(String, Value)],
    (host, registry, declarations): (&Host, &Registry, &Json),
    output: &mut Output,
) -> Result<i32, Stop> {
    let bindings = Bindings::from_declarations(registry, declarations, host.name.as_deref())
        .map_err(|e| Stop::usage(format!("invalid declarations: {e}")))?;
    if !program.capabilities.iter().any(|id| bindings.get(id).is_some()) {
        return command_run_native(format, program, grants, declared, None, values, output);
    }
    let session = Session::open(bindings, &host.command, host.limits)
        .map_err(|e| Stop::usage(format!("cannot connect to the MCP server: {e}")))?;
    command_run_native(format, program, grants, declared, Some(&session), values, output)
}

fn command_run_native(
    format: Format,
    program: &Program,
    grants: &Grants,
    declared: &Declared,
    mcp: Option<&Session>,
    values: &[(String, Value)],
    output: &mut Output,
) -> Result<i32, Stop> {
    let missing = crate::native::unimplemented(program, declared, mcp);
    if !missing.is_empty() {
        return Err(Stop::usage(format!(
            "no pack implements {}, so the program cannot run without --responses",
            missing.join(", ")
        )));
    }
    let (outcome, machine) = crate::native::run(program, grants, declared, mcp, values, Budgets::default());
    report_run(format, outcome, &machine, output)
}

fn command_run(
    format: Format,
    program: &Program,
    grants: &Grants,
    responses: &Json,
    values: &[(String, Value)],
    output: &mut Output,
) -> Result<i32, Stop> {
    let mut host = FixtureHost::from_responses(&program.signatures.capabilities, responses)
        .map_err(|e| Stop::usage(e.to_string()))?;
    let finished = match run(program, grants, &mut host, values, Budgets::default(), &Cores) {
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
    report_run(format, finished.outcome, &finished.machine, output)
}

fn report_run(format: Format, outcome: Outcome, machine: &Machine<'_>, output: &mut Output) -> Result<i32, Stop> {
    let report = run_report(outcome, machine)?;
    if format == Format::Text {
        for line in &report.logs {
            output.stderr.push_str(&format!("log: {line}\n"));
        }
    }
    output.emit(format, report.json, &report.text)?;
    Ok(if report.completed { EXIT_OK } else { EXIT_FAILED })
}

pub(crate) struct RunReport {
    pub completed: bool,
    pub json: Json,
    pub text: String,
    pub logs: Vec<String>,
}

pub(crate) fn run_report(outcome: Outcome, machine: &Machine<'_>) -> Result<RunReport, Stop> {
    let trace = Json::Array(machine.trace().to_vec());
    let logs = machine.logs().to_vec();
    let logged = |mut fields: Vec<(&'static str, Json)>| {
        if !logs.is_empty() {
            fields.push(("logs", Json::Array(logs.iter().map(|line| Json::string(line.as_str())).collect())));
        }
        fields.push(("trace", trace.clone()));
        Json::object(fields)
    };
    match outcome {
        Outcome::Completed { value, json } => Ok(RunReport {
            completed: true,
            json: logged(vec![("outcome", Json::string("completed")), ("result", json)]),
            text: show_value(&value),
            logs: logs.clone(),
        }),
        Outcome::Failed(failure) => {
            let json = logged(vec![
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
            ]);
            let place = failure.span.map(|span| format!(" at {span}")).unwrap_or_default();
            Ok(RunReport {
                completed: false,
                json,
                text: format!("failed[{}]{place}: {}", failure.kind.name(), failure.message),
                logs: logs.clone(),
            })
        }
        Outcome::Suspended(_) => Err(Stop::Internal("the run ended suspended".to_string())),
    }
}
