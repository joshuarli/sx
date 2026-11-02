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
const WAIT: Duration = Duration::from_secs(20);

static SERIAL: AtomicUsize = AtomicUsize::new(0);

fn json(text: &str) -> Json {
    parse(text).unwrap_or_else(|e| panic!("{e}: {text}"))
}

fn shown(value: &Json) -> String {
    dumps(value, Layout::Spaced).expect("serializable")
}

struct Workspace {
    base: PathBuf,
}

impl Workspace {
    fn new(label: &str, declarations: &str, grants: &str) -> Workspace {
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("mcp-serve-cli-{}-{label}-{serial}", std::process::id()));
        if base.exists() {
            fs::remove_dir_all(&base).expect("old workspace removed");
        }
        fs::create_dir_all(base.join("root")).expect("workspace");
        let base = fs::canonicalize(base).expect("canonical workspace");
        let root = base.join("root");
        let substitute = |text: &str| text.replace("ROOT", root.to_str().expect("UTF-8 path"));
        fs::write(base.join("declarations.json"), substitute(declarations)).expect("declarations");
        fs::write(base.join("grants.json"), substitute(grants)).expect("grants");
        Workspace { base }
    }

    fn root(&self) -> PathBuf {
        self.base.join("root")
    }

    fn arguments(&self) -> Vec<String> {
        let at = |name: &str| self.base.join(name).to_str().expect("UTF-8 path").to_string();
        ["mcp", "serve", "--declarations", &at("declarations.json"), "--grants", &at("grants.json")]
            .map(str::to_string)
            .to_vec()
    }
}

