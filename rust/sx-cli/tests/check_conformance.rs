use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use sx_cores::kernel::{Cores, declared_registry};
use sx_kernel::authority::{Authority, Grants};
use sx_kernel::budgets::Budgets;
use sx_kernel::checker::compile_source;
use sx_kernel::diagnostics::render;
use sx_kernel::export::export_program;
use sx_kernel::host::{FixtureHost, parse_result, run};
use sx_kernel::json::{Json, Layout, dumps, parse, python_equal};
use sx_kernel::machine::{Machine, Outcome, Response};
use sx_kernel::registry::{Registry, load_declarations};
use sx_kernel::types::Type;
use sx_kernel::values::{Value, decode};

mod common;
use common::{json_case_runs_in_python, source_runs_in_python};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_json(path: &Path) -> Json {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn shown(json: &Json) -> String {
    dumps(json, Layout::Spaced).expect("serializable")
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Spec {
    Task(String),
    Declarations(String),
    Help,
    Cores,
    Fuzz,
}

impl Spec {
    fn json(&self) -> Json {
        match self {
            Spec::Task(id) => Json::object([("kind", Json::string("task")), ("id", Json::string(id.as_str()))]),
            Spec::Declarations(path) => {
                Json::object([("kind", Json::string("declarations")), ("path", Json::string(path.as_str()))])
            }
            Spec::Help => Json::object([("kind", Json::string("help"))]),
            Spec::Cores => Json::object([("kind", Json::string("cores"))]),
            Spec::Fuzz => Json::object([("kind", Json::string("fuzz"))]),
        }
    }
}

struct Examples {
    help: Vec<String>,
    guide: Vec<String>,
    functions: Vec<String>,
    declarations: Json,
}

fn python_examples() -> &'static Examples {
    static EXAMPLES: OnceLock<Examples> = OnceLock::new();
    EXAMPLES.get_or_init(|| {
        let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("examples.json");
        let status = Command::new("uv")
            .args(["run", "--quiet", "python", "rust/scripts/check_corpus.py", "examples"])
            .arg(&target)
            .current_dir(root())
            .status()
            .expect("uv runs the Python corpus checker");
        assert!(status.success(), "rust/scripts/check_corpus.py examples failed");
        let json = read_json(&target);
        let strings = |key: &str| -> Vec<String> {
            json.get(key).and_then(Json::as_array).expect("example list").iter().filter_map(Json::as_str).map(str::to_string).collect()
        };
        let declarations = Command::new("uv")
            .args([
                "run",
                "--quiet",
                "python",
                "-c",
                "import json; from sx.help import EXAMPLE_DECLARATIONS as d; print(json.dumps(d))",
            ])
            .current_dir(root())
            .output()
            .expect("uv prints the help declarations");
        assert!(declarations.status.success());
        let declarations = parse(&String::from_utf8_lossy(&declarations.stdout)).expect("declarations JSON");
        Examples { help: strings("help"), guide: strings("guide"), functions: strings("functions"), declarations }
    })
}

fn registry_for(spec: &Spec) -> Registry {
    static CACHE: OnceLock<std::sync::Mutex<BTreeMap<Spec, Registry>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(found) = cache.lock().expect("registry cache").get(spec) {
        return found.clone();
    }
    let registry = match spec {
        Spec::Task(id) => {
            let task = read_json(&root().join("bench/codemode/tasks").join(id).join("task.json"));
            declared_registry(&task).unwrap_or_else(|e| panic!("task {id}: {e}"))
        }
        Spec::Declarations(path) => {
            let declarations = read_json(&root().join(path));
            let mut registry = Registry::new();
            for id in declarations.get("cores").and_then(Json::as_array).into_iter().flatten() {
                let core = id.as_str().and_then(sx_cores::kernel::core).unwrap_or_else(|| panic!("{path}: unknown core {id:?}"));
                registry.add_core(core).unwrap_or_else(|e| panic!("{path}: {e}"));
            }
            for id in declarations.get("packs").and_then(Json::as_array).into_iter().flatten() {
                let pack = id.as_str().and_then(sx_packs::pack).unwrap_or_else(|| panic!("{path}: unknown pack {id:?}"));
                registry.add_pack(pack).unwrap_or_else(|e| panic!("{path}: {e}"));
            }
            load_declarations(&mut registry, &declarations).unwrap_or_else(|e| panic!("{path}: {e}"));
            registry
        }
        Spec::Help => {
            let mut registry = Registry::new();
            load_declarations(&mut registry, &python_examples().declarations).expect("help declarations load");
            registry
        }
        Spec::Cores => {
            let mut registry = Registry::new();
            for core in sx_cores::kernel::cores() {
                registry.add_core(core).expect("core registers");
            }
            registry
        }
        Spec::Fuzz => {
            let mut registry = Registry::new();
            for path in [RUN_DECLARATIONS, "tests/fuzz/declarations.json"] {
                load_declarations(&mut registry, &read_json(&root().join(path))).expect("fuzz declarations load");
            }
            registry
        }
    };
    cache.lock().expect("registry cache").insert(spec.clone(), registry.clone());
    registry
}

#[derive(Clone)]
struct Case {
    name: String,
    source: String,
    spec: Spec,
    allowed: Option<Vec<String>>,
    grants: Option<String>,
    budgets: Vec<(String, u64)>,
}

impl Case {
    fn new(name: impl Into<String>, source: impl Into<String>, spec: Spec) -> Case {
        Case { name: name.into(), source: source.into(), spec, allowed: None, grants: None, budgets: Vec::new() }
    }

    fn json(&self) -> Json {
        let allowed = match &self.allowed {
            None => Json::Null,
            Some(ids) => Json::Array(ids.iter().map(|id| Json::string(id.as_str())).collect()),
        };
        let budgets = Json::Object(self.budgets.iter().map(|(k, v)| (k.clone(), Json::Int(*v as i64))).collect());
        Json::object([
            ("source", Json::string(self.source.as_str())),
            ("registry", self.spec.json()),
            ("allowed", allowed),
            ("budgets", budgets),
        ])
    }

    fn budgets(&self) -> Budgets {
        let mut budgets = Budgets::default();
        for (name, value) in &self.budgets {
            budgets.set(name, *value).expect("known budget");
        }
        budgets
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Checked {
    Rejected { codes: Vec<String>, rendered: String },
    Accepted { ir: String },
    Crashed(String),
}

fn python_checks(label: &str, cases: &[Case]) -> Vec<Checked> {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let manifest = directory.join(format!("{label}-manifest.json"));
    let target = directory.join(format!("{label}-python.json"));
    fs::write(&manifest, shown(&Json::Array(cases.iter().map(Case::json).collect()))).expect("manifest written");
    let status = Command::new("uv")
        .args(["run", "--quiet", "python", "rust/scripts/check_corpus.py"])
        .arg(&manifest)
        .arg(&target)
        .current_dir(root())
        .status()
        .expect("uv runs the Python corpus checker");
    assert!(status.success(), "rust/scripts/check_corpus.py failed");
    let results = read_json(&target);
    let results = results.as_array().expect("result list");
    assert_eq!(results.len(), cases.len());
    results
        .iter()
        .map(|result| {
            if let Some(ir) = result.get("ir").and_then(Json::as_str) {
                return Checked::Accepted { ir: ir.to_string() };
            }
            if let Some(crash) = result.get("crash").and_then(Json::as_str) {
                return Checked::Crashed(crash.to_string());
            }
            let codes = result.get("codes").and_then(Json::as_array).expect("codes");
            Checked::Rejected {
                codes: codes.iter().filter_map(Json::as_str).map(str::to_string).collect(),
                rendered: result.get("rendered").and_then(Json::as_str).expect("rendered").to_string(),
            }
        })
        .collect()
}

fn on_large_stack<R: Send>(work: impl FnOnce() -> R + Send) -> R {
    sx_kernel::stack::on_sized_stack(work).expect("the worker thread starts")
}

fn rust_check(case: &Case) -> (Checked, Option<sx_kernel::ir::Program>) {
    on_large_stack(|| rust_check_here(case))
}

fn grants_for(case: &Case, registry: &Registry) -> Grants {
    let mut grants = match &case.grants {
        Some(path) => Grants::load(registry, &read_json(&root().join(path))).unwrap_or_else(|e| panic!("{path}: {e}")),
        None => Grants::new(),
    };
    let allowed: Vec<String> = match &case.allowed {
        None if case.grants.is_some() => Vec::new(),
        None => registry.capabilities().filter(|c| c.scope.is_none()).map(|c| c.id.clone()).collect(),
        Some(ids) => ids.clone(),
    };
    for id in &allowed {
        grants.grant_unscoped(registry, id).unwrap_or_else(|e| panic!("{}: {e}", case.name));
    }
    grants
}

fn rust_check_here(case: &Case) -> (Checked, Option<sx_kernel::ir::Program>) {
    let registry = registry_for(&case.spec);
    let grants = grants_for(case, &registry);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        compile_source(&case.source, &registry, Authority::Granted(&grants), &BTreeMap::new(), case.budgets())
    }));
    match outcome {
        Err(panic) => {
            let message = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_default();
            (Checked::Crashed(format!("panic: {message}")), None)
        }
        Ok(result) => match result.program {
            Some(program) => (Checked::Accepted { ir: shown(&export_program(&program)) }, Some(program)),
            None => (
                Checked::Rejected {
                    codes: result.diagnostics.iter().map(|d| d.code.to_string()).collect(),
                    rendered: render(&case.source, &result.diagnostics),
                },
                None,
            ),
        },
    }
}

