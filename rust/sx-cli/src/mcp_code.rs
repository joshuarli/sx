use std::collections::BTreeMap;
use std::io;
use std::panic::{AssertUnwindSafe, catch_unwind};

use sx_kernel::authority::{Authority, Grants};
use sx_kernel::budgets::Budgets;
use sx_kernel::checker::input_disagreement;
use sx_kernel::closers::{CloserEdit, Compiled, compile_source_inferring, edits_json};
use sx_kernel::diagnostics::render;
use sx_kernel::explain::{ExplainedTool, explain_tools};
use sx_kernel::ir::Program;
use sx_kernel::json::{Json, Layout, dumps};
use sx_kernel::registry::Registry;
use sx_kernel::types::Type;
use sx_kernel::values::Value;
use sx_packs::Declared;
use sx_packs::mcp::{Answer, Bindings, Session, ToolHost, serve as serve_connection};

use crate::argparse::Language;
use crate::commands::{
    Naming, Stop, add_typescript_cores, declared_inputs, diagnostic_json, explain_report, input_values, load_registry, read_json,
    run_report, script_input_values,
};
use crate::guide::{guide_text, notes_text};
use crate::mcp::{Host, scan, usage};
use crate::mcp_policy::{Policy, Verdict};
use crate::mcp_serve::{grants_of, stopped};
use crate::pyfile::path_text;
use crate::{EXIT_FAILED, EXIT_OK, Exit};

const INVALID_PARAMS: i64 = -32602;
const CLIPPED_CHARACTERS: usize = 200;

struct McpTools {
    bindings: Bindings,
    host: Host,
}

struct CodeServer {
    registry: Registry,
    declared: Declared,
    grants: Grants,
    inputs: BTreeMap<String, Type>,
    policy: Policy,
    mcp: Option<McpTools>,
    guide: String,
    language: Language,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Check,
    Run,
}

impl Tool {
    fn from_name(name: &str) -> Option<Tool> {
        match name {
            "check" => Some(Tool::Check),
            "run" => Some(Tool::Run),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Tool::Check => "check",
            Tool::Run => "run",
        }
    }

    fn arguments(self, language: Language) -> &'static [&'static str] {
        match (self, language) {
            (Tool::Check, _) | (Tool::Run, Language::TypeScript) => &["program"],
            (Tool::Run, Language::Sx) => &["program", "inputs"],
        }
    }
}

struct Proposal {
    program: Program,
    script: Option<BTreeMap<String, Type>>,
    tools: Vec<ExplainedTool>,
    verdict: Verdict,
    inferred: Vec<CloserEdit>,
}

enum Examined {
    Rejected { diagnostics: Json, text: String, inferred: Vec<CloserEdit> },
    Proposed(Box<Proposal>),
}

struct Reply {
    is_error: bool,
    structured: Json,
    text: String,
}

fn text_block(text: &str) -> Json {
    Json::object([("type", Json::string("text")), ("text", Json::string(text))])
}

fn tool_result(is_error: bool, text: &str, structured: Json) -> Json {
    Json::object([
        ("content", Json::Array(vec![text_block(text)])),
        ("structuredContent", structured),
        ("isError", Json::Bool(is_error)),
    ])
}

fn failed_result(message: &str) -> Json {
    tool_result(true, message, Json::object([("error", Json::string(message))]))
}

fn with_inferred(structured: Json, inferred: &[CloserEdit]) -> Json {
    match structured {
        Json::Object(mut fields) if !inferred.is_empty() => {
            fields.push(("inferred_closers".to_string(), edits_json(inferred)));
            Json::Object(fields)
        }
        other => other,
    }
}

fn clipped(text: &str) -> String {
    match text.char_indices().nth(CLIPPED_CHARACTERS) {
        Some((end, _)) => format!("{}...", text.get(..end).unwrap_or(text)),
        None => text.to_string(),
    }
}

fn shown(json: &Json) -> String {
    clipped(&dumps(json, Layout::Spaced).unwrap_or_else(|error| error.0))
}

fn logs_text(logs: &[String]) -> String {
    if logs.is_empty() {
        return String::new();
    }
    let mut lines = vec![format!("logs ({}):", logs.len())];
    lines.extend(logs.iter().map(|line| format!("  {}", clipped(line))));
    lines.join("\n") + "\n"
}

