use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use sx_kernel::json::{Json, Layout, dumps, parse};

const SX: &str = env!("CARGO_BIN_EXE_sx");
const FIXTURE: &str = env!("CARGO_BIN_EXE_mcp-fixture");
const WAIT: Duration = Duration::from_secs(30);

static SERIAL: AtomicUsize = AtomicUsize::new(0);

fn json(text: &str) -> Json {
    parse(text).unwrap_or_else(|e| panic!("{e}: {text}"))
}

fn shown(value: &Json) -> String {
    dumps(value, Layout::Spaced).expect("serializable")
}

fn fixture_tools() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/mcp/run.json").to_string_lossy().into_owned()
}

fn string_at<'a>(value: &'a Json, path: &[&str]) -> &'a str {
    let mut at = value;
    for key in path {
        at = at.get(key).unwrap_or_else(|| panic!("no {key} in {}", shown(value)));
    }
    at.as_str().unwrap_or_else(|| panic!("{path:?} is not a string in {}", shown(value)))
}

fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, into: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).expect("directory") {
            let path = entry.expect("entry").path();
            let name = path.strip_prefix(base).expect("inside").to_string_lossy().into_owned();
            if path.is_dir() {
                into.insert(format!("{name}/"), Vec::new());
                walk(base, &path, into);
            } else {
                into.insert(name, fs::read(&path).expect("file"));
            }
        }
    }
    let mut into = BTreeMap::new();
    walk(root, root, &mut into);
    into
}

struct Sandbox {
    base: PathBuf,
    proxied: bool,
    typescript: bool,
}

impl Sandbox {
    fn new(label: &str, proxied: bool, grants: &str, policy: &str) -> Sandbox {
        Sandbox::build(label, proxied, false, grants, policy)
    }

    fn typescript(label: &str, proxied: bool, grants: &str, policy: &str) -> Sandbox {
        Sandbox::build(label, proxied, true, grants, policy)
    }

    fn build(label: &str, proxied: bool, typescript: bool, grants: &str, policy: &str) -> Sandbox {
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("mcp-code-{}-{label}-{serial}", std::process::id()));
        if base.exists() {
            fs::remove_dir_all(&base).expect("old sandbox removed");
        }
        fs::create_dir_all(base.join("root")).expect("sandbox");
        let base = fs::canonicalize(base).expect("canonical sandbox");
        fs::write(base.join("root/a.txt"), "alpha\n").expect("seed file");
        fs::write(base.join("root/names.txt"), "root/b.txt\n").expect("seed file");
        let root = base.join("root");
        let substitute = |text: &str| text.replace("ROOT", root.to_str().expect("UTF-8 path"));
        let mut declarations = json(r#"{"packs": ["fs"]}"#);
        if proxied {
            let imported = Command::new(SX)
                .args(["mcp", "import", "--name", "fx", "--tools", &fixture_tools()])
                .output()
                .expect("import runs");
            assert_eq!(imported.status.code(), Some(0));
            let mut merged = json(&String::from_utf8_lossy(&imported.stdout));
            if let (Json::Object(fields), Json::Object(own)) = (&mut merged, &mut declarations) {
                fields.append(own);
            }
            declarations = merged;
        }
        fs::write(base.join("declarations.json"), shown(&declarations)).expect("declarations");
        fs::write(base.join("grants.json"), substitute(grants)).expect("grants");
        fs::write(base.join("policy.json"), policy).expect("policy");
        Sandbox { base, proxied, typescript }
    }

    fn root(&self) -> PathBuf {
        self.base.join("root")
    }

    fn at(&self, name: &str) -> String {
        self.base.join(name).to_string_lossy().into_owned()
    }

    fn arguments(&self) -> Vec<String> {
        let mut args: Vec<String> = [
            "mcp",
            "code",
            "--declarations",
            &self.at("declarations.json"),
            "--grants",
            &self.at("grants.json"),
            "--policy",
            &self.at("policy.json"),
        ]
        .map(str::to_string)
        .to_vec();
        if self.typescript {
            args.extend(["--language".to_string(), "ts".to_string()]);
        }
        if self.proxied {
            args.extend(["--".to_string(), FIXTURE.to_string(), fixture_tools()]);
        }
        args
    }

    fn client(&self) -> Client {
        let mut client = Client::start(&self.arguments());
        client.initialize();
        client
    }
}

struct Client {
    child: Child,
    input: Option<ChildStdin>,
    lines: Receiver<Json>,
    errors: Receiver<String>,
    next_id: i64,
}