fn header_spec(source: &str) -> (Spec, Option<Vec<String>>, Option<String>) {
    let mut lines = source.split('\n');
    let first = lines.next().unwrap_or_default();
    let spec = match first.strip_prefix("; task: ") {
        Some(task) => Spec::Task(task.to_string()),
        None => Spec::Declarations(first.strip_prefix("; declarations: ").unwrap_or(RUN_DECLARATIONS).to_string()),
    };
    let mut allowed = None;
    let mut grants = None;
    for line in lines.take_while(|line| line.starts_with(';')) {
        if let Some(ids) = line.strip_prefix("; allow: ") {
            allowed = Some(ids.split_whitespace().map(str::to_string).collect());
        } else if let Some(path) = line.strip_prefix("; grants: ") {
            grants = Some(path.to_string());
        }
    }
    (spec, allowed, grants)
}

const RUN_DECLARATIONS: &str = "fixtures/run/declarations.json";

fn sx_files(directory: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(root().join(directory))
        .expect("fixture directory")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "sx"))
        .collect();
    found.sort();
    found
}

fn stem(path: &Path) -> String {
    path.file_stem().and_then(|s| s.to_str()).expect("file name").to_string()
}

fn check_fixture_cases() -> Vec<(Case, String)> {
    sx_files("fixtures/check")
        .into_iter()
        .map(|path| {
            let source = fs::read_to_string(&path).expect("fixture source");
            let expected = fs::read_to_string(path.with_extension("expected")).expect("expected output");
            let (spec, allowed, grants) = header_spec(&source);
            let mut case = Case::new(stem(&path), source, spec);
            case.allowed = allowed;
            case.grants = grants;
            (case, expected)
        })
        .collect()
}