fn calls_text(trace: &Json, naming: Naming) -> String {
    let mut calls: Vec<(String, String, Option<String>)> = Vec::new();
    for entry in trace.as_array().unwrap_or_default() {
        if let Some(id) = entry.get("request").and_then(Json::as_str) {
            let capability = entry.get("capability").and_then(Json::as_str).map_or("?".to_string(), |id| naming.tool(id));
            let input = entry.get("input").map(shown).unwrap_or_default();
            calls.push((id.to_string(), format!("{capability} {input}"), None));
        } else if let Some(id) = entry.get("response").and_then(Json::as_str) {
            let outcome = match (entry.get("output"), entry.get("error")) {
                (Some(output), _) => format!("ok {}", shown(output)),
                (None, Some(error)) => {
                    let kind = error.get("kind").and_then(Json::as_str).unwrap_or("?");
                    let message = error.get("message").and_then(Json::as_str).unwrap_or("");
                    format!("failed[{kind}] {}", clipped(message))
                }
                (None, None) => "answered".to_string(),
            };
            if let Some(call) = calls.iter_mut().find(|(held, ..)| held == id) {
                call.2 = Some(outcome);
            }
        }
    }
    if calls.is_empty() {
        return "calls: none".to_string();
    }
    let mut lines = vec![format!("calls ({}):", calls.len())];
    for (number, (_, call, outcome)) in calls.iter().enumerate() {
        lines.push(format!("  {}. {call} -> {}", number + 1, outcome.as_deref().unwrap_or("no answer")));
    }
    lines.join("\n")
}

impl CodeServer {
    fn naming(&self) -> Naming {
        self.language.into()
    }

    fn describe(&self, tool: Tool) -> String {
        if self.language == Language::TypeScript {
            return self.describe_typescript(tool);
        }
        let purpose = match tool {
            Tool::Check => {
                "Check a program written in Sx and perform no effect. Returns the \
                 diagnostics when the program does not check. When it checks, returns its explain report, every tool \
                 it can call with each literal input and that input's scope verdict, and whether the approval policy \
                 would let it run. Use it to repair a program before running it."
            }
            Tool::Run => {
                "Run a program written in Sx, the language described below. The host checks the program, explains it, \
                 judges it against the approval policy, and only then runs it against the real tools. A program that \
                 does not check, or that the policy refuses, performs nothing and the result says why. Otherwise the \
                 result holds the program's value and every call made with its outcome. A call that already ran is \
                 not undone when a later call fails."
            }
        };
        match tool {
            Tool::Check => format!(
                "{purpose} The language and its tools are described in the run tool's description.\n\n{}",
                self.policy.describe()
            ),
            Tool::Run => format!("{purpose}\n\n{}\n\n{}", self.policy.describe(), self.guide.trim_end()),
        }
    }

    fn describe_typescript(&self, tool: Tool) -> String {
        let purpose = match tool {
            Tool::Check => {
                "Check a program written in checkable TypeScript and perform no effect. Returns the diagnostics, with \
                 line and column in the program you wrote, when it does not check. When it checks, returns its explain \
                 report, every tool it can call with each literal input and that input's scope verdict, and whether the \
                 approval policy would let it run. Use it to repair a program before running it."
            }
            Tool::Run => {
                "Run a program written in checkable TypeScript, a subset described below. The program is the body of an \
                 async function that calls tools as tools.namespace.name({ ... }). The host checks the program, explains \
                 it, judges it against the approval policy, and only then runs it against the real tools. A program that \
                 does not check, or that the policy refuses, performs nothing and the result says why. Otherwise the \
                 result holds the program's value, the console.log lines as logs, and every call made with its outcome. \
                 A call that already ran is not undone when a later call fails."
            }
        };
        match tool {
            Tool::Check => format!(
                "{purpose} The tools and the subset are described in the run tool's description.\n\n{}",
                self.policy.describe()
            ),
            Tool::Run => format!("{purpose}\n\n{}\n\n{}", self.policy.describe(), self.guide.trim_end()),
        }
    }