struct Client {
    child: Child,
    input: Option<ChildStdin>,
    lines: Receiver<Json>,
    errors: Receiver<String>,
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
        Client { child, input, lines, errors }
    }

    fn send(&mut self, text: &str) {
        let input = self.input.as_mut().expect("stdin is open");
        input.write_all(format!("{text}\n").as_bytes()).and_then(|()| input.flush()).expect("written");
    }

    fn next(&self, within: Duration) -> Option<Json> {
        self.lines.recv_timeout(within).ok()
    }

    fn initialize(&mut self) {
        self.send(r#"{"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}}"#);
        let reply = self.next(WAIT).expect("initialize answered");
        assert_eq!(reply.get("id").and_then(Json::as_i64), Some(0));
        self.send(r#"{"jsonrpc": "2.0", "method": "notifications/initialized"}"#);
    }

    fn call(&mut self, id: i64, name: &str, arguments: &str) {
        self.send(&format!(
            r#"{{"jsonrpc": "2.0", "id": {id}, "method": "tools/call", "params": {{"name": "{name}", "arguments": {arguments}}}}}"#
        ));
    }

    fn close(&mut self) {
        self.input.take();
    }

    fn finish(mut self) -> (Option<i32>, String) {
        self.close();
        let deadline = Instant::now() + WAIT;
        let status = loop {
            if let Some(status) = self.child.try_wait().expect("waited") {
                break status.code();
            }
            assert!(Instant::now() < deadline, "sx mcp serve did not exit");
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

fn program(candidates: &[&str]) -> String {
    candidates.iter().find(|path| Path::new(path).exists()).expect("a system program").to_string()
}

fn process_workspace(label: &str) -> (Workspace, String, String) {
    let sleep = program(&["/bin/sleep", "/usr/bin/sleep"]);
    let shell = program(&["/bin/sh"]);
    let grants = format!(
        r#"{{"process.run": {{"commands": [{{"program": "{sleep}", "args": ["AnyOne"]}}, {{"program": "{shell}", "args": ["Rest"]}}], "roots": ["ROOT"]}}}}"#
    );
    (Workspace::new(label, r#"{"packs": ["process"]}"#, &grants), sleep, shell)
}

#[test]
fn a_capability_without_a_native_implementation_is_not_served() {
    let w = Workspace::new(
        "unserved",
        r#"{"packs": ["fs", "http"], "capabilities": [{"id": "demo.echo", "effect": "read", "input": "Record {}", "output": "String", "description": "Echoes."}]}"#,
        r#"{"fs.read-text": {"roots": ["ROOT"]}, "demo.echo": null}"#,
    );
    let mut client = Client::start(&w.arguments());
    client.initialize();
    client.send(r#"{"jsonrpc": "2.0", "id": 1, "method": "tools/list"}"#);
    let listed = client.next(WAIT).expect("listed");
    let names: Vec<String> = listed
        .get("result")
        .and_then(|r| r.get("tools"))
        .and_then(Json::as_array)
        .expect("tools")
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Json::as_str).map(str::to_string))
        .collect();
    assert_eq!(names, ["fs.read-text"]);
    let (code, stderr) = client.finish();
    assert_eq!(code, Some(0));
    assert!(stderr.contains("not serving demo.echo"), "{stderr}");
}

#[test]
fn nothing_to_serve_is_a_usage_error() {
    let w = Workspace::new("empty", r#"{"packs": ["fs"]}"#, "{}");
    let output = Command::new(SX).args(w.arguments()).stdin(Stdio::null()).output().expect("sx runs");
    assert_eq!(output.status.code(), Some(64));
    assert!(String::from_utf8_lossy(&output.stderr).contains("no granted capability can be served"));
}

#[test]
fn calls_run_concurrently_and_each_answer_carries_its_own_id() {
    let (w, sleep, _) = process_workspace("concurrent");
    let mut client = Client::start(&w.arguments());
    client.initialize();
    let started = Instant::now();
    let calls = 8;
    for id in 1..=calls {
        let arguments = format!(
            r#"{{"program": "{sleep}", "args": ["0.{id}"], "cwd": "{}", "stdin": null}}"#,
            w.root().display()
        );
        client.call(id, "process.run", &arguments);
    }
    let mut seen = Vec::new();
    for _ in 0..calls {
        let reply = client.next(WAIT).expect("an answer");
        let result = reply.get("result").expect("a result");
        assert_eq!(result.get("isError").and_then(Json::as_bool), Some(false), "{}", shown(&reply));
        seen.push(reply.get("id").and_then(Json::as_i64).expect("id"));
    }
    let elapsed = started.elapsed();
    let sequential: f64 = (1..=calls).map(|id| f64::from(i32::try_from(id).expect("small")) / 10.0).sum();
    assert!(elapsed.as_secs_f64() < sequential / 2.0, "{elapsed:?} against {sequential}s run one after another");
    assert_eq!(seen[0], 1, "the shortest sleep answers first: {seen:?}");
    seen.sort_unstable();
    assert_eq!(seen, (1..=calls).collect::<Vec<_>>());
}

#[test]
fn a_client_that_disconnects_leaves_the_server_to_finish_the_call_and_exit() {
    let (w, _, shell) = process_workspace("disconnect");
    let mut client = Client::start(&w.arguments());
    client.initialize();
    let arguments = format!(
        r#"{{"program": "{shell}", "args": ["-c", "sleep 0.4; echo finished > marker"], "cwd": "{}", "stdin": null}}"#,
        w.root().display()
    );
    client.call(1, "process.run", &arguments);
    client.close();
    let (code, _) = client.finish();
    assert_eq!(code, Some(0));
    assert_eq!(fs::read_to_string(w.root().join("marker")).expect("the call finished").trim(), "finished");
}

#[test]
fn a_client_that_disconnects_idle_ends_the_server_cleanly() {
    let (w, ..) = process_workspace("idle");
    let mut client = Client::start(&w.arguments());
    client.initialize();
    let (code, stderr) = client.finish();
    assert_eq!((code, stderr.as_str()), (Some(0), ""));
}

#[test]
fn a_cancelled_call_gets_no_answer_and_the_connection_keeps_serving() {
    let (w, sleep, _) = process_workspace("cancel");
    let mut client = Client::start(&w.arguments());
    client.initialize();
    let arguments = format!(r#"{{"program": "{sleep}", "args": ["0.6"], "cwd": "{}", "stdin": null}}"#, w.root().display());
    client.call(1, "process.run", &arguments);
    client.send(r#"{"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 1}}"#);
    client.send(r#"{"jsonrpc": "2.0", "id": 2, "method": "ping"}"#);
    let reply = client.next(WAIT).expect("the ping answered");
    assert_eq!(reply.get("id").and_then(Json::as_i64), Some(2));
    assert!(client.next(Duration::from_millis(1200)).is_none(), "a cancelled call was answered");
}

const ROUND_TRIP: &str = r#"{
  "packs": ["fs"],
  "enums": {
    "mcp-rt.RtEchoInputMode": {"Fast": null, "Slow": null},
    "mcp-rt.RtEchoInputNestedKind": {"A": null, "B": null},
    "mcp-rt.RtEchoOutputState": {"Ok": null, "Bad": null}
  },
  "capabilities": [
    {"id": "rt.echo", "effect": "write",
     "http": {"method": "POST", "url": "http://127.0.0.1:9/echo"},
     "input": "Record {name: String, count: Int, ratio: Float, flag: Bool, tags: List<String>, pair: Tuple<Int, String>, labels: Map<String, Int>, mode: mcp-rt.RtEchoInputMode, note: Option<String>, nested: Record {kind: mcp-rt.RtEchoInputNestedKind, b: Option<List<Int>>}}",
     "output": "Record {id: Int, state: mcp-rt.RtEchoOutputState, maybe: Option<String>, rows: List<Record {k: String, v: Option<Int>}>, extra: Map<String, String>}"},
    {"id": "rt.peek", "effect": "read",
     "http": {"method": "GET", "url": "http://127.0.0.1:9/peek/{id}", "query": ["verbose"]},
     "input": "Record {id: Int, verbose: Option<Bool>}",
     "output": "Record open {value: String, size: Option<Int>}"},
    {"id": "rt.put", "effect": "write", "idempotent": true,
     "http": {"method": "PUT", "url": "http://127.0.0.1:9/put"},
     "input": "Record {key: String, value: String}",
     "output": "String"}
  ]
}"#;

const ROUND_TRIP_GRANTS: &str = r#"{
  "rt.echo": {"origins": ["http://127.0.0.1:9"], "methods": ["POST"]},
  "rt.peek": {"origins": ["http://127.0.0.1:9"], "methods": ["GET"]},
  "rt.put": {"origins": ["http://127.0.0.1:9"], "methods": ["PUT"]},
  "fs.read-text": {"roots": ["ROOT"]},
  "fs.write-text": {"roots": ["ROOT"]}
}"#;

#[test]
fn importing_what_sx_serves_reproduces_the_declared_types() {
    let w = Workspace::new("round-trip", ROUND_TRIP, ROUND_TRIP_GRANTS);
    let declared = json(&fs::read_to_string(w.base.join("declarations.json")).expect("declarations"));
    let mut command = vec!["mcp".to_string(), "import".into(), "--name".into(), "rt".into(), "--".into(), SX.into()];
    command.extend(w.arguments());
    let imported = Command::new(SX).args(&command).stdin(Stdio::null()).output().expect("sx runs");
    let stderr = String::from_utf8_lossy(&imported.stderr).into_owned();
    assert_eq!(imported.status.code(), Some(0), "{stderr}");
    assert!(stderr.contains("skipped fs.write-text"), "a Unit result has no Sx type on import: {stderr}");
    let imported = json(&String::from_utf8(imported.stdout).expect("UTF-8"));
    let find = |declarations: &Json, id: &str| {
        declarations
            .get("capabilities")
            .and_then(Json::as_array)
            .and_then(|all| all.iter().find(|c| c.get("id").and_then(Json::as_str) == Some(id)))
            .cloned()
            .unwrap_or_else(|| panic!("{id} is not declared"))
    };
    for (id, imported_id) in [("rt.echo", "mcp-rt.rt-echo"), ("rt.peek", "mcp-rt.rt-peek"), ("rt.put", "mcp-rt.rt-put")] {
        let (original, round) = (find(&declared, id), find(&imported, imported_id));
        for key in ["input", "output", "effect"] {
            assert_eq!(shown(original.get(key).expect(key)), shown(round.get(key).expect(key)), "{id} {key}");
        }
        assert_eq!(
            original.get("idempotent").and_then(Json::as_bool).unwrap_or(false),
            round.get("idempotent").and_then(Json::as_bool).unwrap_or(false),
            "{id} idempotent"
        );
    }
    let read = find(&imported, "mcp-rt.fs-read-text");
    assert_eq!(read.get("output").and_then(Json::as_str), Some("String"));
    assert_eq!(shown(read.get("mcp").expect("mcp")), r#"{"server": "rt", "tool": "fs.read-text", "result": "text"}"#);
}

#[test]
fn a_program_runs_against_what_sx_serves_with_the_same_result_as_natively() {
    let w = Workspace::new("program", ROUND_TRIP, ROUND_TRIP_GRANTS);
    fs::write(w.root().join("note.txt"), "inside").expect("note");
    let mut command = vec!["mcp".to_string(), "import".into(), "--name".into(), "rt".into(), "--".into(), SX.into()];
    command.extend(w.arguments());
    let imported = Command::new(SX).args(&command).stdin(Stdio::null()).output().expect("sx runs");
    fs::write(w.base.join("imported.json"), &imported.stdout).expect("imported declarations");
    let source = format!("(call mcp-rt.fs-read-text {{:path \"{}\"}})", w.root().join("note.txt").display());
    fs::write(w.base.join("program.sx"), source).expect("program");
    let mut args: Vec<String> = ["mcp", "run"].map(str::to_string).to_vec();
    args.extend([
        w.base.join("program.sx").to_str().expect("UTF-8").to_string(),
        "--declarations".into(),
        w.base.join("imported.json").to_str().expect("UTF-8").to_string(),
        "--allow".into(),
        "mcp-rt.fs-read-text".into(),
        "--format".into(),
        "json".into(),
        "--".into(),
        SX.into(),
    ]);
    args.extend(w.arguments());
    let ran = Command::new(SX).args(&args).stdin(Stdio::null()).output().expect("sx runs");
    let report = json(&String::from_utf8(ran.stdout).expect("UTF-8"));
    assert_eq!(report.get("result").and_then(Json::as_str), Some("inside"), "{}", String::from_utf8_lossy(&ran.stderr));
}

#[test]
fn describe_lists_the_served_capabilities_with_the_named_types_they_use() {
    let w = Workspace::new(
        "describe",
        r#"{"packs": ["fs", "http"]}"#,
        r#"{"fs.list": {"roots": ["ROOT"]}, "fs.stat": {"roots": ["ROOT"]}}"#,
    );
    let mut args = w.arguments();
    args[1] = "describe".to_string();
    let output = Command::new(SX).args(&args).stdin(Stdio::null()).output().expect("sx runs");
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    let described = json(&String::from_utf8(output.stdout).expect("UTF-8"));
    assert_eq!(
        shown(described.get("enums").expect("enums")),
        r#"{"fs.Kind": {"File": null, "Directory": null, "Symlink": null, "Other": null}}"#
    );
    assert_eq!(
        shown(described.get("types").expect("types")),
        r#"{"fs.Entry": "Record {name: String, kind: fs.Kind}", "fs.Stat": "Record {kind: fs.Kind, size: Int}"}"#
    );
    let ids: Vec<&str> = described
        .get("capabilities")
        .and_then(Json::as_array)
        .expect("capabilities")
        .iter()
        .filter_map(|c| c.get("id").and_then(Json::as_str))
        .collect();
    assert_eq!(ids, ["fs.list", "fs.stat"]);
}

fn exported(w: &Workspace) -> (Json, String) {
    let at = |name: &str| w.base.join(name).to_str().expect("UTF-8 path").to_string();
    let args = ["mcp", "export", "--declarations", &at("declarations.json")];
    let output = Command::new(SX).args(args).stdin(Stdio::null()).output().expect("sx runs");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert_eq!(output.status.code(), Some(0), "{stderr}");
    (json(&String::from_utf8(output.stdout).expect("UTF-8")), stderr)
}

fn tool_named<'a>(tools: &'a [Json], name: &str) -> &'a Json {
    tools.iter().find(|tool| tool.get("name").and_then(Json::as_str) == Some(name)).unwrap_or_else(|| panic!("{name} is not listed"))
}

#[test]
fn export_lists_what_serve_lists_and_names_each_result_shape() {
    let w = Workspace::new("export", ROUND_TRIP, ROUND_TRIP_GRANTS);
    let mut client = Client::start(&w.arguments());
    client.initialize();
    client.send(r#"{"jsonrpc": "2.0", "id": 1, "method": "tools/list"}"#);
    let served = client.next(WAIT).expect("listed");
    let served = served.get("result").and_then(|r| r.get("tools")).and_then(Json::as_array).expect("tools").to_vec();
    drop(client);

    let (listing, _) = exported(&w);
    let tools = listing.get("tools").and_then(Json::as_array).expect("tools");
    assert!(tools.len() > served.len(), "export lists the capabilities nobody granted too");
    assert!(!served.is_empty());
    for tool in &served {
        let name = tool.get("name").and_then(Json::as_str).expect("name");
        assert_eq!(shown(tool_named(tools, name)), shown(tool), "{name}");
    }
    let names: Vec<&str> = tools.iter().filter_map(|tool| tool.get("name").and_then(Json::as_str)).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted);

    let shapes = listing.get("shapes").expect("shapes");
    let shape = |id: &str| shapes.get(id).and_then(Json::as_str).unwrap_or_else(|| panic!("no shape for {id}")).to_string();
    assert_eq!(shape("rt.put"), "text");
    assert_eq!(shape("rt.echo"), "structured");
    assert_eq!(shape("fs.list"), "wrapped");
    assert_eq!(shapes.as_object().map(<[_]>::len), Some(names.len()));
}

#[test]
fn importing_an_exported_listing_reads_back_what_importing_the_server_reads() {
    let w = Workspace::new("export-import", ROUND_TRIP, ROUND_TRIP_GRANTS);
    let (listing, _) = exported(&w);
    fs::write(w.base.join("listing.json"), shown(&listing)).expect("listing");
    let import = |tail: Vec<String>| {
        let mut args = vec!["mcp".to_string(), "import".into(), "--name".into(), "rt".into()];
        args.extend(tail);
        let done = Command::new(SX).args(&args).stdin(Stdio::null()).output().expect("sx runs");
        assert_eq!(done.status.code(), Some(0), "{}", String::from_utf8_lossy(&done.stderr));
        json(&String::from_utf8(done.stdout).expect("UTF-8"))
    };
    let listing_path = w.base.join("listing.json").to_str().expect("UTF-8 path").to_string();
    let from_listing = import(vec!["--tools".into(), listing_path]);
    let mut command = vec!["--".to_string(), SX.into()];
    command.extend(w.arguments());
    let from_server = import(command);

    let capabilities = |declarations: &Json| declarations.get("capabilities").and_then(Json::as_array).expect("capabilities").to_vec();
    let (listed, served) = (capabilities(&from_listing), capabilities(&from_server));
    let id_of = |capability: &Json| capability.get("id").and_then(Json::as_str).expect("id").to_string();
    assert!(served.len() >= 4);
    for capability in &served {
        let id = id_of(capability);
        let read_back = listed.iter().find(|candidate| id_of(candidate) == id).unwrap_or_else(|| panic!("{id} was not imported"));
        for key in ["effect", "idempotent", "input", "output", "mcp"] {
            assert_eq!(read_back.get(key).map(shown), capability.get(key).map(shown), "{id} {key}");
        }
    }
    let declared = json(&fs::read_to_string(w.base.join("declarations.json")).expect("declarations"));
    let original = |id: &str| capabilities(&declared).into_iter().find(|c| id_of(c) == id).expect("declared");
    for (id, imported) in [("rt.echo", "mcp-rt.rt-echo"), ("rt.peek", "mcp-rt.rt-peek"), ("rt.put", "mcp-rt.rt-put")] {
        let read_back = listed.iter().find(|c| id_of(c) == imported).expect("imported");
        for key in ["input", "output", "effect"] {
            assert_eq!(shown(original(id).get(key).expect(key)), shown(read_back.get(key).expect(key)), "{id} {key}");
        }
    }
}

#[test]
fn export_names_a_capability_it_cannot_describe_and_lists_the_rest() {
    let w = Workspace::new(
        "export-skip",
        r#"{"capabilities": [
            {"id": "demo.echo", "effect": "read", "input": "Record {}", "output": "String", "description": "Echoes."},
            {"id": "demo.bare", "effect": "read", "input": "String", "output": "String", "description": "Takes a bare string."}
        ]}"#,
        "{}",
    );
    let (listing, stderr) = exported(&w);
    let tools = listing.get("tools").and_then(Json::as_array).expect("tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].get("name").and_then(Json::as_str), Some("demo.echo"));
    assert!(stderr.contains("not exporting demo.bare"), "{stderr}");
}