#[test]
fn every_check_fixture_renders_byte_identically() {
    let cases = check_fixture_cases();
    assert!(!cases.is_empty());
    let mut failures = Vec::new();
    for (case, expected) in &cases {
        let rendered = match rust_check(case).0 {
            Checked::Rejected { rendered, .. } => rendered,
            Checked::Accepted { .. } => String::new(),
            Checked::Crashed(message) => format!("crashed: {message}"),
        };
        if &rendered != expected {
            failures.push(format!("{}:\n--- expected\n{expected}--- rust\n{rendered}", case.name));
        }
    }
    eprintln!("{} check fixtures compared", cases.len());
    assert!(failures.is_empty(), "{} of {} differ:\n{}", failures.len(), cases.len(), failures.join("\n"));
}

struct RunCase {
    case: Case,
    fixture: Json,
}

fn run_cases() -> Vec<RunCase> {
    let mut paths: Vec<PathBuf> = fs::read_dir(root().join("fixtures/run"))
        .expect("fixtures/run")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "json") && !path.ends_with("declarations.json"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let fixture = read_json(&path);
            let declarations = fixture.get("declarations").and_then(Json::as_str).expect("declarations");
            let source = fixture.get("program").and_then(Json::as_str).expect("program");
            let mut case = Case::new(stem(&path), source, Spec::Declarations(format!("fixtures/run/{declarations}")));
            case.allowed = Some(
                fixture.get("allow").and_then(Json::as_array).expect("allow").iter().filter_map(Json::as_str).map(str::to_string).collect(),
            );
            if let Some(overrides) = fixture.get("budgets").and_then(Json::as_object) {
                case.budgets = overrides
                    .iter()
                    .map(|(k, v)| (k.clone(), v.as_i64().and_then(|v| u64::try_from(v).ok()).expect("budget count")))
                    .collect();
            }
            RunCase { case, fixture }
        })
        .collect()
}

fn expectation(outcome: &Outcome) -> Json {
    match outcome {
        Outcome::Completed { json, .. } => Json::object([("completed", json.clone())]),
        Outcome::Failed(failure) => Json::object([("failed", Json::string(failure.kind.name()))]),
        Outcome::Suspended(requests) => {
            Json::object([("suspended", Json::Array(requests.iter().map(|r| Json::string(r.id())).collect()))])
        }
    }
}

fn fixture_inputs(program: &sx_kernel::ir::Program, fixture: &Json) -> Result<Vec<(String, Value)>, String> {
    let given = fixture.get("inputs").and_then(Json::as_object).unwrap_or_default();
    if let Some((name, _)) = given.iter().find(|(name, _)| !program.inputs.contains_key(name)) {
        return Err(format!("{name} is not an input of the script"));
    }
    let mut values = Vec::new();
    for (name, declared) in &program.inputs {
        let value = match given.iter().find(|(held, _)| held == name) {
            Some((_, json)) => decode(json, declared, &format!("${name}")).map_err(|e| e.to_string())?,
            None if matches!(declared, Type::Option(_)) => Value::None,
            None => return Err(format!("the script needs a value for input {name}")),
        };
        values.push((name.clone(), value));
    }
    Ok(values)
}