impl Client {
    fn start(args: &[String]) -> Client {
        let mut child = Command::new(SX)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("sx starts");
        let input = child.stdin.take();
        let stdout = child.stdout.take().expect("stdout");
        let mut stderr = child.stderr.take().expect("stderr");
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                if sender.send(json(&line)).is_err() {
                    return;
                }
            }
        });
        let (error_sender, errors) = mpsc::channel();
        thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.read_to_string(&mut text);
            let _ = error_sender.send(text);
        });
        Client { child, input, lines, errors, next_id: 1 }
    }

    fn send(&mut self, message: &Json) {
        let input = self.input.as_mut().expect("stdin is open");
        input.write_all(format!("{}\n", shown(message)).as_bytes()).and_then(|()| input.flush()).expect("written");
    }

    fn request(&mut self, method: &str, params: Json) -> Json {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&Json::object([
            ("jsonrpc", Json::string("2.0")),
            ("id", Json::Int(id)),
            ("method", Json::string(method)),
            ("params", params),
        ]));
        let reply = self.lines.recv_timeout(WAIT).expect("an answer");
        assert_eq!(reply.get("id").and_then(Json::as_i64), Some(id), "{}", shown(&reply));
        reply
    }

    fn initialize(&mut self) {
        let params = json(r#"{"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}"#);
        self.request("initialize", params);
        self.send(&json(r#"{"jsonrpc": "2.0", "method": "notifications/initialized"}"#));
    }

    fn list(&mut self) -> Vec<Json> {
        let reply = self.request("tools/list", Json::Object(Vec::new()));
        reply.get("result").and_then(|r| r.get("tools")).and_then(Json::as_array).expect("tools").to_vec()
    }

    fn tool(&mut self, name: &str, arguments: Json) -> Json {
        let params = Json::object([("name", Json::string(name)), ("arguments", arguments)]);
        let reply = self.request("tools/call", params);
        reply.get("result").cloned().unwrap_or_else(|| panic!("no result in {}", shown(&reply)))
    }

    fn program(&mut self, tool: &str, source: &str) -> Reply {
        self.program_with(tool, source, None)
    }

    fn program_with(&mut self, tool: &str, source: &str, inputs: Option<Json>) -> Reply {
        let mut fields = vec![("program", Json::string(source))];
        if let Some(inputs) = inputs {
            fields.push(("inputs", inputs));
        }
        let result = self.tool(tool, Json::object(fields));
        Reply {
            is_error: result.get("isError").and_then(Json::as_bool).expect("isError"),
            structured: result.get("structuredContent").cloned().expect("structuredContent"),
            text: string_at(result.get("content").and_then(Json::as_array).and_then(|c| c.first()).expect("a text block"), &["text"]).to_string(),
        }
    }

    fn finish(mut self) -> (Option<i32>, String) {
        self.input.take();
        let deadline = Instant::now() + WAIT;
        let status = loop {
            if let Some(status) = self.child.try_wait().expect("waited") {
                break status.code();
            }
            assert!(Instant::now() < deadline, "sx mcp code did not exit");
            thread::sleep(Duration::from_millis(10));
        };
        (status, self.errors.recv_timeout(WAIT).unwrap_or_default())
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Reply {
    is_error: bool,
    structured: Json,
    text: String,
}

fn capabilities_called(trace: &Json) -> Vec<String> {
    trace
        .as_array()
        .expect("trace")
        .iter()
        .filter_map(|entry| entry.get("capability").and_then(Json::as_str).map(str::to_string))
        .collect()
}

const FS_GRANTS: &str = r#"{"fs.read-text": {"roots": ["ROOT"]}, "fs.write-text": {"roots": ["ROOT"]}}"#;
const MIXED_GRANTS: &str =
    r#"{"fs.read-text": {"roots": ["ROOT"]}, "fs.write-text": {"roots": ["ROOT"]}, "mcp-fx.add": null, "mcp-fx.fail-write": null}"#;
const OPEN_POLICY: &str = r#"{"capabilities": ["fs.read-text", "fs.write-text"], "computed_inputs": ["fs.read-text", "fs.write-text"]}"#;

const COPY: &str = "(let [text (call fs.read-text {:path \"ROOT/a.txt\"})]\n  (call fs.write-text {:path \"ROOT/b.txt\" :text text}))";
const LITERAL_WRITE: &str = "(call fs.write-text {:path \"ROOT/c.txt\" :text \"created\"})";

fn program_for(sandbox: &Sandbox, source: &str) -> String {
    source.replace("ROOT", sandbox.root().to_str().expect("UTF-8 path"))
}

#[test]
fn the_server_offers_check_and_run_and_lists_the_guide_for_its_grants_once() {
    let sandbox = Sandbox::new("listing", false, FS_GRANTS, OPEN_POLICY);
    let guide = Command::new(SX)
        .args(["guide", "--declarations", &sandbox.at("declarations.json"), "--grants", &sandbox.at("grants.json")])
        .output()
        .expect("guide runs");
    assert_eq!(guide.status.code(), Some(0));
    let guide = String::from_utf8_lossy(&guide.stdout).trim_end().to_string();
    let mut client = sandbox.client();
    let tools = client.list();
    let names: Vec<&str> = tools.iter().map(|tool| string_at(tool, &["name"])).collect();
    assert_eq!(names, ["check", "run"]);
    let descriptions: Vec<&str> = tools.iter().map(|tool| string_at(tool, &["description"])).collect();
    assert_eq!(descriptions.iter().filter(|description| description.contains(&guide)).count(), 1, "{descriptions:?}");
    assert!(descriptions.get(1).is_some_and(|run| run.contains(&guide)), "the run description lacks the guide");
    assert!(descriptions.first().is_some_and(|check| check.contains("the run tool's description")), "{descriptions:?}");
    for description in &descriptions {
        assert!(description.contains("fs.read-text, fs.write-text"), "{description}");
    }
    let required = tools.iter().map(|tool| shown(tool.get("inputSchema").expect("schema"))).collect::<Vec<_>>();
    assert!(required.iter().all(|schema| schema.contains(r#""required": ["program"]"#)), "{required:?}");
}

#[test]
fn check_explains_a_good_program_and_performs_nothing() {
    let sandbox = Sandbox::new("check-good", false, FS_GRANTS, OPEN_POLICY);
    let before = snapshot(&sandbox.root());
    let mut client = sandbox.client();
    let reply = client.program("check", &program_for(&sandbox, COPY));
    assert!(!reply.is_error, "{}", shown(&reply.structured));
    assert_eq!(reply.structured.get("ok").and_then(Json::as_bool), Some(true));
    let tools = reply.structured.get("explain").and_then(|e| e.get("tools")).and_then(Json::as_array).expect("tools");
    let ids: Vec<&str> = tools.iter().map(|tool| string_at(tool, &["id"])).collect();
    assert_eq!(ids, ["fs.read-text", "fs.write-text"]);
    assert_eq!(string_at(&reply.structured, &["policy", "verdict"]), "approved");
    assert_eq!(snapshot(&sandbox.root()), before);
}

#[test]
fn a_program_that_does_not_check_returns_diagnostics_and_performs_nothing() {
    let sandbox = Sandbox::new("bad", false, FS_GRANTS, OPEN_POLICY);
    let before = snapshot(&sandbox.root());
    let mut client = sandbox.client();
    let source = program_for(&sandbox, "(call fs.write-text {:path \"ROOT/c.txt\" :content \"x\"})");
    for tool in ["check", "run"] {
        let reply = client.program(tool, &source);
        assert!(reply.is_error, "{tool}");
        let diagnostics = reply.structured.get("diagnostics").and_then(Json::as_array).expect("diagnostics");
        assert!(diagnostics.iter().any(|d| string_at(d, &["code"]) == "unknown-field"), "{}", shown(&reply.structured));
        assert!(reply.text.contains("content"), "{}", reply.text);
    }
    assert_eq!(snapshot(&sandbox.root()), before);
}

#[test]
fn a_program_that_calls_an_ungranted_tool_does_not_check() {
    let sandbox = Sandbox::new("ungranted", false, r#"{"fs.read-text": {"roots": ["ROOT"]}}"#, r#"{"capabilities": ["fs.read-text"]}"#);
    let before = snapshot(&sandbox.root());
    let mut client = sandbox.client();
    let reply = client.program("run", &program_for(&sandbox, LITERAL_WRITE));
    assert!(reply.is_error);
    let diagnostics = reply.structured.get("diagnostics").and_then(Json::as_array).expect("diagnostics");
    assert!(diagnostics.iter().any(|d| string_at(d, &["code"]) == "unauthorized"), "{}", shown(&reply.structured));
    assert_eq!(snapshot(&sandbox.root()), before);
}

#[test]
fn run_performs_native_calls_and_returns_the_result_and_the_calls() {
    let sandbox = Sandbox::new("native", false, FS_GRANTS, OPEN_POLICY);
    let mut client = sandbox.client();
    let reply = client.program("run", &program_for(&sandbox, COPY));
    assert!(!reply.is_error, "{}", shown(&reply.structured));
    assert_eq!(string_at(&reply.structured, &["outcome"]), "completed");
    let trace = reply.structured.get("trace").expect("trace");
    assert_eq!(capabilities_called(trace), ["fs.read-text", "fs.write-text"]);
    assert!(reply.text.contains("calls (2):"), "{}", reply.text);
    assert_eq!(fs::read_to_string(sandbox.root().join("b.txt")).expect("written"), "alpha\n");
}

#[test]
fn run_calls_a_proxied_mcp_server_next_to_native_tools() {
    let policy = r#"{"capabilities": ["fs.read-text", "mcp-fx.add"]}"#;
    let sandbox = Sandbox::new("proxied", true, MIXED_GRANTS, policy);
    let mut client = sandbox.client();
    let source = program_for(
        &sandbox,
        "(let [text (call fs.read-text {:path \"ROOT/a.txt\"})\n      sum (call mcp-fx.add {:a 20 :b 22})]\n  {:text text :sum (get sum :sum)})",
    );
    let reply = client.program("run", &source);
    assert!(!reply.is_error, "{}", shown(&reply.structured));
    let result = reply.structured.get("result").expect("result");
    assert_eq!(result.get("sum").and_then(Json::as_i64), Some(42));
    assert_eq!(string_at(result, &["text"]), "alpha\n");
    let mut called = capabilities_called(reply.structured.get("trace").expect("trace"));
    called.sort();
    assert_eq!(called, ["fs.read-text", "mcp-fx.add"]);
}

#[test]
fn run_decodes_a_proxied_payload_enum_and_results_into_sx_values() {
    let policy = r#"{"capabilities": ["mcp-fx.variants"]}"#;
    let sandbox = Sandbox::new("variants", true, r#"{"mcp-fx.variants": null}"#, policy);
    let mut client = sandbox.client();
    let source = program_for(
        &sandbox,
        "(let [r (call mcp-fx.variants {})]\n  {:shape (match (get r :shape) Dot \"dot\" (Circle radius) (str \"circle \" radius) (Square s) (str \"square \" (get s :side)))\n   :outcome (match (get r :outcome) (ok n) n (err e) -1)\n   :fail (match (get r :fail) (ok n) \"ok\" (err e) e)})",
    );
    let reply = client.program("run", &source);
    assert!(!reply.is_error, "{}", shown(&reply.structured));
    let result = reply.structured.get("result").expect("result");
    assert_eq!(string_at(result, &["shape"]), "circle 2.5");
    assert_eq!(result.get("outcome").and_then(Json::as_i64), Some(3));
    assert_eq!(string_at(result, &["fail"]), "nope");
}

#[test]
fn run_supplies_declared_inputs_to_the_program() {
    let sandbox = Sandbox::new("inputs", false, FS_GRANTS, OPEN_POLICY);
    let mut client = sandbox.client();
    let source = program_for(&sandbox, "(script {:inputs {:file \"String\"}} (call fs.read-text {:path file}))");
    let path = format!("{}/a.txt", sandbox.root().display());
    let reply = client.program_with("run", &source, Some(Json::object([("file", Json::string(path))])));
    assert!(!reply.is_error, "{}", shown(&reply.structured));
    assert_eq!(string_at(&reply.structured, &["result"]), "alpha\n");
    let missing = client.program("run", &source);
    assert!(missing.is_error);
    assert!(string_at(&missing.structured, &["error"]).contains("needs a value for input file"), "{}", shown(&missing.structured));
}

#[test]
fn closers_inferred_from_indentation_are_reported_with_the_result() {
    let sandbox = Sandbox::new("closers", false, FS_GRANTS, OPEN_POLICY);
    let mut client = sandbox.client();
    let reply = client.program("run", "(let [a 1\n      b 2]\n  (+ a b)))\n");
    assert!(!reply.is_error, "{}", shown(&reply.structured));
    assert_eq!(reply.structured.get("result").and_then(Json::as_i64), Some(3));
    let edits = reply.structured.get("inferred_closers").and_then(Json::as_array).expect("inferred_closers");
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].get("line").and_then(Json::as_i64), Some(3));
}

#[test]
fn logs_are_reported_next_to_the_result_and_survive_a_failure() {
    let sandbox = Sandbox::new("logs", false, FS_GRANTS, OPEN_POLICY);
    let mut client = sandbox.client();
    let reply = client.program("run", "(let [_ (log \"found\" 2) _ (log {:ok true})] 5)");
    assert!(!reply.is_error, "{}", shown(&reply.structured));
    assert_eq!(shown(reply.structured.get("logs").expect("logs")), r#"["found 2", "{\"ok\":true}"]"#);
    assert!(reply.text.contains("logs (2):\n  found 2\n"), "{}", reply.text);
    let quiet = client.program("run", "5");
    assert!(quiet.structured.get("logs").is_none(), "{}", shown(&quiet.structured));
    let failed = client.program("run", "(let [_ (log \"before\")] (fail \"stop\"))");
    assert!(failed.is_error);
    assert_eq!(string_at(&failed.structured, &["failure", "kind"]), "thrown");
    assert_eq!(shown(failed.structured.get("logs").expect("logs")), r#"["before"]"#);
}

fn refusal_kinds(reply: &Reply) -> Vec<String> {
    let refusals = reply.structured.get("policy").and_then(|p| p.get("refusals")).and_then(Json::as_array).expect("refusals");
    refusals.iter().map(|refusal| string_at(refusal, &["kind"]).to_string()).collect()
}

fn assert_refused_without_effect(sandbox: &Sandbox, source: &str, kinds: &[&str], mentions: &[&str]) {
    let before = snapshot(&sandbox.root());
    let mut client = sandbox.client();
    let reply = client.program("run", &program_for(sandbox, source));
    assert!(reply.is_error, "{}", shown(&reply.structured));
    assert_eq!(string_at(&reply.structured, &["outcome"]), "refused", "{}", shown(&reply.structured));
    assert_eq!(refusal_kinds(&reply), kinds);
    assert!(reply.structured.get("trace").is_none());
    for mention in mentions {
        assert!(reply.text.contains(mention), "{mention} missing from {}", reply.text);
    }
    assert_eq!(snapshot(&sandbox.root()), before);
    let checked = client.program("check", &program_for(sandbox, source));
    assert!(checked.is_error);
    assert_eq!(refusal_kinds(&checked), kinds);
}

#[test]
fn a_capability_the_policy_does_not_allow_is_refused_before_anything_runs() {
    let sandbox = Sandbox::new("refuse-capability", false, FS_GRANTS, r#"{"capabilities": ["fs.read-text"]}"#);
    assert_refused_without_effect(&sandbox, COPY, &["capability-not-allowed"], &["fs.write-text at 2:3"]);
}

#[test]
fn a_computed_input_the_policy_does_not_allow_is_refused_before_anything_runs() {
    let policy = r#"{"capabilities": ["fs.read-text", "fs.write-text"], "computed_inputs": ["fs.read-text"]}"#;
    let sandbox = Sandbox::new("refuse-computed", false, FS_GRANTS, policy);
    let source = "(let [name (call fs.read-text {:path \"ROOT/names.txt\"})]\n  (call fs.write-text {:path name :text \"x\"}))";
    assert_refused_without_effect(&sandbox, source, &["computed-input-not-allowed"], &["fs.write-text at 2:3", "computed_inputs"]);
}

#[test]
fn more_write_sites_than_the_policy_allows_are_refused_before_anything_runs() {
    let policy = r#"{"capabilities": ["fs.read-text", "fs.write-text"], "max_write_sites": 1}"#;
    let sandbox = Sandbox::new("refuse-writes", false, FS_GRANTS, policy);
    let source = "(let [_ (call fs.write-text {:path \"ROOT/c.txt\" :text \"1\"})]\n  (call fs.write-text {:path \"ROOT/d.txt\" :text \"2\"}))";
    assert_refused_without_effect(&sandbox, source, &["too-many-write-sites"], &["2 write call sites", "fs.write-text at 1:", "allows 1"]);
}

#[test]
fn write_sites_are_counted_in_the_program_text_and_not_by_calls_made() {
    let policy = r#"{"capabilities": ["fs.write-text"], "computed_inputs": ["fs.write-text"], "max_write_sites": 1}"#;
    let sandbox = Sandbox::new("one-site", false, FS_GRANTS, policy);
    let mut client = sandbox.client();
    let source = program_for(
        &sandbox,
        "(let [_ (map (fn [name] (call fs.write-text {:path (str \"ROOT/\" name) :text name})) [\"p.txt\" \"q.txt\"])]\n  (count [\"p.txt\" \"q.txt\"]))",
    );
    let reply = client.program("run", &source);
    assert!(!reply.is_error, "{}", shown(&reply.structured));
    assert_eq!(capabilities_called(reply.structured.get("trace").expect("trace")), ["fs.write-text", "fs.write-text"]);
    assert!(sandbox.root().join("p.txt").exists() && sandbox.root().join("q.txt").exists());
}

#[test]
fn a_failure_part_way_returns_the_calls_made_before_it() {
    let policy = r#"{"capabilities": ["fs.write-text", "mcp-fx.fail-write"]}"#;
    let sandbox = Sandbox::new("fails", true, MIXED_GRANTS, policy);
    let mut client = sandbox.client();
    let source = program_for(
        &sandbox,
        "(let [_ (call fs.write-text {:path \"ROOT/c.txt\" :text \"created\"})]\n  (call mcp-fx.fail-write {}))",
    );
    let reply = client.program("run", &source);
    assert!(reply.is_error);
    assert_eq!(string_at(&reply.structured, &["outcome"]), "failed");
    assert!(string_at(&reply.structured, &["failure", "message"]).contains("disk on fire"), "{}", shown(&reply.structured));
    let trace = reply.structured.get("trace").expect("trace");
    assert_eq!(capabilities_called(trace), ["fs.write-text", "mcp-fx.fail-write"]);
    assert!(reply.text.contains("fs.write-text") && reply.text.contains("failed[Failed]"), "{}", reply.text);
    assert_eq!(fs::read_to_string(sandbox.root().join("c.txt")).expect("the first call stays done"), "created");
}

fn startup_failure(policy: &str, grants: &str) -> String {
    let sandbox = Sandbox::new("startup", false, grants, policy);
    let output = Command::new(SX).args(sandbox.arguments()).stdin(Stdio::null()).output().expect("sx runs");
    assert_eq!(output.status.code(), Some(64), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_policy_that_does_not_fit_the_declarations_is_a_usage_error() {
    let cases = [
        (r#"{"capabilities": ["fs.nope"]}"#, "which the declarations do not define"),
        (r#"{"capabilities": ["fs.read-text"], "computed_inputs": ["fs.write-text"]}"#, "which capabilities does not allow"),
        (r#"{"capabilities": ["fs.read-text"], "max_write_sites": -1}"#, "must not be negative"),
        (r#"{"capabilities": ["fs.read-text"], "max_write_sites": 1.5}"#, "must be a whole number"),
        (r#"{"capabilities": ["fs.read-text"], "writes": 1}"#, "unknown key"),
        (r#"{"computed_inputs": []}"#, "needs capabilities"),
        (r#"["fs.read-text"]"#, "must be a JSON object"),
    ];
    for (policy, expected) in cases {
        let stderr = startup_failure(policy, FS_GRANTS);
        assert!(stderr.contains("invalid policy") && stderr.contains(expected), "{policy}: {stderr}");
    }
}

#[test]
fn a_policy_must_be_given_and_an_mcp_tool_needs_its_server() {
    let sandbox = Sandbox::new("usage", false, FS_GRANTS, OPEN_POLICY);
    let without: Vec<String> = sandbox.arguments().into_iter().take(6).collect();
    let output = Command::new(SX).args(&without).stdin(Stdio::null()).output().expect("sx runs");
    assert_eq!(output.status.code(), Some(64));
    assert!(String::from_utf8_lossy(&output.stderr).contains("needs --policy FILE"));

    let proxied = Sandbox::new("usage-proxied", true, MIXED_GRANTS, r#"{"capabilities": ["mcp-fx.add"]}"#);
    let bare: Vec<String> = proxied.arguments().into_iter().take_while(|arg| arg != "--").collect();
    let output = Command::new(SX).args(&bare).stdin(Stdio::null()).output().expect("sx runs");
    assert_eq!(output.status.code(), Some(64));
    assert!(String::from_utf8_lossy(&output.stderr).contains("is an MCP tool; start its server"), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn an_unknown_tool_is_a_protocol_error_and_the_server_ends_cleanly() {
    let sandbox = Sandbox::new("unknown", false, FS_GRANTS, OPEN_POLICY);
    let mut client = sandbox.client();
    let params = Json::object([("name", Json::string("exec")), ("arguments", Json::Object(Vec::new()))]);
    let reply = client.request("tools/call", params);
    assert_eq!(reply.get("error").and_then(|e| e.get("code")).and_then(Json::as_i64), Some(-32602));
    let extra = client.tool("check", json(r#"{"program": "1", "inputs": {}}"#));
    assert_eq!(extra.get("isError").and_then(Json::as_bool), Some(true));
    let (code, stderr) = client.finish();
    assert_eq!((code, stderr.as_str()), (Some(0), ""));
}

const COPY_TS: &str =
    "const text = await tools.fs.readText({ path: \"ROOT/a.txt\" });\nawait tools.fs.writeText({ path: \"ROOT/b.txt\", text });\nreturn text.length;";

#[test]
fn typescript_mode_lists_the_declarations_in_place_of_the_sx_guide() {
    let sandbox = Sandbox::typescript("ts-listing", true, MIXED_GRANTS, r#"{"capabilities": ["fs.read-text", "mcp-fx.add"]}"#);
    let mut client = sandbox.client();
    let tools = client.list();
    let names: Vec<&str> = tools.iter().map(|tool| string_at(tool, &["name"])).collect();
    assert_eq!(names, ["check", "run"]);
    let check = string_at(&tools[0], &["description"]);
    let run = string_at(&tools[1], &["description"]);
    assert!(check.contains("the run tool's description") && !check.contains("interface Tools"), "{check}");
    for expected in [
        "checkable TypeScript",
        "type Int = number;",
        "interface CallError extends Error",
        "interface Tools {",
        "    /** Reads a whole file as UTF-8 text. [read] */\n    readText(input: { path: string }): Promise<string>;",
        "[write, idempotent]",
        "  mcpFx: {",
        "declare const tools: Tools;",
        "declare function retry<T>",
        "declare function loop<S extends object, R>",
        "tools.fs.readText, tools.mcpFx.add",
        "## Writing the program",
        "never `x === null`",
    ] {
        assert!(run.contains(expected), "{expected} missing from {run}");
    }
    for absent in ["(call ", "fs.read-text", "mcp-fx", "Coming from TypeScript"] {
        assert!(!run.contains(absent), "{absent} found in {run}");
    }
    for tool in &tools {
        let schema = shown(tool.get("inputSchema").expect("schema"));
        assert!(schema.contains(r#""required": ["program"]"#) && !schema.contains("inputs"), "{schema}");
    }
}

#[test]
fn typescript_check_explains_a_good_program_with_tool_names_and_performs_nothing() {
    let sandbox = Sandbox::typescript("ts-check-good", false, FS_GRANTS, OPEN_POLICY);
    let before = snapshot(&sandbox.root());
    let mut client = sandbox.client();
    let reply = client.program("check", &program_for(&sandbox, COPY_TS));
    assert!(!reply.is_error, "{}", shown(&reply.structured));
    assert_eq!(string_at(&reply.structured, &["policy", "verdict"]), "approved");
    assert!(reply.text.contains("tools.fs.readText  [read]") && reply.text.contains("tools.fs.writeText"), "{}", reply.text);
    assert_eq!(snapshot(&sandbox.root()), before);
}

#[test]
fn typescript_diagnostics_point_at_the_typescript_source() {
    let sandbox = Sandbox::typescript("ts-bad", false, FS_GRANTS, OPEN_POLICY);
    let before = snapshot(&sandbox.root());
    let mut client = sandbox.client();
    let cases = [
        (
            "const a = 1;\nawait tools.fs.writeText({ path: \"ROOT/c.txt\", content: \"x\" });",
            "unknown-field",
            2,
            "await tools.fs.writeText({ path: \"ROOT/c.txt\", content: \"x\" });",
        ),
        ("const a = 1;\nwhile (a < 3) {}\nreturn a;", "ts-while", 2, "while (a < 3) {}"),
        ("const r = await tools.fs.list({ path: \"ROOT\" });\nreturn r[0].nope;", "unknown-field", 2, "return r[0].nope;"),
        ("await tools.fs.nothing({ path: \"ROOT/a.txt\" });", "unknown-tool", 1, "await tools.fs.nothing"),
        ("await tools.fs.list({ path: \"ROOT\" });", "unauthorized", 1, "await tools.fs.list"),
        ("const n = null;\nreturn n === null;", "js-null-equality", 2, "return n === null;"),
    ];
    for tool in ["check", "run"] {
        for (source, code, line, excerpt) in cases {
            let reply = client.program(tool, &program_for(&sandbox, source));
            assert!(reply.is_error, "{tool}: {source}");
            let diagnostics = reply.structured.get("diagnostics").and_then(Json::as_array).expect("diagnostics");
            let found = diagnostics.iter().find(|d| string_at(d, &["code"]) == code);
            let found = found.unwrap_or_else(|| panic!("{code} missing from {}", shown(&reply.structured)));
            let start = found.get("span").and_then(|span| span.get("start")).expect("span");
            assert_eq!(start.get("line").and_then(Json::as_i64), Some(line), "{}", shown(found));
            assert_eq!(string_at(found, &["help"]), "typescript", "{}", shown(found));
            assert!(reply.text.contains(&format!("error[{code}] {line}:")), "{}", reply.text);
            assert!(reply.text.contains(&program_for(&sandbox, excerpt)), "{}", reply.text);
        }
    }
    assert_eq!(snapshot(&sandbox.root()), before);
}

#[test]
fn typescript_diagnostics_name_types_and_tools_in_typescript_terms() {
    let sandbox = Sandbox::typescript("ts-terms", false, FS_GRANTS, r#"{"capabilities": ["fs.read-text"]}"#);
    let mut client = sandbox.client();
    let reply = client.program("check", "const xs = await tools.fs.list({ path: \"/\" });\nconst n: number = xs;\nreturn xs.length + xs;");
    assert!(reply.is_error);
    assert!(!reply.text.contains("List<") && !reply.text.contains("Record {"), "{}", reply.text);
    let ungranted = client.program("check", "return await tools.fs.list({ path: \"/\" });");
    let text = ungranted.text;
    assert!(text.contains("may not call tools.fs.list.") && text.contains("allows: tools.fs.readText"), "{text}");
    assert!(!text.contains("fs.read-text"), "{text}");
}

#[test]
fn typescript_run_performs_native_calls_and_returns_result_logs_and_calls() {
    let sandbox = Sandbox::typescript("ts-native", false, FS_GRANTS, OPEN_POLICY);
    let mut client = sandbox.client();
    let source = "const text = await tools.fs.readText({ path: \"ROOT/a.txt\" });\nconsole.log(\"read\", text.length);\nawait tools.fs.writeText({ path: \"ROOT/b.txt\", text });\nconsole.log({ ok: true });\nreturn { size: text.length };";
    let reply = client.program("run", &program_for(&sandbox, source));
    assert!(!reply.is_error, "{}", shown(&reply.structured));
    assert_eq!(string_at(&reply.structured, &["outcome"]), "completed");
    assert_eq!(reply.structured.get("result").and_then(|r| r.get("size")).and_then(Json::as_i64), Some(6));
    assert_eq!(shown(reply.structured.get("logs").expect("logs")), r#"["read 6", "{\"ok\":true}"]"#);
    assert!(reply.text.contains("logs (2):\n  read 6\n"), "{}", reply.text);
    assert_eq!(capabilities_called(reply.structured.get("trace").expect("trace")), ["fs.read-text", "fs.write-text"]);
    assert!(reply.text.contains("calls (2):") && reply.text.contains("1. tools.fs.readText"), "{}", reply.text);
    assert_eq!(fs::read_to_string(sandbox.root().join("b.txt")).expect("written"), "alpha\n");
    let failed = client.program("run", "console.log(\"before\");\nthrow new Error(\"stop\");");
    assert!(failed.is_error);
    assert_eq!(string_at(&failed.structured, &["failure", "kind"]), "thrown");
    assert_eq!(shown(failed.structured.get("logs").expect("logs")), r#"["before"]"#);
}

#[test]
fn typescript_run_catches_a_failed_tool_call_as_a_call_error() {
    let sandbox = Sandbox::typescript("ts-catch", false, FS_GRANTS, OPEN_POLICY);
    let mut client = sandbox.client();
    let source = "try {\n  await tools.fs.readText({ path: \"ROOT/missing.txt\" });\n  return \"read\";\n} catch (e) {\n  return e.kind + \": \" + e.message.length;\n}";
    let reply = client.program("run", &program_for(&sandbox, source));
    assert!(!reply.is_error, "{}", shown(&reply.structured));
    assert!(string_at(&reply.structured, &["result"]).starts_with("NotFound: "), "{}", shown(&reply.structured));
}

#[test]
fn typescript_run_calls_a_proxied_mcp_server_next_to_native_tools() {
    let policy = r#"{"capabilities": ["fs.read-text", "mcp-fx.add"]}"#;
    let sandbox = Sandbox::typescript("ts-proxied", true, MIXED_GRANTS, policy);
    let mut client = sandbox.client();
    let source = "const text = await tools.fs.readText({ path: \"ROOT/a.txt\" });\nconst total = await tools.mcpFx.add({ a: 20, b: 22 });\nreturn { text, sum: total.sum };";
    let reply = client.program("run", &program_for(&sandbox, source));
    assert!(!reply.is_error, "{}", shown(&reply.structured));
    let result = reply.structured.get("result").expect("result");
    assert_eq!(result.get("sum").and_then(Json::as_i64), Some(42));
    assert_eq!(string_at(result, &["text"]), "alpha\n");
    let mut called = capabilities_called(reply.structured.get("trace").expect("trace"));
    called.sort();
    assert_eq!(called, ["fs.read-text", "mcp-fx.add"]);
}

#[test]
fn typescript_reads_a_payload_enum_and_a_result_as_the_declarations_say() {
    let sandbox = Sandbox::typescript("ts-variants", true, r#"{"mcp-fx.variants": null}"#, r#"{"capabilities": ["mcp-fx.variants"]}"#);
    let mut client = sandbox.client();
    let run = string_at(&client.list()[1], &["description"]).to_string();
    assert!(run.contains("type Settled<T, E> = { status: \"fulfilled\"; value: T } | { status: \"rejected\"; reason: E };"), "{run}");
    assert!(run.contains("outcome: Settled<Int, string>"), "{run}");
    assert!(run.contains("{ tag: \"Circle\"; value: Float }"), "{run}");
    let source = "const r = await tools.mcpFx.variants({});\nconst shape = r.shape;\nconst outcome = r.outcome.status === \"fulfilled\" ? r.outcome.value : -1;\nconst fail = r.fail.status === \"rejected\" ? r.fail.reason : \"ok\";\nreturn { shape, outcome, fail, whole: r.outcome };";
    let reply = client.program("run", source);
    assert!(!reply.is_error, "{}\n{}", shown(&reply.structured), reply.text);
    let result = reply.structured.get("result").expect("result");
    assert_eq!(shown(result.get("shape").expect("shape")), r#"{"tag": "Circle", "value": 2.5}"#);
    assert_eq!(result.get("outcome").and_then(Json::as_i64), Some(3));
    assert_eq!(string_at(result, &["fail"]), "nope");
    assert_eq!(shown(result.get("whole").expect("whole")), r#"{"status": "fulfilled", "value": 3}"#);
}

#[test]
fn typescript_tells_the_variants_of_a_payload_enum_apart_by_tag() {
    let sandbox = Sandbox::typescript("ts-variant-tags", true, r#"{"mcp-fx.variants": null}"#, r#"{"capabilities": ["mcp-fx.variants"]}"#);
    let mut client = sandbox.client();
    let source = "const s = (await tools.mcpFx.variants({})).shape;\nconst label = s === \"Dot\" ? \"dot\" : s.tag === \"Circle\" ? \"circle \" + s.value : \"square\";\nif (s === \"Dot\" || s.tag !== \"Circle\") { return { label, radius: -1.5, tag: \"none\", kind: \"string\" }; }\nreturn { label, radius: s.value, tag: s.tag, kind: typeof s };";
    let reply = client.program("run", source);
    assert!(!reply.is_error, "{}\n{}", shown(&reply.structured), reply.text);
    let result = reply.structured.get("result").expect("result");
    assert_eq!(string_at(result, &["label"]), "circle 2.5");
    assert_eq!(shown(result.get("radius").expect("radius")), "2.5");
    assert_eq!(string_at(result, &["tag"]), "Circle");
    assert_eq!(string_at(result, &["kind"]), "object");
}

#[test]
fn typescript_policy_refusals_name_tools_and_typescript_positions_and_leave_the_sandbox_untouched() {
    let only_reads = Sandbox::typescript("ts-refuse-capability", false, FS_GRANTS, r#"{"capabilities": ["fs.read-text"]}"#);
    assert_refused_without_effect(&only_reads, COPY_TS, &["capability-not-allowed"], &["tools.fs.writeText at 2:7"]);

    let policy = r#"{"capabilities": ["fs.read-text", "fs.write-text"], "computed_inputs": ["fs.read-text"]}"#;
    let computed = Sandbox::typescript("ts-refuse-computed", false, FS_GRANTS, policy);
    let source = "const name = await tools.fs.readText({ path: \"ROOT/names.txt\" });\nawait tools.fs.writeText({ path: name, text: \"x\" });";
    assert_refused_without_effect(&computed, source, &["computed-input-not-allowed"], &["tools.fs.writeText at 2:7", "computed_inputs"]);

    let policy = r#"{"capabilities": ["fs.read-text", "fs.write-text"], "max_write_sites": 1}"#;
    let many = Sandbox::typescript("ts-refuse-writes", false, FS_GRANTS, policy);
    let source = "await tools.fs.writeText({ path: \"ROOT/c.txt\", text: \"1\" });\nawait tools.fs.writeText({ path: \"ROOT/d.txt\", text: \"2\" });";
    assert_refused_without_effect(&many, source, &["too-many-write-sites"], &["2 write call sites", "tools.fs.writeText at 1:7", "allows 1"]);
}

#[test]
fn typescript_mode_takes_no_inputs_and_an_unknown_language_is_a_usage_error() {
    let sandbox = Sandbox::typescript("ts-inputs", false, FS_GRANTS, OPEN_POLICY);
    let mut client = sandbox.client();
    let extra = client.tool("run", json(r#"{"program": "return 1;", "inputs": {}}"#));
    assert_eq!(extra.get("isError").and_then(Json::as_bool), Some(true));
    let plain = Sandbox::new("lang-usage", false, FS_GRANTS, OPEN_POLICY);
    let mut args = plain.arguments();
    args.extend(["--language".to_string(), "python".to_string()]);
    let output = Command::new(SX).args(&args).stdin(Stdio::null()).output().expect("sx runs");
    assert_eq!(output.status.code(), Some(64));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--language takes sx or ts"), "{}", String::from_utf8_lossy(&output.stderr));
}
