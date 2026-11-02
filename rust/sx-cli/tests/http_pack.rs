#[path = "../../sx-packs/tests/support/mod.rs"]
mod support;

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use support::{Reply, Server};
use sx_kernel::json::{Json, Layout, dumps, parse};

static SERIAL: AtomicUsize = AtomicUsize::new(0);

struct Workspace {
    base: PathBuf,
}

#[derive(Debug)]
struct Ran {
    code: i32,
    stdout: String,
    stderr: String,
}

fn shown(json: &Json) -> String {
    dumps(json, Layout::Spaced).expect("serializable")
}

fn text(path: &Path) -> &str {
    path.to_str().expect("test paths are UTF-8")
}

impl Workspace {
    fn new(label: &str, declarations: &str, grants: &str) -> Workspace {
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("http-pack-{}-{label}-{serial}", std::process::id()));
        if base.exists() {
            fs::remove_dir_all(&base).expect("old workspace removed");
        }
        fs::create_dir_all(&base).expect("workspace");
        fs::write(base.join("declarations.json"), declarations).expect("declarations");
        fs::write(base.join("grants.json"), grants).expect("grants");
        Workspace { base }
    }

    fn sx(&self, command: &str, source: &str, extra: &[&str]) -> Ran {
        let program = self.base.join(format!("program-{}.sx", SERIAL.fetch_add(1, Ordering::Relaxed)));
        fs::write(&program, source).expect("program");
        let declarations = self.base.join("declarations.json");
        let grants = self.base.join("grants.json");
        let mut args = vec![command, text(&program), "--declarations", text(&declarations), "--grants", text(&grants)];
        args.extend(extra);
        let output = Command::new(env!("CARGO_BIN_EXE_sx")).args(&args).output().expect("sx runs");
        Ran {
            code: output.status.code().expect("exit status"),
            stdout: String::from_utf8(output.stdout).expect("utf-8"),
            stderr: String::from_utf8(output.stderr).expect("utf-8"),
        }
    }

    fn result(&self, source: &str) -> Json {
        let ran = self.sx("run", source, &["--format", "json"]);
        let outcome = parse(&ran.stdout).unwrap_or_else(|e| panic!("{e}: {ran:?}"));
        assert_eq!(outcome.get("outcome").and_then(Json::as_str), Some("completed"), "{ran:?}");
        outcome.get("result").cloned().expect("a result")
    }

    fn explain(&self, source: &str) -> Json {
        let ran = self.sx("explain", source, &["--format", "json"]);
        assert_eq!(ran.code, 0, "{ran:?}");
        parse(&ran.stdout).unwrap_or_else(|e| panic!("{e}: {ran:?}"))
    }
}

fn declarations(origin: &str) -> String {
    format!(
        r#"{{
  "packs": ["http"],
  "capabilities": [
    {{"id": "github.repo", "http": {{"method": "GET", "url": "{origin}/repos/{{owner}}/{{name}}", "query": [], "headers": {{"accept": "application/vnd.github+json"}}}},
      "input": "Record {{owner: String, name: String}}", "output": "Record open {{full_name: String, stargazers_count: Int}}", "effect": "read"}},
    {{"id": "github.issues", "http": {{"method": "GET", "url": "{origin}/repos/{{owner}}/{{name}}/issues", "query": ["state"]}},
      "input": "Record {{owner: String, name: String, state: String}}", "output": "List<Record open {{number: Int, title: String}}>", "effect": "read"}},
    {{"id": "github.open-issue", "http": {{"method": "POST", "url": "{origin}/repos/{{owner}}/{{name}}/issues"}},
      "input": "Record {{owner: String, name: String, title: String}}", "output": "Record open {{number: Int}}", "effect": "write"}}
  ]
}}"#
    )
}