fn run_rust(program: &sx_kernel::ir::Program, case: &RunCase) -> Result<(), String> {
    let fixture = &case.fixture;
    let budgets = case.case.budgets();
    let cores = Cores;
    let inputs = fixture_inputs(program, fixture)?;
    let (outcome, trace) = if let Some(steps) = fixture.get("steps").and_then(Json::as_array) {
        let mut machine = Machine::new(program, &inputs, budgets, &cores);
        let mut outcome = machine.start();
        for step in steps {
            let Outcome::Suspended(requests) = &outcome else {
                return Err(format!("expected a suspension, got {}", shown(&expectation(&outcome))));
            };
            let ids: Vec<&str> = requests.iter().map(|r| r.id()).collect();
            let wanted: Vec<&str> =
                step.get("requests").and_then(Json::as_array).expect("requests").iter().filter_map(Json::as_str).collect();
            if ids != wanted {
                return Err(format!("suspended with {ids:?}, expected {wanted:?}"));
            }
            let mut responses = Vec::new();
            for pair in step.get("respond").and_then(Json::as_array).expect("respond") {
                let [id, entry] = pair.as_array().expect("response pair") else {
                    return Err("a response is not a pair".to_string());
                };
                let response: Response = parse_result(entry).map_err(|e| e.to_string())?.response();
                responses.push((id.as_str().expect("response id").to_string(), response));
            }
            outcome = machine.resume(responses);
        }
        (outcome, machine.trace().to_vec())
    } else {
        let responses = fixture.get("responses").cloned().unwrap_or(Json::Array(Vec::new()));
        let mut host =
            FixtureHost::from_responses(&program.signatures.capabilities, &responses).map_err(|e| e.to_string())?;
        let registry = registry_for(&case.case.spec);
        let grants = grants_for(&case.case, &registry);
        let finished = run(program, &grants, &mut host, &inputs, budgets, &cores).map_err(|e| format!("unexpected call: {e}"))?;
        let trace = finished.machine.trace().to_vec();
        (finished.outcome, trace)
    };
    let expected = fixture.get("expect").expect("expect");
    let actual = expectation(&outcome);
    if !python_equal(&actual, expected) {
        return Err(format!("outcome {} != {}", shown(&actual), shown(expected)));
    }
    let wanted = fixture.get("trace").cloned().unwrap_or(Json::Array(Vec::new()));
    let actual = Json::Array(trace);
    if !python_equal(&actual, &wanted) {
        return Err(format!("trace {} != {}", shown(&actual), shown(&wanted)));
    }
    Ok(())
}

fn invalid_input(fixture: &Json) -> Option<&str> {
    fixture.get("expect").and_then(|e| e.get("invalid-input")).and_then(Json::as_str)
}

fn judge_alone(case: &RunCase) -> Result<(), String> {
    let (rust, program) = rust_check(&case.case);
    let wanted = case.fixture.get("expect").and_then(|e| e.get("rejected"));
    match (&rust, program) {
        (Checked::Accepted { .. }, Some(program)) if wanted.is_none() && invalid_input(&case.fixture).is_some() => {
            match (fixture_inputs(&program, &case.fixture), invalid_input(&case.fixture)) {
                (Err(message), Some(wanted)) if message.contains(wanted) => Ok(()),
                (other, _) => Err(format!("the inputs gave {other:?}, but the fixture expects an invalid input")),
            }
        }
        (Checked::Accepted { .. }, Some(program)) if wanted.is_none() => run_rust(&program, case),
        (Checked::Rejected { codes, .. }, _) => {
            let listed = Json::Array(codes.iter().map(|c| Json::string(c.as_str())).collect());
            if wanted.is_some_and(|w| python_equal(w, &listed)) {
                Ok(())
            } else {
                Err(format!("codes {codes:?} are not the fixture's"))
            }
        }
        _ => Err(format!("rust {rust:?} but the fixture expects {:?}", case.fixture.get("expect").map(shown))),
    }
}

#[test]
fn every_run_fixture_checks_exports_and_runs_as_python_does() {
    let (cases, alone): (Vec<RunCase>, Vec<RunCase>) =
        run_cases().into_iter().partition(|c| json_case_runs_in_python(&c.fixture));
    let plain: Vec<Case> = cases.iter().map(|c| c.case.clone()).collect();
    let python = python_checks("run", &plain);
    let mut failures = Vec::new();
    let (mut accepted, mut rejected) = (0, 0);
    for (case, python) in cases.iter().zip(&python) {
        let (rust, program) = rust_check(&case.case);
        let name = &case.case.name;
        match (&rust, python, program) {
            (Checked::Accepted { ir }, Checked::Accepted { ir: theirs }, Some(program)) => {
                accepted += 1;
                if ir != theirs {
                    failures.push(format!("{name}: IR differs\n rust:   {ir}\n python: {theirs}"));
                    continue;
                }
                if let Err(problem) = run_rust(&program, case) {
                    failures.push(format!("{name}: {problem}"));
                }
            }
            (Checked::Rejected { codes, .. }, Checked::Rejected { codes: theirs, .. }, _) => {
                rejected += 1;
                if codes != theirs {
                    failures.push(format!("{name}: codes {codes:?} != Python's {theirs:?}"));
                }
                let wanted = case.fixture.get("expect").and_then(|e| e.get("rejected"));
                let listed = Json::Array(codes.iter().map(|c| Json::string(c.as_str())).collect());
                if !wanted.is_some_and(|w| python_equal(w, &listed)) {
                    failures.push(format!("{name}: codes {codes:?} are not the fixture's"));
                }
            }
            _ => failures.push(format!("{name}: rust {rust:?} but python {python:?}")),
        }
    }
    for case in &alone {
        if let Err(problem) = judge_alone(case) {
            failures.push(format!("{}: {problem}", case.case.name));
        }
    }
    eprintln!(
        "{accepted} accepted and {rejected} rejected run fixtures compared, {} marked python: false judged by their own expectations",
        alone.len()
    );
    assert!(failures.is_empty(), "{} of {} differ:\n{}", failures.len(), cases.len() + alone.len(), failures.join("\n"));
}

