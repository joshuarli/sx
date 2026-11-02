#[path = "../../sx-packs/tests/support/mod.rs"]
mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
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