    fn listing(&self) -> Vec<Json> {
        let program_text = match self.language {
            Language::Sx => "The Sx program text.",
            Language::TypeScript => "The TypeScript program text.",
        };
        let program = Json::object([("type", Json::string("string")), ("description", Json::string(program_text))]);
        let inputs = Json::object([
            ("type", Json::string("object")),
            (
                "description",
                Json::string("Values for the inputs the program declares, as NAME to JSON value."),
            ),
        ]);
        [Tool::Check, Tool::Run]
            .into_iter()
            .map(|tool| {
                let mut properties = vec![("program", program.clone())];
                if tool == Tool::Run && self.language == Language::Sx {
                    properties.push(("inputs", inputs.clone()));
                }
                let schema = Json::object([
                    ("type", Json::string("object")),
                    ("properties", Json::object(properties)),
                    ("required", Json::Array(vec![Json::string("program")])),
                    ("additionalProperties", Json::Bool(false)),
                ]);
                let annotations = match tool {
                    Tool::Check => Json::object([("readOnlyHint", Json::Bool(true))]),
                    Tool::Run => Json::object([
                        ("readOnlyHint", Json::Bool(false)),
                        ("destructiveHint", Json::Bool(true)),
                        ("openWorldHint", Json::Bool(true)),
                    ]),
                };
                Json::object([
                    ("name", Json::string(tool.name())),
                    ("description", Json::string(self.describe(tool))),
                    ("inputSchema", schema),
                    ("annotations", annotations),
                ])
            })
            .collect()
    }

    fn examine(&self, source: &str) -> Result<Examined, Stop> {
        let authority = Authority::Granted(&self.grants);
        let compiled = match self.language {
            Language::Sx => compile_source_inferring(source, &self.registry, authority, &self.inputs, Budgets::default()),
            Language::TypeScript => Compiled {
                check: sx_ts::compile_source(source, &self.registry, authority, &self.inputs, Budgets::default()),
                source: source.to_string(),
                edits: Vec::new(),
            },
        };
        let checked = compiled.check;
        let Some(program) = checked.program else {
            return Ok(Examined::Rejected {
                diagnostics: Json::Array(checked.diagnostics.iter().map(diagnostic_json).collect()),
                text: render(&compiled.source, &checked.diagnostics).trim_end_matches('\n').to_string(),
                inferred: compiled.edits,
            });
        };
        let script = checked.header_inputs;
        if let Some(header) = &script
            && let Some(difference) = input_disagreement(header, &self.inputs)
        {
            return Err(Stop::Usage(difference));
        }
        let tools = explain_tools(&program, &self.registry, &self.grants).map_err(Stop::Internal)?;
        let verdict = self.policy.judge(&tools);
        Ok(Examined::Proposed(Box::new(Proposal { program, script, tools, verdict, inferred: compiled.edits })))
    }

    fn check(&self, source: &str) -> Result<Reply, Stop> {
        Ok(match self.examine(source)? {
            Examined::Rejected { diagnostics, text, inferred } => Reply {
                is_error: true,
                structured: with_inferred(
                    Json::object([("ok", Json::Bool(false)), ("diagnostics", diagnostics)]),
                    &inferred,
                ),
                text,
            },
            Examined::Proposed(proposal) => {
                let (explain, explained) = explain_report(&proposal.program, &proposal.tools, proposal.script.as_ref(), self.naming())?;
                let approved = proposal.verdict == Verdict::Approved;
                Reply {
                    is_error: !approved,
                    structured: with_inferred(
                        Json::object([
                            ("ok", Json::Bool(approved)),
                            ("explain", explain),
                            ("policy", proposal.verdict.json()),
                        ]),
                        &proposal.inferred,
                    ),
                    text: format!("{explained}\n{}", proposal.verdict.text()),
                }
            }
        })
    }

    fn values(&self, proposal: &Proposal, arguments: &Json) -> Result<Vec<(String, Value)>, Stop> {
        let pairs: Vec<String> = match arguments.get("inputs") {
            None | Some(Json::Null) => Vec::new(),
            Some(Json::Object(entries)) => entries
                .iter()
                .map(|(name, value)| {
                    dumps(value, Layout::Spaced).map(|text| format!("{name}={text}")).map_err(|e| Stop::Internal(e.0))
                })
                .collect::<Result<_, _>>()?,
            Some(_) => return Err(Stop::Usage("inputs must be an object of NAME to JSON value".to_string())),
        };
        match &proposal.script {
            Some(header) => script_input_values(&pairs, header),
            None => input_values(&pairs, &self.inputs),
        }
    }