#[test]
fn only_a_leading_comment_line_marks_a_source_as_python_free() {
    assert!(!source_runs_in_python("; python: no\n1"));
    assert!(!source_runs_in_python("; task: t\n; allow: a.b\n; python: no\n(+ 1 2)"));
    assert!(source_runs_in_python("; task: t\n(+ 1 2)\n; python: no"));
    assert!(source_runs_in_python("; task: t\n1"));
    assert!(source_runs_in_python("; python: nope\n1"));
}

const PYTHON_CRASHES: [&str; 1] = ["RecursionError"];

#[derive(Default)]
struct Tally {
    compared: usize,
    accepted: usize,
    rejected: usize,
    widened: usize,
    python_crashes: Vec<String>,
    codes: BTreeSet<String>,
}

const RUST_ONLY_HINTS: [&str; 1] =
    [" To call a tool only when a test holds, write (when test (call ...)), which is (some result) or none."];

fn as_python_renders(checked: Checked) -> Checked {
    match checked {
        Checked::Rejected { codes, rendered } => {
            let rendered = RUST_ONLY_HINTS.iter().fold(rendered, |text, hint| text.replace(hint, ""));
            Checked::Rejected { codes, rendered }
        }
        other => other,
    }
}

fn is_frozen_refusal(codes: &[String], rendered: &str) -> bool {
    codes == ["multiple-expressions"] || rendered.contains("is always present, so get takes no default for it")
}

fn is_unfilled_accumulator(codes: &[String], source: &str) -> bool {
    source.contains("(loop") && codes.iter().any(|code| code == "unresolved-type")
}

fn is_capitalized_binding(rendered: &str) -> bool {
    rendered.lines().any(|line| {
        line.strip_prefix("  ").is_some_and(|text| {
            text.starts_with(|c: char| c.is_ascii_uppercase()) && text.contains(" is not a pattern. A pattern is a name")
        })
    })
}

const FORM_NAME_AS_VALUE: &str = " is the name of a form, so it cannot name a value.";

fn blocks(rendered: &str) -> Vec<&str> {
    rendered.split("\n\n").map(str::trim_end).filter(|block| !block.is_empty()).collect()
}

fn message_line(block: &str) -> &str {
    block.lines().nth(3).unwrap_or("")
}

fn is_form_name_as_value(block: &str) -> bool {
    block.starts_with("error[reserved-name]") && message_line(block).ends_with(FORM_NAME_AS_VALUE) && !message_line(block).starts_with("  none ")
}

fn as_python_reserved_name(block: &str) -> String {
    if !block.starts_with("error[reserved-name]") {
        return block.to_string();
    }
    let lines: Vec<&str> = block.lines().collect();
    let message = lines[3];
    let reserved = if let Some(name) = message.strip_suffix(" so it cannot name a function.").and_then(|m| m.strip_suffix(" is the name of a form,")) {
        format!("{name}{FORM_NAME_AS_VALUE}")
    } else if message.starts_with("  none is the absent value, so it cannot name a") {
        format!("  none{FORM_NAME_AS_VALUE}")
    } else {
        return block.to_string();
    };
    let mut kept: Vec<String> = lines[..3].iter().map(|line| (*line).to_string()).collect();
    kept.push(reserved);
    kept.extend(lines[5..].iter().map(|line| (*line).to_string()));
    kept.join("\n")
}

fn is_form_name_widening(rust: &Checked, python_rendered: &str) -> bool {
    let theirs = blocks(python_rendered);
    let ours: Vec<String> = match rust {
        Checked::Accepted { .. } => Vec::new(),
        Checked::Rejected { rendered, .. } => blocks(rendered).into_iter().map(as_python_reserved_name).collect(),
        Checked::Crashed(_) => return false,
    };
    let kept: Vec<&str> = theirs
        .iter()
        .copied()
        .filter(|block| !is_form_name_as_value(block) || ours.iter().any(|ours| ours == block))
        .collect();
    ours.iter().map(String::as_str).eq(kept.iter().copied())
}

fn compare(label: &str, cases: &[Case]) -> (Tally, Vec<String>) {
    let python = python_checks(label, cases);
    let mut failures = Vec::new();
    let mut tally = Tally { compared: cases.len(), ..Tally::default() };
    for (case, python) in cases.iter().zip(&python) {
        let (rust, _) = rust_check(case);
        let rust = as_python_renders(rust);
        match (&rust, python) {
            (Checked::Crashed(message), _) => {
                failures.push(format!("{}: Rust crashed: {message}\n{}", case.name, case.source));
            }
            (Checked::Rejected { .. }, Checked::Crashed(crash)) if PYTHON_CRASHES.contains(&crash.as_str()) => {
                tally.python_crashes.push(format!("{}: Python {crash}: {}", case.name, case.source));
            }
            (_, Checked::Rejected { codes, rendered }) if is_frozen_refusal(codes, rendered) => tally.widened += 1,
            (_, Checked::Rejected { codes, .. }) if &rust != python && is_unfilled_accumulator(codes, &case.source) => {
                tally.widened += 1;
            }
            (_, Checked::Rejected { rendered, .. }) if &rust != python && is_capitalized_binding(rendered) => {
                tally.widened += 1;
            }
            (_, Checked::Rejected { rendered, .. }) if &rust != python && is_form_name_widening(&rust, rendered) => {
                tally.widened += 1;
            }
            _ if &rust != python => {
                failures.push(format!("{}:\n{}\n--- rust\n{rust:?}\n--- python\n{python:?}", case.name, case.source));
            }
            (Checked::Accepted { .. }, _) => tally.accepted += 1,
            (Checked::Rejected { codes, .. }, _) => {
                tally.rejected += 1;
                tally.codes.extend(codes.iter().cloned());
            }
        }
    }
    (tally, failures)
}

