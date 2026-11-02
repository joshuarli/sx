use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::OnceLock;
use std::thread;

use sx_kernel::host::FixtureHost;
use sx_kernel::ir::CapabilitySignature;
use sx_kernel::json::{Json, Layout, dumps, parse, python_equal};
use sx_kernel::machine::Request;
use sx_cores::kernel::declared_registry;

mod common;
use common::json_case_runs_in_python;

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

fn python_outputs() -> &'static (PathBuf, PathBuf) {
    static OUTPUTS: OnceLock<(PathBuf, PathBuf)> = OnceLock::new();
    OUTPUTS.get_or_init(|| {
        let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
        let run = |script: &str, directory: &str| {
            let target = base.join(directory);
            let status = Command::new("uv")
                .args(["run", "--quiet", "python"])
                .arg(format!("rust/scripts/{script}"))
                .arg(&target)
                .current_dir(root())
                .status()
                .expect("uv runs the Python exporter");
            assert!(status.success(), "rust/scripts/{script} failed");
            target
        };
        (run("export_fixtures.py", "serve-outcomes"), run("explain_fixtures.py", "serve-explain"))
    })
}

struct Kernel {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl Kernel {
    fn spawn() -> Kernel {
        let mut child = Command::new(env!("CARGO_BIN_EXE_sx"))
            .arg("serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("sx serve starts");
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
        Kernel { child, stdin, stdout }
    }

    fn send_bytes(&mut self, bytes: &[u8]) {
        let stdin = self.stdin.as_mut().expect("stdin is open");
        stdin.write_all(bytes).expect("kernel reads its input");
        stdin.write_all(b"\n").expect("kernel reads its input");
        stdin.flush().expect("kernel reads its input");
    }

    fn send(&mut self, message: &Json) {
        self.send_bytes(shown(message).as_bytes());
    }

    fn receive(&mut self) -> Json {
        let mut line = String::new();
        let read = self.stdout.read_line(&mut line).expect("kernel answers");
        assert!(read > 0, "the kernel closed its output while a reply was expected");
        parse(&line).unwrap_or_else(|e| panic!("reply is not JSON ({e}): {line}"))
    }

    fn finish(mut self) -> ExitStatus {
        drop(self.stdin.take());
        let mut rest = String::new();
        self.stdout.read_line(&mut rest).expect("kernel closes its output");
        assert!(rest.is_empty(), "the kernel said more than expected: {rest}");
        self.child.wait().expect("kernel exits")
    }
}

impl Drop for Kernel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn kind(reply: &Json) -> &str {
    reply.get("type").and_then(Json::as_str).expect("reply has a type")
}

fn cases() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(root().join("fixtures/run"))
        .expect("fixtures/run exists")
        .map(|entry| entry.expect("readable entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "json") && !path.ends_with("declarations.json"))
        .collect();
    found.sort();
    found
}

fn signatures(declarations: &Json) -> BTreeMap<String, CapabilitySignature> {
    let registry = declared_registry(declarations).expect("declarations load");
    registry
        .capabilities()
        .map(|c| {
            let signature = CapabilitySignature {
                effect: c.effect,
                idempotent: c.idempotent,
                input: c.input.clone(),
                output: c.output.clone(),
            };
            (c.id.clone(), signature)
        })
        .collect()
}

fn start_message(case: &Json, declarations: &Json) -> Json {
    let mut fields = vec![
        ("version", Json::Int(1)),
        ("type", Json::string("start")),
        ("program", case.get("program").cloned().expect("program")),
        ("declarations", declarations.clone()),
        ("allow", case.get("allow").cloned().expect("allow")),
    ];
    if let Some(budgets) = case.get("budgets") {
        fields.push(("budgets", budgets.clone()));
    }
    if let Some(inputs) = case.get("inputs") {
        fields.push(("inputs", inputs.clone()));
    }
    Json::object(fields)
}

fn respond(responses: Vec<Json>) -> Json {
    Json::object([("type", Json::string("respond")), ("responses", Json::Array(responses))])
}

fn with_id(id: &str, entry: &Json) -> Json {
    let mut fields = vec![("id".to_string(), Json::string(id))];
    fields.extend(entry.as_object().expect("a result is an object").iter().cloned());
    Json::Object(fields)
}

fn request_of(json: &Json) -> Request {
    let id = json.get("id").and_then(Json::as_str).expect("request id").to_string();
    match json.get("kind").and_then(Json::as_str) {
        Some("call") => Request::Call {
            id,
            capability: json.get("capability").and_then(Json::as_str).expect("capability").to_string(),
            input: json.get("input").cloned().expect("input"),
        },
        Some("delay") => Request::Delay { id, milliseconds: json.get("milliseconds").and_then(Json::as_i64).expect("ms") },
        other => panic!("unknown request kind {other:?}"),
    }
}

fn drive(kernel: &mut Kernel, case: &Json, declarations: &Json) -> Result<Json, String> {
    kernel.send(&start_message(case, declarations));
    let capabilities = signatures(declarations);
    let responses = case.get("responses").cloned().unwrap_or(Json::Array(Vec::new()));
    let mut host = FixtureHost::from_responses(&capabilities, &responses).map_err(|e| e.to_string())?;
    let steps = case.get("steps").and_then(Json::as_array);
    let mut step = 0;
    loop {
        let reply = kernel.receive();
        if kind(&reply) != "requests" {
            return Ok(reply);
        }
        let listed = reply.get("requests").and_then(Json::as_array).expect("requests list");
        let requests: Vec<Request> = listed.iter().map(request_of).collect();
        if let Some(steps) = steps {
            let ids: Vec<&str> = requests.iter().map(Request::id).collect();
            let Some(current) = steps.get(step) else {
                kernel.send(&Json::object([("type", Json::string("cancel"))]));
                continue;
            };
            step += 1;
            let wanted: Vec<&str> = current
                .get("requests")
                .and_then(Json::as_array)
                .expect("step requests")
                .iter()
                .filter_map(Json::as_str)
                .collect();
            if ids != wanted {
                return Err(format!("suspended with {ids:?}, expected {wanted:?}"));
            }
            let mut answers = Vec::new();
            let mut answered: Vec<&str> = Vec::new();
            for pair in current.get("respond").and_then(Json::as_array).expect("step responses") {
                let [id, entry] = pair.as_array().expect("response pair") else {
                    return Err("a response is not a pair".to_string());
                };
                let id = id.as_str().expect("response id");
                answered.push(id);
                answers.push(with_id(id, entry));
            }
            kernel.send(&respond(answers));
            let unique: HashSet<&str> = answered.iter().copied().collect();
            let faulty = unique.len() != answered.len() || answered.iter().any(|id| !ids.contains(id));
            if !faulty && ids.iter().any(|id| !unique.contains(id)) {
                kernel.send(&Json::object([("type", Json::string("cancel"))]));
            }
        } else {
            let mut answers = Vec::new();
            for request in &requests {
                let response = host.answer(request).map_err(|e| format!("unexpected call: {e}"))?;
                let entry = match response {
                    sx_kernel::machine::Response::Output(output) => Json::object([("output", output)]),
                    sx_kernel::machine::Response::Refusal { kind, message } => Json::object([(
                        "error",
                        Json::object([("kind", Json::string(kind)), ("message", Json::string(message))]),
                    )]),
                };
                answers.push(with_id(request.id(), &entry));
            }
            for answer in answers.into_iter().rev() {
                kernel.send(&respond(vec![answer]));
            }
        }
    }
}

fn span_list(span: &Json) -> Json {
    if span.is_null() {
        return Json::Null;
    }
    let number = |side: &str, key: &str| span.get(side).and_then(|s| s.get(key)).cloned().expect("span coordinate");
    Json::Array(vec![number("start", "line"), number("start", "column"), number("end", "line"), number("end", "column")])
}

fn compare_logs(case: &Json, reply: &Json) -> Result<(), String> {
    let logs = reply.get("logs").cloned().unwrap_or(Json::Array(Vec::new()));
    let wanted = case.get("logs").cloned().unwrap_or(Json::Array(Vec::new()));
    if python_equal(&logs, &wanted) { Ok(()) } else { Err(format!("logs {} != {}", shown(&logs), shown(&wanted))) }
}

fn compare(case: &Json, reply: &Json, python: Option<&Json>) -> Result<(), String> {
    let expected = case.get("expect").expect("expect");
    if matches!(kind(reply), "completed" | "failed") {
        compare_logs(case, reply)?;
    }
    match kind(reply) {
        "completed" => {
            let value = reply.get("value").expect("value");
            let wanted = expected.get("completed").ok_or_else(|| format!("completed, expected {}", shown(expected)))?;
            if !python_equal(value, wanted) {
                return Err(format!("value {} != {}", shown(value), shown(wanted)));
            }
            let trace = reply.get("trace").expect("trace");
            let wanted_trace = case.get("trace").cloned().unwrap_or(Json::Array(Vec::new()));
            if !python_equal(trace, &wanted_trace) {
                return Err(format!("trace {} != {}", shown(trace), shown(&wanted_trace)));
            }
            if let Some(python) = python {
                let python_value =
                    python.get("completed").ok_or_else(|| format!("Python did not complete: {}", shown(python)))?;
                if !python_equal(value, python_value) {
                    return Err(format!("value {} != Python's {}", shown(value), shown(python_value)));
                }
            }
        }
        "failed" => {
            let failure = reply.get("failure").expect("failure");
            let name = failure.get("kind").and_then(Json::as_str).expect("failure kind");
            if expected.get("failed").and_then(Json::as_str) != Some(name) {
                return Err(format!("failed with {name}, expected {}", shown(expected)));
            }
            let trace = reply.get("trace").expect("trace");
            let wanted_trace = case.get("trace").cloned().unwrap_or(Json::Array(Vec::new()));
            if !python_equal(trace, &wanted_trace) {
                return Err(format!("trace {} != {}", shown(trace), shown(&wanted_trace)));
            }
            let detail = Json::object([
                ("failed", Json::string(name)),
                ("message", failure.get("message").cloned().expect("message")),
                ("span", span_list(failure.get("span").expect("span"))),
                ("request", failure.get("request").cloned().expect("request")),
            ]);
            if let Some(python) = python
                && !python_equal(&detail, python)
            {
                return Err(format!("failure {} != Python's {}", shown(&detail), shown(python)));
            }
        }
        "rejected" => {
            let codes: Vec<Json> = reply
                .get("diagnostics")
                .and_then(Json::as_array)
                .expect("diagnostics")
                .iter()
                .map(|d| d.get("code").cloned().expect("code"))
                .collect();
            let wanted = expected.get("rejected").ok_or_else(|| format!("rejected, expected {}", shown(expected)))?;
            if !python_equal(&Json::Array(codes.clone()), wanted) {
                return Err(format!("rejected with {}, expected {}", shown(&Json::Array(codes)), shown(wanted)));
            }
            if reply.get("rendered").and_then(Json::as_str).is_none_or(str::is_empty) {
                return Err("a rejection carries rendered diagnostics".to_string());
            }
        }
        "error" => {
            let wanted = expected
                .get("invalid-input")
                .and_then(Json::as_str)
                .ok_or_else(|| format!("an error, expected {}", shown(expected)))?;
            let code = reply.get("code").and_then(Json::as_str);
            let message = reply.get("message").and_then(Json::as_str).unwrap_or_default();
            if code != Some("invalid-input") || !message.contains(wanted) {
                return Err(format!("error {} does not match invalid-input {wanted:?}", shown(reply)));
            }
        }
        other => return Err(format!("unexpected reply {other}: {}", shown(reply))),
    }
    Ok(())
}

#[test]
fn every_run_fixture_gives_the_same_outcome_and_trace_through_the_protocol() {
    let (outcomes, _) = python_outputs();
    let mut accepted = 0;
    let mut rejected = 0;
    let mut failures = Vec::new();
    for path in cases() {
        let case = read_json(&path);
        let stem = path.file_stem().and_then(|s| s.to_str()).expect("fixture name");
        let declarations = read_json(&root().join("fixtures/run").join(case.get("declarations").and_then(Json::as_str).expect("declarations")));
        let is_rejected = case.get("expect").and_then(|e| e.get("rejected")).is_some();
        let python = if is_rejected || !json_case_runs_in_python(&case) {
            None
        } else {
            Some(read_json(&outcomes.join(format!("{stem}.outcome.json"))))
        };
        let mut kernel = Kernel::spawn();
        let result = drive(&mut kernel, &case, &declarations).and_then(|reply| compare(&case, &reply, python.as_ref()));
        let status = kernel.finish();
        match result {
            Err(problem) => failures.push(format!("{stem}: {problem}")),
            Ok(()) if !status.success() => failures.push(format!("{stem}: the kernel exited with {status}")),
            Ok(()) if is_rejected => rejected += 1,
            Ok(()) => accepted += 1,
        }
    }
    eprintln!("{accepted} accepted and {rejected} rejected run fixtures agree through the protocol");
    assert!(accepted > 100 && rejected > 10);
    assert!(failures.is_empty(), "{} fixtures disagree:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn explain_reports_what_python_reports_for_every_run_fixture() {
    let (_, explained) = python_outputs();
    let mut failures = Vec::new();
    let mut compared = 0;
    let mut kernel = Kernel::spawn();
    for path in cases() {
        let case = read_json(&path);
        if !json_case_runs_in_python(&case) {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).expect("fixture name");
        let declarations = read_json(&root().join("fixtures/run").join(case.get("declarations").and_then(Json::as_str).expect("declarations")));
        let python = read_json(&explained.join(format!("{stem}.explain.json")));
        kernel.send(&Json::object([
            ("version", Json::Int(1)),
            ("type", Json::string("explain")),
            ("program", case.get("program").cloned().expect("program")),
            ("declarations", declarations),
            ("allow", case.get("allow").cloned().expect("allow")),
        ]));
        let reply = kernel.receive();
        let output = python.get("output").expect("Python output");
        let agrees = match kind(&reply) {
            "explained" => {
                ["tools", "inputs", "result"]
                    .iter()
                    .all(|key| match (reply.get(key), output.get(key)) {
                        (Some(ours), Some(theirs)) => python_equal(ours, theirs),
                        _ => false,
                    })
            }
            "rejected" => {
                let ours: Vec<Json> =
                    reply.get("diagnostics").and_then(Json::as_array).expect("diagnostics").iter().filter_map(|d| d.get("code").cloned()).collect();
                let theirs: Vec<Json> = output.get("diagnostics").and_then(Json::as_array).map_or(Vec::new(), |list| {
                    list.iter().filter_map(|d| d.get("code").cloned()).collect()
                });
                python_equal(&Json::Array(ours), &Json::Array(theirs))
            }
            _ => false,
        };
        compared += 1;
        if !agrees {
            failures.push(format!("{stem}: {} != Python's {}", shown(&reply), shown(output)));
        }
    }
    assert!(kernel.finish().success());
    assert!(compared > 100);
    assert!(failures.is_empty(), "{} explanations disagree:\n{}", failures.len(), failures.join("\n"));
}

const DECLARATIONS: &str = r#"{"inputs":{"name":"String"},"capabilities":[{"id":"store.get","effect":"read","input":"Record {id: String}","output":"Record {id: String, size: Int}","description":"Reads."}]}"#;

fn start(extra: &str) -> String {
    format!(
        r#"{{"version":1,"type":"start","program":"(par (call store.get {{:id name}}) (call store.get {{:id \"b\"}}))","declarations":{DECLARATIONS},"allow":["store.get"]{extra}}}"#
    )
}

fn replies(lines: Vec<Vec<u8>>) -> (Vec<Json>, ExitStatus) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sx"))
        .arg("serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("sx serve starts");
    let mut stdin = child.stdin.take().expect("piped stdin");
    let writer = thread::spawn(move || {
        for line in lines {
            if stdin.write_all(&line).and_then(|()| stdin.write_all(b"\n")).is_err() {
                break;
            }
        }
    });
    let mut parsed = Vec::new();
    for line in BufReader::new(child.stdout.take().expect("piped stdout")).lines() {
        let line = line.expect("kernel output is UTF-8");
        parsed.push(parse(&line).unwrap_or_else(|e| panic!("reply is not JSON ({e}): {line}")));
    }
    writer.join().expect("writer finishes");
    (parsed, child.wait().expect("kernel exits"))
}

fn lines(texts: &[&str]) -> Vec<Vec<u8>> {
    texts.iter().map(|t| t.as_bytes().to_vec()).collect()
}

fn summary(reply: &Json) -> String {
    match kind(reply) {
        "error" => format!("error:{}", reply.get("code").and_then(Json::as_str).expect("error code")),
        other => other.to_string(),
    }
}

fn summaries(texts: &[&str]) -> Vec<String> {
    let (replies, status) = replies(lines(texts));
    assert!(status.success() || status.code() == Some(1), "unexpected exit {status}");
    replies.iter().map(summary).collect()
}

#[test]
fn malformed_messages_get_a_structured_error_and_the_session_continues() {
    let first = r#"{"version":1,"type":"cancel"}"#;
    let good = start(r#","inputs":{"name":"a"}"#);
    let started_0 = start(r#","inputs":{"name":"a","other":1}"#);
    let started_1 = start(r#","inputs":{"name":3}"#);
    let started_2 = start(r#","inputs":[]"#);
    let started_3 = start(r#","inputs":{"name":"a"},"budgets":{"fuel":3}"#);
    let started_4 = start(r#","inputs":{"name":"a"},"budgets":{"steps":-3}"#);
    let started_5 = start(r#","inputs":{"name":"a"},"budgets":{"steps":"3"}"#);
    let without_input = start("");
    let cases: Vec<(&str, Vec<&str>, Vec<&str>)> = vec![
        ("not JSON", vec!["not json"], vec!["error:malformed-json"]),
        ("a truncated object", vec![r#"{"version":1,"type":"#], vec!["error:malformed-json"]),
        ("a bare array", vec!["[1,2]"], vec!["error:invalid-message"]),
        ("a bare string", vec!["\"start\""], vec!["error:invalid-message"]),
        ("no version first", vec![r#"{"type":"cancel"}"#], vec!["error:invalid-message"]),
        ("a string version", vec![r#"{"version":"1","type":"cancel"}"#], vec!["error:invalid-message"]),
        ("a float version", vec![r#"{"version":1.0,"type":"cancel"}"#], vec!["error:invalid-message"]),
        ("another version", vec![r#"{"version":2,"type":"cancel"}"#], vec!["error:unsupported-version"]),
        ("a changed version", vec![first, r#"{"version":3,"type":"cancel"}"#], vec!["error:unexpected-message", "error:unsupported-version"]),
        ("no type", vec![r#"{"version":1}"#], vec!["error:invalid-message"]),
        ("a numeric type", vec![r#"{"version":1,"type":4}"#], vec!["error:invalid-message"]),
        ("an unknown type", vec![r#"{"version":1,"type":"stop"}"#], vec!["error:invalid-message"]),
        ("an unknown field", vec![r#"{"version":1,"type":"cancel","now":true}"#], vec!["error:invalid-message"]),
        ("no program", vec![r#"{"version":1,"type":"start","allow":[]}"#], vec!["error:invalid-message"]),
        ("a numeric program", vec![r#"{"version":1,"type":"start","program":1,"allow":[]}"#], vec!["error:invalid-message"]),
        ("no allow list", vec![r#"{"version":1,"type":"start","program":"1"}"#], vec!["error:invalid-message"]),
        ("an allow list of numbers", vec![r#"{"version":1,"type":"start","program":"1","allow":[1]}"#], vec!["error:invalid-message"]),
        ("responses outside an execution", vec![r#"{"version":1,"type":"respond","responses":[]}"#], vec!["error:unexpected-message"]),
        ("a cancel outside an execution", vec![first], vec!["error:unexpected-message"]),
        ("responses that are not a list", vec![r#"{"version":1,"type":"respond","responses":{}}"#], vec!["error:invalid-message"]),
        (
            "declarations that are not an object",
            vec![r#"{"version":1,"type":"start","program":"1","allow":[],"declarations":[]}"#],
            vec!["error:invalid-declarations"],
        ),
        (
            "a type that does not parse",
            vec![r#"{"version":1,"type":"start","program":"1","allow":[],"declarations":{"types":{"A":"Nope<"}}}"#],
            vec!["error:invalid-declarations"],
        ),
        (
            "an unknown core",
            vec![r#"{"version":1,"type":"start","program":"1","allow":[],"declarations":{"cores":["nope"]}}"#],
            vec!["error:invalid-declarations"],
        ),
        (
            "an allow-list entry nobody declared",
            vec![r#"{"version":1,"type":"start","program":"1","allow":["ghost.call"]}"#],
            vec!["error:unknown-capability"],
        ),
        ("a declared input without a value", vec![&without_input[..]], vec!["error:invalid-input"]),
        ("an undeclared input", vec![&started_0[..]], vec!["error:invalid-input"]),
        ("an input of the wrong type", vec![&started_1[..]], vec!["error:invalid-input"]),
        ("inputs that are not an object", vec![&started_2[..]], vec!["error:invalid-input"]),
        ("an unknown budget", vec![&started_3[..]], vec!["error:invalid-budget"]),
        ("a negative budget", vec![&started_4[..]], vec!["error:invalid-budget"]),
        ("a textual budget", vec![&started_5[..]], vec!["error:invalid-budget"]),
        ("blank lines", vec!["", "   ", first], vec!["error:unexpected-message"]),
        (
            "errors then a good execution",
            vec!["{", "[]", &good[..], r#"{"type":"cancel"}"#],
            vec!["error:malformed-json", "error:invalid-message", "requests", "failed"],
        ),
        (
            "a start while waiting",
            vec![&good[..], &good[..], r#"{"type":"cancel"}"#],
            vec!["requests", "error:unexpected-message", "failed"],
        ),
        (
            "explain while waiting",
            vec![&good[..], r#"{"type":"explain","program":"1"}"#, r#"{"type":"cancel"}"#],
            vec!["requests", "explained", "failed"],
        ),
        (
            "malformed responses while waiting",
            vec![
                &good[..],
                r#"{"type":"respond","responses":[{"id":"r.0:1"}]}"#,
                r#"{"type":"respond","responses":[{"id":"r.0:1","output":1,"error":{"kind":"Failed","message":"x"}}]}"#,
                r#"{"type":"respond","responses":[{"output":1}]}"#,
                r#"{"type":"respond","responses":[{"id":"r.0:1","error":{"kind":"Failed"}}]}"#,
                r#"{"type":"respond","responses":[{"id":"r.0:1","output":1,"extra":1}]}"#,
                r#"{"type":"respond","responses":[7]}"#,
                r#"{"type":"cancel"}"#,
            ],
            vec![
                "requests",
                "error:invalid-message",
                "error:invalid-message",
                "error:invalid-message",
                "error:invalid-message",
                "error:invalid-message",
                "error:invalid-message",
                "failed",
            ],
        ),
        (
            "an input that ends mid-execution",
            vec![&good[..]],
            vec!["requests", "error:end-of-input"],
        ),
    ];
    for (name, texts, wanted) in cases {
        assert_eq!(summaries(&texts), wanted, "{name}");
    }
}

#[test]
fn explain_reports_a_rejected_program_without_running_it() {
    let texts = [
        r#"{"version":1,"type":"explain","program":"(call ghost.call {})"}"#,
        r#"{"type":"explain","program":"(call store.get {:id \"a\"})","declarations":{"capabilities":[{"id":"store.get","effect":"read","input":"Record {id: String}","output":"String","description":"Reads."}]}}"#,
    ];
    let (replies, status) = replies(lines(&texts));
    assert!(status.success());
    assert_eq!(replies.iter().map(summary).collect::<Vec<_>>(), ["rejected", "explained"]);
    let codes: Vec<&str> = replies[0]
        .get("diagnostics")
        .and_then(Json::as_array)
        .expect("diagnostics")
        .iter()
        .filter_map(|d| d.get("code").and_then(Json::as_str))
        .collect();
    assert_eq!(codes, ["unknown-capability"]);
    let tool = &replies[1].get("tools").and_then(Json::as_array).expect("tools")[0];
    assert_eq!(tool.get("allowed").and_then(Json::as_bool), Some(false));
}

#[test]
fn hostile_framing_is_answered_and_the_session_survives() {
    let first = br#"{"version":1,"type":"cancel"}"#.to_vec();
    let mut invalid_utf8 = br#"{"version":1,"type":""#.to_vec();
    invalid_utf8.extend_from_slice(&[0xff, 0xfe, 0xc0]);
    invalid_utf8.extend_from_slice(br#""}"#);
    let mut nested = vec![b'['; 200_000];
    nested.extend(std::iter::repeat_n(b']', 200_000));
    let mut deep_object = Vec::new();
    for _ in 0..50_000 {
        deep_object.extend_from_slice(br#"{"a":"#);
    }
    deep_object.push(b'1');
    deep_object.extend(std::iter::repeat_n(b'}', 50_000));
    let mut long = br#"{"version":1,"type":"cancel","pad":""#.to_vec();
    long.extend(std::iter::repeat_n(b'a', 17 * 1024 * 1024));
    long.extend_from_slice(br#""}"#);
    let carriage = b"{\"version\":1,\"type\":\"cancel\"}\r".to_vec();
    let (answered, status) = replies(vec![invalid_utf8, nested, deep_object, long, carriage, first]);
    assert!(status.success());
    assert_eq!(
        answered.iter().map(summary).collect::<Vec<_>>(),
        [
            "error:malformed-json",
            "error:malformed-json",
            "error:malformed-json",
            "error:line-too-long",
            "error:unexpected-message",
            "error:unexpected-message",
        ]
    );
}

fn mutations(seed: u64, originals: &[Vec<u8>], count: usize) -> Vec<Vec<u8>> {
    let mut state = seed;
    let mut next = move || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) as usize
    };
    let mut out = Vec::new();
    for _ in 0..count {
        let mut line = originals[next() % originals.len()].clone();
        for _ in 0..=next() % 4 {
            if line.is_empty() {
                break;
            }
            let at = next() % line.len();
            match next() % 5 {
                0 => line[at] = (next() % 256) as u8,
                1 => {
                    line.remove(at);
                }
                2 => line.truncate(at),
                3 => line.insert(at, b"{}[]\":,0-\\n"[next() % 11]),
                _ => {
                    let piece: Vec<u8> = line[at..].to_vec();
                    line.extend(piece);
                }
            }
        }
        line.retain(|b| *b != b'\n');
        out.push(line);
    }
    out
}

#[test]
fn mutated_messages_never_crash_the_kernel() {
    let good = start(r#","inputs":{"name":"a"},"budgets":{"steps":100000}"#);
    let originals: Vec<Vec<u8>> = [
        r#"{"version":1,"type":"cancel"}"#.to_string(),
        r#"{"type":"respond","responses":[{"id":"r.0:1","output":{"id":"a","size":1}}]}"#.to_string(),
        r#"{"type":"respond","responses":[{"id":"r.1:1","error":{"kind":"NotFound","message":"gone"}}]}"#.to_string(),
        r#"{"type":"explain","program":"(call store.get {:id \"a\"})","allow":["store.get"],"declarations":{"capabilities":[{"id":"store.get","effect":"read","input":"Record {id: String}","output":"String","description":"d"}]}}"#.to_string(),
        good,
    ]
    .iter()
    .map(|t| t.as_bytes().to_vec())
    .collect();
    let mut all = vec![br#"{"version":1,"type":"cancel"}"#.to_vec()];
    all.extend(mutations(7, &originals, 4000));
    let (answered, status) = replies(all);
    assert!(status.success() || status.code() == Some(1), "the kernel crashed: {status}");
    assert!(!answered.is_empty());
    let known = ["error", "rejected", "explained", "requests", "completed", "failed"];
    for reply in &answered {
        assert!(known.contains(&kind(reply)), "unexpected reply {}", shown(reply));
        if kind(reply) == "error" {
            assert!(reply.get("code").and_then(Json::as_str).is_some());
            assert!(reply.get("message").and_then(Json::as_str).is_some());
        }
    }
}

fn script_start(program: &str, extra: &str) -> String {
    let program = dumps(&Json::string(program), Layout::Spaced).expect("program text");
    format!(r#"{{"version":1,"type":"start","program":{program},"allow":[]{extra}}}"#)
}

const SCRIPT: &str = r#"(script {:inputs {:owners "List<String>" :note "Option<String>"}} [(count owners) note])"#;

#[test]
fn a_script_starts_with_its_inputs_as_json_and_needs_no_declarations() {
    let good = script_start(SCRIPT, r#","inputs":{"owners":["a","b"]}"#);
    let explicit = script_start(SCRIPT, r#","inputs":{"owners":[],"note":"x"}"#);
    let missing = script_start(SCRIPT, r#","inputs":{"note":"x"}"#);
    let unknown = script_start(SCRIPT, r#","inputs":{"owners":[],"extra":1}"#);
    let mistyped = script_start(SCRIPT, r#","inputs":{"owners":["a",1]}"#);
    let (replies, status) = replies(lines(&[&good, &explicit, &missing, &unknown, &mistyped]));
    assert!(status.success());
    let value = |at: usize| replies[at].get("value").cloned().expect("a completed value");
    assert!(python_equal(&value(0), &parse("[2, null]").expect("json")));
    assert!(python_equal(&value(1), &parse("[0, \"x\"]").expect("json")));
    let message = |at: usize| {
        assert_eq!(summary(&replies[at]), "error:invalid-input");
        replies[at].get("message").and_then(Json::as_str).expect("message").to_string()
    };
    assert_eq!(message(2), "the script needs a value for input owners; its required inputs are owners: List<String>");
    assert_eq!(message(3), "extra is not an input of the script, whose inputs are note, owners");
    assert_eq!(message(4), "input owners: $owners[1]: expected String, found an integer");
}

#[test]
fn explain_lists_a_scripts_declared_inputs_with_their_types() {
    let explain = script_start(SCRIPT, "").replace(r#""type":"start""#, r#""type":"explain""#);
    let (replies, _) = replies(lines(&[&explain]));
    let reply = &replies[0];
    assert_eq!(kind(reply), "explained");
    assert!(python_equal(reply.get("inputs").expect("inputs"), &parse(r#"["note", "owners"]"#).expect("json")));
    let types = parse(r#"{"note": "Option<String>", "owners": "List<String>"}"#).expect("json");
    assert!(python_equal(reply.get("input_types").expect("input_types"), &types));
}

#[test]
fn a_script_and_a_declarations_object_that_disagree_on_inputs_are_invalid_declarations() {
    let declarations = r#","declarations":{"inputs":{"owners":"List<Int>"}}"#;
    let start = script_start(SCRIPT, &format!(r#"{declarations},"inputs":{{"owners":[]}}"#));
    let explain = script_start(SCRIPT, declarations).replace(r#""type":"start""#, r#""type":"explain""#);
    let (replies, _) = replies(lines(&[&start, &explain]));
    for reply in &replies {
        assert_eq!(summary(reply), "error:invalid-declarations");
        let message = reply.get("message").and_then(Json::as_str).expect("message");
        assert!(message.contains("input owners is List<String> in the script's header and List<Int> in the declarations"), "{message}");
        assert!(message.contains("the script's header declares input note (Option<String>), which the declarations do not"), "{message}");
    }
}