    fn session(&self, program: &Program) -> Result<Option<Session>, Stop> {
        let Some(tools) = &self.mcp else {
            return Ok(None);
        };
        if !program.capabilities.iter().any(|id| tools.bindings.get(id).is_some()) {
            return Ok(None);
        }
        Session::open(tools.bindings.clone(), &tools.host.command, tools.host.limits)
            .map(Some)
            .map_err(|e| Stop::Usage(format!("cannot connect to the MCP server: {e}")))
    }

    fn run(&self, source: &str, arguments: &Json) -> Result<Reply, Stop> {
        let proposal = match self.examine(source)? {
            Examined::Rejected { diagnostics, text, inferred } => {
                return Ok(Reply {
                    is_error: true,
                    structured: with_inferred(
                        Json::object([("outcome", Json::string("rejected")), ("diagnostics", diagnostics)]),
                        &inferred,
                    ),
                    text: format!("{text}\nThe program does not check, so nothing ran."),
                });
            }
            Examined::Proposed(proposal) => *proposal,
        };
        if let Verdict::Refused(_) = &proposal.verdict {
            return Ok(Reply {
                is_error: true,
                structured: with_inferred(
                    Json::object([("outcome", Json::string("refused")), ("policy", proposal.verdict.json())]),
                    &proposal.inferred,
                ),
                text: format!("{}\nThe policy refused the program, so nothing ran.", proposal.verdict.text()),
            });
        }
        let values = self.values(&proposal, arguments)?;
        let session = self.session(&proposal.program)?;
        let missing = crate::native::unimplemented(&proposal.program, &self.declared, session.as_ref());
        if !missing.is_empty() {
            return Err(Stop::Usage(format!("no pack or MCP server implements {}, so nothing ran", missing.join(", "))));
        }
        let (outcome, machine) =
            crate::native::run(&proposal.program, &self.grants, &self.declared, session.as_ref(), &values, Budgets::default());
        let report = run_report(outcome, &machine)?;
        let calls = report.json.get("trace").map(|trace| calls_text(trace, self.naming())).unwrap_or_default();
        let logs = logs_text(&report.logs);
        Ok(Reply {
            is_error: !report.completed,
            structured: with_inferred(report.json, &proposal.inferred),
            text: format!("{}\n{logs}{calls}", report.text),
        })
    }

    fn dispatch(&self, tool: Tool, arguments: &Json) -> Json {
        if let Json::Object(fields) = arguments
            && let Some((name, _)) = fields.iter().find(|(name, _)| !tool.arguments(self.language).contains(&name.as_str()))
        {
            return failed_result(&format!("{} takes {}, not {name}", tool.name(), tool.arguments(self.language).join(" and ")));
        }
        let Some(source) = arguments.get("program").and_then(Json::as_str) else {
            return failed_result(&format!("{} needs a string program", tool.name()));
        };
        let replied = match tool {
            Tool::Check => self.check(source),
            Tool::Run => self.run(source, arguments),
        };
        match replied {
            Ok(reply) => tool_result(reply.is_error, &reply.text, reply.structured),
            Err(Stop::Usage(message)) => failed_result(&message),
            Err(Stop::Internal(message)) => failed_result(&format!("internal error: {message}")),
        }
    }
}

impl ToolHost for CodeServer {
    fn tools(&self) -> Vec<Json> {
        self.listing()
    }

    fn call(&self, params: &Json) -> Answer {
        let rejected = |message: String| Answer::Rejected { code: INVALID_PARAMS, message };
        let Some(name) = params.get("name").and_then(Json::as_str) else {
            return rejected("tools/call needs a string name".to_string());
        };
        let Some(tool) = Tool::from_name(name) else {
            return rejected(format!("unknown tool {name}; the tools are check and run"));
        };
        let none = Json::Object(Vec::new());
        let arguments = match params.get("arguments") {
            None | Some(Json::Null) => &none,
            Some(object @ Json::Object(_)) => object,
            Some(_) => return rejected("arguments must be an object".to_string()),
        };
        let performed = sx_kernel::stack::on_sized_stack(|| {
            catch_unwind(AssertUnwindSafe(|| self.dispatch(tool, arguments)))
                .unwrap_or_else(|_| failed_result(&format!("{name} panicked")))
        });
        Answer::Result(performed.unwrap_or_else(|error| failed_result(&format!("cannot start a worker: {error}"))))
    }
}