fn example_cases() -> Vec<Case> {
    let examples = python_examples();
    let mut cases = Vec::new();
    for (index, source) in examples.help.iter().enumerate() {
        cases.push(Case::new(format!("help-{index}"), source.as_str(), Spec::Help));
    }
    for (index, source) in examples.guide.iter().enumerate() {
        cases.push(Case::new(format!("guide-{index}"), source.as_str(), Spec::Help));
    }
    for (index, source) in examples.functions.iter().enumerate() {
        cases.push(Case::new(format!("function-{index}"), source.as_str(), Spec::Cores));
    }
    cases
}

#[test]
fn every_help_and_guide_example_checks_as_python_does() {
    let cases = example_cases();
    let examples = python_examples();
    assert!(!examples.help.is_empty() && examples.guide.len() == 3 && !examples.functions.is_empty());
    let (tally, failures) = compare("examples", &cases);
    eprintln!("{} help, guide, and core examples compared, {} accepted", tally.compared, tally.accepted);
    assert!(failures.is_empty(), "{} of {} differ:\n{}", failures.len(), tally.compared, failures.join("\n\n"));
    assert!(tally.python_crashes.is_empty());
}

struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Token {
    Atom,
    Open,
    Close,
}

fn tokens(source: &str) -> Vec<(usize, usize, Token)> {
    let bytes = source.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let b = bytes[at];
        match b {
            b';' => {
                while at < bytes.len() && bytes[at] != b'\n' {
                    at += 1;
                }
            }
            b' ' | b'\t' | b'\n' | b'\r' | b',' => at += 1,
            b'(' | b'[' | b'{' => {
                found.push((at, at + 1, Token::Open));
                at += 1;
            }
            b')' | b']' | b'}' => {
                found.push((at, at + 1, Token::Close));
                at += 1;
            }
            b'"' => {
                let start = at;
                at += 1;
                while at < bytes.len() && bytes[at] != b'"' {
                    at += if bytes[at] == b'\\' { 2 } else { 1 };
                }
                at = (at + 1).min(bytes.len());
                found.push((start, at, Token::Atom));
            }
            _ => {
                let start = at;
                while at < bytes.len() && !b" \t\n\r,()[]{}\";".contains(&bytes[at]) {
                    at += 1;
                }
                found.push((start, at, Token::Atom));
            }
        }
    }
    found
}

fn group_end(found: &[(usize, usize, Token)], open: usize) -> Option<usize> {
    let mut depth = 0i64;
    for (index, (_, _, kind)) in found.iter().enumerate().skip(open) {
        match kind {
            Token::Open => depth += 1,
            Token::Close => depth -= 1,
            Token::Atom => {}
        }
        if depth == 0 {
            return Some(index);
        }
    }
    None
}

const VOCABULARY: &[&str] = &[
    "0", "1", "-1", "2", "1.5", "\"a\"", "\"Passing\"", "true", "false", "none", "_", ":name", ":else", "x", "xs",
    "count", "map", "filter", "reduce", "get", "let", "if", "match", "fn", "str", "+", "-", "=", "<", "first",
    "nth", "some", "ok", "err", "attempt", "retry", "loop", "recur", "par", "par-map", "keep", "sort-by", "assoc",
    "into-map", "concat", "contains?", "Timeout", "Failing", "Missing", "1s", "[]", "{}", "(some 1)", "[1 \"a\"]",
    "{:a 1}", "(fn [x] x)", "text.trim", "semver.parse",
];