fn grants(origin: &str, methods: &str) -> String {
    let scope = format!(r#"{{"origins": ["{origin}"], "methods": [{methods}]}}"#);
    format!(r#"{{"github.repo": {scope}, "github.issues": {scope}, "github.open-issue": {scope}, "http.request": {scope}, "http.get": {scope}}}"#)
}

fn github(seen: &support::Seen) -> Reply {
    match (seen.method.as_str(), seen.target.as_str()) {
        ("GET", "/repos/acme/widget") => Reply::json(r#"{"full_name": "acme/widget", "stargazers_count": 42, "id": 9}"#),
        ("GET", "/repos/acme/widget/issues?state=open") => Reply::json(r#"[{"number": 1, "title": "one", "user": {}}]"#),
        ("POST", "/repos/acme/widget/issues") => Reply::json(r#"{"number": 2}"#),
        ("GET", "/repos/acme/missing") => Reply::status(404, r#"{"message": "Not Found"}"#),
        _ => Reply::status(400, "unexpected"),
    }
}

#[test]
fn typed_endpoints_and_raw_requests_run_end_to_end() {
    let server = Server::start(github);
    let origin = server.origin();
    let w = Workspace::new("end-to-end", &declarations(&origin), &grants(&origin, r#""GET", "POST""#));
    let repo = w.result(r#"(call github.repo {:owner "acme" :name "widget"})"#);
    assert_eq!(shown(&repo), r#"{"full_name": "acme/widget", "stargazers_count": 42}"#);
    let issues = w.result(r#"(call github.issues {:owner "acme" :name "widget" :state "open"})"#);
    assert_eq!(shown(&issues), r#"[{"number": 1, "title": "one"}]"#);
    let opened = w.result(r#"(call github.open-issue {:owner "acme" :name "widget" :title "hi"})"#);
    assert_eq!(shown(&opened), r#"{"number": 2}"#);
    let missing = w.result(r#"(attempt (call github.repo {:owner "acme" :name "missing"}))"#);
    assert_eq!(missing.get("err").and_then(|e| e.get("kind")).and_then(Json::as_str), Some("NotFound"));
    let raw = w.result(&format!(r#"(let [r (call http.get {{:url "{origin}/repos/acme/widget" :headers {{}}}})] [(get r :status) (get (get r :headers) "content-type" "none")])"#));
    assert_eq!(shown(&raw), r#"[200, "application/json"]"#);
    let posted = w.result(&format!(
        r#"(call http.request {{:method "POST" :url "{origin}/repos/acme/widget/issues" :headers {{}} :body (some "{{}}")}})"#
    ));
    assert_eq!(posted.get("status").and_then(Json::as_i64), Some(200));
    assert_eq!(posted.get("body").and_then(Json::as_str), Some(r#"{"number": 2}"#));
    let sent = server.requests();
    assert_eq!(sent[0].header("accept"), Some("application/vnd.github+json"));
    assert_eq!(sent[2].text(), r#"{"title": "hi"}"#);
}

#[test]
fn a_literal_call_outside_its_grant_is_rejected_before_any_request() {
    let server = Server::start(github);
    let origin = server.origin();
    let elsewhere = Server::start(github);
    let w = Workspace::new("static", &declarations(&elsewhere.origin()), &grants(&origin, r#""GET""#));
    let program = r#"(call github.repo {:owner "acme" :name "widget"})"#;
    let checked = w.sx("check", program, &["--format", "json"]);
    assert_eq!(checked.code, 1, "{checked:?}");
    let diagnostics = parse(&checked.stdout).expect("JSON");
    let first = diagnostics.get("diagnostics").and_then(Json::as_array).and_then(<[Json]>::first).expect("a diagnostic");
    assert_eq!(first.get("code").and_then(Json::as_str), Some("out-of-scope"));
    let message = first.get("message").and_then(Json::as_str).expect("message");
    assert!(message.contains("outside every granted origin"), "{message}");
    let ran = w.sx("run", program, &["--format", "json"]);
    assert_eq!(ran.code, 1, "{ran:?}");
    assert!(ran.stderr.is_empty(), "{ran:?}");
    assert_eq!((server.connections(), elsewhere.connections()), (0, 0));
}

#[test]
fn a_literal_call_outside_its_granted_methods_is_rejected() {
    let server = Server::start(github);
    let w = Workspace::new("methods", &declarations(&server.origin()), &grants(&server.origin(), r#""POST""#));
    let checked = w.sx("check", r#"(call github.repo {:owner "acme" :name "widget"})"#, &["--format", "json"]);
    assert_eq!(checked.code, 1, "{checked:?}");
    assert!(checked.stdout.contains("method GET is not granted"), "{checked:?}");
    let accepted = w.sx("check", r#"(call github.open-issue {:owner "acme" :name "widget" :title "t"})"#, &["--format", "json"]);
    assert_eq!(accepted.code, 0, "{accepted:?}");
}

#[test]
fn explain_shows_a_verdict_for_every_call_site() {
    let server = Server::start(github);
    let origin = server.origin();
    let w = Workspace::new("explain", &declarations(&origin), &grants(&origin, r#""GET""#));
    let program = format!(
        r#"(let [a (call github.repo {{:owner "acme" :name "widget"}})
               b (call github.open-issue {{:owner "acme" :name "widget" :title "t"}})
               c (call http.get {{:url (str "{origin}" "/repos/acme/widget") :headers {{}}}})]
           a)"#
    );
    let explained = w.explain(&program);
    let tools = explained.get("tools").and_then(Json::as_array).expect("tools");
    let verdicts = |id: &str| {
        let tool = tools.iter().find(|t| t.get("id").and_then(Json::as_str) == Some(id)).expect("tool");
        shown(tool.get("verdicts").expect("verdicts"))
    };
    assert_eq!(verdicts("github.repo"), r#"[{"verdict": "allowed"}]"#);
    assert!(verdicts("github.open-issue").contains("method POST is not granted"), "{}", verdicts("github.open-issue"));
    assert_eq!(verdicts("http.get"), r#"[{"verdict": "checked per request"}]"#);
    let text = w.sx("explain", &program, &[]);
    assert!(text.stdout.contains("scope: denied: method POST is not granted"), "{text:?}");
}

#[test]
fn a_computed_url_outside_the_grant_is_refused_without_a_socket() {
    let inside = Server::start(github);
    let outside = Server::start(github);
    let w = Workspace::new("computed", &declarations(&inside.origin()), &grants(&inside.origin(), r#""GET""#));
    let outcome = w.result(&format!(
        r#"(attempt (call http.get {{:url (str "{}" "/repos/acme/widget") :headers {{}}}}))"#,
        outside.origin()
    ));
    assert_eq!(outcome.get("err").and_then(|e| e.get("kind")).and_then(Json::as_str), Some("PermissionDenied"));
    let userinfo = w.result(&format!(
        r#"(attempt (call http.get {{:url (str "http://user@127.0.0.1:" "{}" "/") :headers {{}}}}))"#,
        inside.port()
    ));
    assert_eq!(userinfo.get("err").and_then(|e| e.get("kind")).and_then(Json::as_str), Some("PermissionDenied"));
    assert_eq!((inside.connections(), outside.connections()), (0, 0));
}

#[test]
fn a_protocol_host_declares_endpoints_and_asks_for_verdicts() {
    let server = Server::start(github);
    let origin = server.origin();
    let mut child = Command::new(env!("CARGO_BIN_EXE_sx"))
        .arg("serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("sx serve starts");
    let mut stdin = child.stdin.take().expect("stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout"));
    let mut exchange = |message: &Json| {
        writeln!(stdin, "{}", shown(message)).expect("kernel reads");
        let mut line = String::new();
        stdout.read_line(&mut line).expect("kernel answers");
        parse(&line).unwrap_or_else(|e| panic!("{e}: {line}"))
    };
    let start = |grants: &str| {
        Json::object([
            ("version", Json::Int(1)),
            ("type", Json::string("start")),
            ("program", Json::string(r#"(call github.repo {:owner "acme" :name "widget"})"#)),
            ("declarations", parse(&declarations(&origin)).expect("JSON")),
            ("grants", parse(grants).expect("JSON")),
        ])
    };
    let requests = exchange(&start(&grants(&origin, r#""GET""#)));
    assert_eq!(requests.get("type").and_then(Json::as_str), Some("requests"), "{}", shown(&requests));
    let id = requests.get("requests").and_then(Json::as_array).and_then(<[Json]>::first).and_then(|r| r.get("id")).and_then(Json::as_str).expect("id").to_string();
    let verdict = exchange(&Json::object([("type", Json::string("check-scope")), ("id", Json::string(id))]));
    assert_eq!(verdict.get("verdict").and_then(Json::as_str), Some("allowed"), "{}", shown(&verdict));
    let cancelled = exchange(&Json::object([("type", Json::string("cancel"))]));
    assert_eq!(cancelled.get("type").and_then(Json::as_str), Some("failed"), "{}", shown(&cancelled));
    let rejected = exchange(&start(&grants("http://127.0.0.1:1", r#""GET""#)));
    assert_eq!(rejected.get("type").and_then(Json::as_str), Some("rejected"), "{}", shown(&rejected));
    assert!(shown(&rejected).contains("out-of-scope"), "{}", shown(&rejected));
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(server.connections(), 0);
}