fn build(args: &[String], new_registry: &dyn Fn() -> Registry) -> Result<CodeServer, Exit> {
    let flags = ["--declarations", "--grants", "--allow", "--policy", "--timeout-ms", "--name", "--language"];
    let options = scan(args, &flags).map_err(|message| usage(&message))?;
    if let Some(extra) = options.positional.first() {
        return Err(usage(&format!("unrecognized argument {extra}")));
    }
    let Some(declarations_path) = options.last("--declarations") else {
        return Err(usage("sx mcp code needs --declarations FILE"));
    };
    let Some(policy_path) = options.last("--policy") else {
        return Err(usage("sx mcp code needs --policy FILE"));
    };
    let limits = options
        .timeout()
        .map_err(|message| usage(&message))?
        .map_or(sx_packs::mcp::Limits::STANDARD, |timeout| sx_packs::mcp::Limits { startup: timeout, call: timeout });
    let language = match options.last("--language") {
        None | Some("sx") => Language::Sx,
        Some("ts") => Language::TypeScript,
        Some(other) => return Err(usage(&format!("--language takes sx or ts, not {other}"))),
    };
    let (mut registry, declarations, declared) = load_registry(Some(declarations_path), new_registry()).map_err(stopped)?;
    if language == Language::TypeScript {
        add_typescript_cores(&mut registry).map_err(stopped)?;
    }
    let grants = grants_of(&options, &registry).map_err(stopped)?;
    let policy = read_json(policy_path, "policy")
        .and_then(|json| {
            Policy::load(&json, &registry, language.into())
                .map_err(|message| Stop::Usage(format!("invalid policy {}: {message}", path_text(policy_path))))
        })
        .map_err(stopped)?;
    let inputs = declared_inputs(&registry, &declarations).map_err(stopped)?;
    let name = options.last("--name").map(str::to_string);
    let bindings = Bindings::from_declarations(&registry, &declarations, name.as_deref());
    let mcp = match (&options.command, bindings) {
        (Some(_), Err(message)) => return Err(usage(&format!("invalid declarations: {message}"))),
        (Some(_), Ok(bindings)) => {
            let command = options.command().map_err(|message| usage(&message))?;
            Some(McpTools { bindings, host: Host { command, name, limits } })
        }
        (None, Ok(bindings)) => {
            if let Some(id) = bindings.ids().find(|id| grants.contains(id)) {
                return Err(usage(&format!("{id} is an MCP tool; start its server with -- COMMAND")));
            }
            None
        }
        (None, Err(_)) => None,
    };
    let guide = match language {
        Language::Sx => guide_text(&registry, &declarations, &grants),
        Language::TypeScript => typescript_guide(&registry, &declarations, &grants, true),
    };
    Ok(CodeServer { registry, declared, grants, inputs, policy, mcp, guide, language })
}

const TYPESCRIPT_RULES: &str = include_str!("../guide/typescript-rules.md");

pub(crate) fn typescript_guide(registry: &Registry, declarations: &Json, grants: &Grants, rules: bool) -> String {
    let declared = sx_ts::declarations::declarations(registry, &|id| grants.contains(id));
    let mut parts = vec![format!("## Your tools\n\n```ts\n{declared}\n```")];
    let notes = notes_text(registry, declarations, grants);
    if !notes.trim().is_empty() {
        parts.push(format!("## How the tools behave\n\n{}", sx_ts::terms::rewrite(notes.trim_end(), registry).replace("`none`", "`null`")));
    }
    if rules {
        parts.push(TYPESCRIPT_RULES.trim_end().to_string());
    }
    parts.join("\n\n")
}

pub(crate) fn serve(args: &[String], new_registry: &dyn Fn() -> Registry) -> Exit {
    let server = match build(args, new_registry) {
        Ok(server) => server,
        Err(exit) => return exit,
    };
    match serve_connection(&server, io::stdin().lock(), io::stdout()) {
        Ok(()) => Exit { code: EXIT_OK, stdout: String::new(), stderr: String::new() },
        Err(message) => Exit { code: EXIT_FAILED, stdout: String::new(), stderr: format!("sx mcp: {message}\n") },
    }
}