fn mutate(source: &str, random: &mut Random) -> String {
    let found = tokens(source);
    if found.is_empty() {
        return source.to_string();
    }
    let atoms: Vec<usize> = (0..found.len()).filter(|&i| found[i].2 == Token::Atom).collect();
    let opens: Vec<usize> = (0..found.len()).filter(|&i| found[i].2 == Token::Open).collect();
    let text = |i: usize| &source[found[i].0..found[i].1];
    let splice = |start: usize, end: usize, with: &str| format!("{}{with}{}", &source[..start], &source[end..]);
    let pick_atom = |random: &mut Random| atoms.get(random.below(atoms.len())).copied();
    let pick_group = |random: &mut Random| {
        let open = *opens.get(random.below(opens.len()))?;
        Some((open, group_end(&found, open)?))
    };
    for _ in 0..8 {
        let mutated = match random.below(13) {
            0 => pick_atom(random).zip(pick_atom(random)).map(|(a, b)| splice(found[a].0, found[a].1, text(b))),
            1 | 2 => pick_atom(random).map(|a| splice(found[a].0, found[a].1, VOCABULARY[random.below(VOCABULARY.len())])),
            3 => pick_atom(random).map(|a| splice(found[a].0, found[a].1, "")),
            4 => pick_atom(random).map(|a| splice(found[a].0, found[a].1, &format!("{} {}", text(a), text(a)))),
            5 => pick_atom(random).zip(pick_atom(random)).filter(|(a, b)| a < b).map(|(a, b)| {
                format!(
                    "{}{}{}{}{}",
                    &source[..found[a].0],
                    text(b),
                    &source[found[a].1..found[b].0],
                    text(a),
                    &source[found[b].1..]
                )
            }),
            6 => pick_group(random).map(|(open, close)| splice(found[open].0, found[close].1, "")),
            7 => pick_group(random).map(|(open, close)| {
                let group = &source[found[open].0..found[close].1];
                splice(found[open].0, found[close].1, &format!("{group} {group}"))
            }),
            8 => pick_group(random).map(|(open, close)| {
                let group = &source[found[open].0..found[close].1];
                let wrapper = ["(some ", "[", "(attempt ", "(first ", "(count "][random.below(5)];
                let closer = if wrapper == "[" { "]" } else { ")" };
                splice(found[open].0, found[close].1, &format!("{wrapper}{group}{closer}"))
            }),
            9 => pick_atom(random).map(|a| splice(found[a].0, found[a].1, &format!("({})", text(a)))),
            10 | 11 => pick_group(random).zip(pick_group(random)).filter(|((a, _), (b, _))| a != b).map(|((a, a_end), (b, b_end))| {
                splice(found[a].0, found[a_end].1, &source[found[b].0..found[b_end].1])
            }),
            _ => {
                let at = found[random.below(found.len())];
                let replacement = ["(", ")", "[", "]", "{", "}", ""][random.below(7)];
                Some(splice(at.0, at.1, replacement))
            }
        };
        if let Some(mutated) = mutated.filter(|m| m != source) {
            return mutated;
        }
    }
    source.to_string()
}

const MUTANTS: usize = 25;

fn corpus() -> Vec<Case> {
    let mut seeds: Vec<Case> =
        check_fixture_cases().into_iter().map(|(case, _)| case).filter(|case| source_runs_in_python(&case.source)).collect();
    for path in sx_files("fixtures/read") {
        let source = fs::read_to_string(&path).expect("read fixture");
        if !source_runs_in_python(&source) {
            continue;
        }
        seeds.push(Case::new(format!("read-{}", stem(&path)), source, Spec::Declarations("fixtures/run/declarations.json".to_string())));
    }
    for path in sx_files("fixtures/cli") {
        let source = fs::read_to_string(&path).expect("cli fixture");
        seeds.push(Case::new(format!("cli-{}", stem(&path)), source, Spec::Declarations("fixtures/cli/declarations.json".to_string())));
    }
    seeds.extend(run_cases().into_iter().filter(|c| json_case_runs_in_python(&c.fixture)).map(|c| c.case));
    seeds.extend(example_cases());
    let declarations = Spec::Declarations("fixtures/run/declarations.json".to_string());
    for depth in [100, 127, 128, 129, 300, 2_000, 20_000] {
        let nested = format!("{}true{}", "(not ".repeat(depth), ")".repeat(depth));
        seeds.push(Case::new(format!("nested-not-{depth}"), nested, declarations.clone()));
        let brackets = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        seeds.push(Case::new(format!("nested-list-{depth}"), brackets, declarations.clone()));
        let lets = format!("{}x{}", "(let [x 1] ".repeat(depth), ")".repeat(depth));
        seeds.push(Case::new(format!("nested-let-{depth}"), lets, declarations.clone()));
    }
    let mut random = Random(0x5eed_5eed_0000_0001);
    let mut cases = Vec::new();
    for seed in &seeds {
        cases.push(seed.clone());
        let mut tight = seed.clone();
        tight.name = format!("{}~tight", seed.name);
        tight.budgets = [("nodes", 30), ("depth", 5), ("instantiations", 2), ("concurrency", 2), ("source_bytes", 200)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        cases.push(tight);
        for index in 0..MUTANTS {
            let mut source = mutate(&seed.source, &mut random);
            if random.below(3) == 0 {
                source = mutate(&source, &mut random);
            }
            let mut mutant = seed.clone();
            mutant.name = format!("{}~{index}", seed.name);
            mutant.source = source;
            cases.push(mutant);
        }
    }
    cases
}

fn generated_cases() -> Vec<Case> {
    let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("generated.json");
    let status = Command::new("uv")
        .args(["run", "--quiet", "python", "rust/scripts/check_corpus.py", "generated"])
        .arg(&target)
        .current_dir(root())
        .status()
        .expect("uv runs the Python corpus generator");
    assert!(status.success(), "rust/scripts/check_corpus.py generated failed");
    let generated = read_json(&target);
    generated
        .as_array()
        .expect("generated programs")
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let source = entry.get("source").and_then(Json::as_str).expect("source");
            let mut case = Case::new(format!("generated-{index}"), source, Spec::Fuzz);
            case.allowed = Some(
                entry.get("allowed").and_then(Json::as_array).expect("allowed").iter().filter_map(Json::as_str).map(str::to_string).collect(),
            );
            case.budgets = entry
                .get("budgets")
                .and_then(Json::as_object)
                .expect("budgets")
                .iter()
                .map(|(k, v)| (k.clone(), v.as_i64().and_then(|v| u64::try_from(v).ok()).expect("budget count")))
                .collect();
            case
        })
        .collect()
}

#[test]
fn differential_corpus_checks_identically_in_rust_and_python() {
    let mut cases = corpus();
    cases.extend(generated_cases());
    let (tally, failures) = compare("differential", &cases);
    eprintln!(
        "{} differential programs compared: {} accepted, {} rejected with {} distinct codes, {} programs that only Rust accepts as written, {} where Python crashes",
        tally.compared,
        tally.accepted,
        tally.rejected,
        tally.codes.len(),
        tally.widened,
        tally.python_crashes.len()
    );
    for crash in &tally.python_crashes {
        eprintln!("  {crash}");
    }
    assert!(failures.is_empty(), "{} of {} differ:\n{}", failures.len(), tally.compared, failures.join("\n\n"));
}

const BAD_DECLARATIONS: &[&str] = &[
    r#"{"capabilities": [{"effect": "read", "input": "Unit", "output": "Unit", "description": "x"}]}"#,
    r#"{"capabilities": [{"id": "a.b", "effect": "poke", "input": "Unit", "output": "Unit", "description": "x"}]}"#,
    r#"{"capabilities": [{"id": "a.b", "effect": 3, "input": "Unit", "output": "Unit", "description": "x"}]}"#,
    r#"{"capabilities": [{"id": "a.b", "effect": "read", "output": "Unit", "description": "x"}]}"#,
    r#"{"capabilities": [{"id": "a.b", "effect": "read", "input": "Unit", "output": "Unit"}]}"#,
    r#"{"capabilities": [{"id": "ab", "effect": "read", "input": "Unit", "output": "Unit", "description": "x"}]}"#,
    r#"{"capabilities": [{"id": "A.b", "effect": "read", "input": "Unit", "output": "Unit", "description": "x"}]}"#,
    r#"{"capabilities": [{"id": "a.b-", "effect": "read", "input": "Unit", "output": "Unit", "description": "x"}]}"#,
    r#"{"capabilities": [{"id": "a.b?", "effect": "read", "input": "Unit", "output": "Unit", "description": " "}]}"#,
    r#"{"capabilities": [{"id": "a.b", "effect": "read", "idempotent": true, "input": "Unit", "output": "Unit", "description": "x"}]}"#,
    r#"{"capabilities": [{"id": "a.b", "effect": "write", "input": "Unit", "output": "Unit", "description": "x"}, {"id": "a.b", "effect": "read", "input": "Unit", "output": "Unit", "description": "y"}]}"#,
    r#"{"capabilities": [{"id": "a.b", "effect": "read", "input": "Map<Int, String>", "output": "Unit", "description": "x"}]}"#,
    r#"{"capabilities": [{"id": "a.b", "effect": "read", "input": "Record {a: Int, a: Int}", "output": "Unit", "description": "x"}]}"#,
    r#"{"capabilities": [{"id": "a.b", "effect": "read", "input": "List<Int", "output": "Unit", "description": "x"}]}"#,
    r#"{"capabilities": [{"id": "a.b", "effect": "read", "input": "Option<Int, Int>", "output": "Unit", "description": "x"}]}"#,
    r#"{"capabilities": [{"id": "a.b", "effect": "read", "input": "Thing", "output": "Unit", "description": "x"}]}"#,
    r#"{"capabilities": [{"id": "a.b", "effect": "read", "input": "Int Int", "output": "Unit", "description": "x"}]}"#,
    r#"{"types": {"A": "B", "B": "C"}}"#,
    r#"{"types": {"A": "Record {b: B}", "B": "Record {a: A}"}}"#,
    r#"{"types": {"A": "Record {x: Int}", "B": "List<A>", "C": "Nope"}}"#,
    r#"{"enums": {"E": {"X": "Missing"}}}"#,
    r#"{"enums": {"E": {"X": null}}, "types": {"E": "Int"}}"#,
    r#"{"enums": {"E": {"X": null}, "F": {"Y": "E"}}, "types": {"T": "Record {e: F}"}, "capabilities": [{"id": "a.b", "effect": "read", "input": "T", "output": "Unit", "description": "fine"}]}"#,
];

#[test]
fn bad_declarations_fail_with_python_s_error_text() {
    let declarations: Vec<Json> = BAD_DECLARATIONS.iter().map(|text| parse(text).expect("declarations JSON")).collect();
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let manifest = directory.join("bad-declarations.json");
    let target = directory.join("bad-declarations-python.json");
    fs::write(&manifest, shown(&Json::Array(declarations.clone()))).expect("manifest written");
    let status = Command::new("uv")
        .args(["run", "--quiet", "python", "rust/scripts/check_corpus.py"])
        .arg(&manifest)
        .arg(&target)
        .current_dir(root())
        .status()
        .expect("uv runs the Python corpus checker");
    assert!(status.success());
    let python = read_json(&target);
    let mut failures = Vec::new();
    for (declaration, expected) in declarations.iter().zip(python.as_array().expect("errors")) {
        let rust = load_declarations(&mut Registry::new(), declaration).err().map(|e| e.to_string());
        let expected = expected.as_str().map(|text| text.split_once(": ").map_or(text, |(_, message)| message).to_string());
        if rust != expected {
            failures.push(format!("{}: rust {rust:?} != python {expected:?}", shown(declaration)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
