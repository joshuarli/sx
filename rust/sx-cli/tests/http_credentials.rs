#[path = "../../sx-packs/tests/support/mod.rs"]
mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use support::{Reply, Server};
use sx_kernel::json::{Json, parse};

static SERIAL: AtomicUsize = AtomicUsize::new(0);

const SECRET: &str = "Bearer cli-secret-token-9f8e7d6c";

struct Workspace {
    base: PathBuf,
    origin: String,
}

#[derive(Debug)]
struct Ran {
    code: i32,
    stdout: String,
    stderr: String,
}

fn text(path: &Path) -> &str {
    path.to_str().expect("test paths are UTF-8")
}

impl Workspace {
    fn new(label: &str, origin: &str, endpoint_headers: &str, secret: &str) -> Workspace {
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("http-credentials-{}-{label}-{serial}", std::process::id()));
        if base.exists() {
            fs::remove_dir_all(&base).expect("old workspace removed");
        }
        fs::create_dir_all(&base).expect("workspace");
        let declarations = format!(
            r#"{{"packs": ["http"], "capabilities": [{{"id": "store.put", "http": {{"method": "PUT", "url": "{origin}/objects/{{key}}"{endpoint_headers}}},
  "input": "Record {{key: String, body: String}}", "output": "Record open {{stored: Bool}}", "effect": "write", "idempotent": true}}]}}"#
        );
        fs::write(base.join("declarations.json"), declarations).expect("declarations");
        let scope = format!(
            r#"{{"origins": ["{origin}"], "methods": ["GET", "PUT"], "headers": {{"{origin}": {{"authorization": {secret}}}}}}}"#
        );
        fs::write(base.join("grants.json"), format!(r#"{{"store.put": {scope}, "http.get": {scope}, "http.request": {scope}}}"#))
            .expect("grants");
        Workspace { base, origin: origin.to_string() }
    }

    fn sx(&self, command: &str, source: &str, extra: &[&str], env: &[(&str, &str)]) -> Ran {
        let program = self.base.join(format!("program-{}.sx", SERIAL.fetch_add(1, Ordering::Relaxed)));
        fs::write(&program, source).expect("program");
        let declarations = self.base.join("declarations.json");
        let grants = self.base.join("grants.json");
        let mut args = vec![command, text(&program), "--declarations", text(&declarations), "--grants", text(&grants)];
        args.extend(extra);
        let output = Command::new(env!("CARGO_BIN_EXE_sx")).args(&args).envs(env.iter().copied()).output().expect("sx runs");
        Ran {
            code: output.status.code().expect("exit status"),
            stdout: String::from_utf8(output.stdout).expect("utf-8"),
            stderr: String::from_utf8(output.stderr).expect("utf-8"),
        }
    }
}

fn literal(secret: &str) -> String {
    format!("\"{secret}\"")
}

fn leaks(ran: &Ran, secret: &str) -> bool {
    let token = secret.strip_prefix("Bearer ").unwrap_or(secret);
    ran.stdout.contains(token) || ran.stderr.contains(token)
}

fn echoing(seen: &support::Seen) -> Reply {
    let echoed = seen.header("authorization").unwrap_or("none");
    if seen.target.starts_with("/objects/") {
        return match seen.header("authorization") {
            Some(_) if seen.target == "/objects/bad" => Reply::status(500, &format!("rejected {echoed}")),
            Some(_) => Reply::json(r#"{"stored": true}"#),
            None => Reply::status(401, "unauthorized"),
        };
    }
    Reply::with(200, &[("x-seen", echoed)], &format!("you sent {echoed}"))
}

#[test]
fn a_program_uses_the_credential_and_never_sees_it() {
    let server = Server::start(echoing);
    let w = Workspace::new("hidden", &server.origin(), "", &literal(SECRET));
    let origin = &w.origin;
    let program = format!(
        r#"(let [stored (call store.put {{:key "a" :body "hello"}})
               raw (call http.get {{:url "{origin}/echo" :headers {{}}}})
               failed (attempt (call store.put {{:key "bad" :body "x"}}))]
           {{:stored stored :raw raw :failed failed}})"#
    );
    let ran = w.sx("run", &program, &["--format", "json"], &[]);
    assert_eq!(ran.code, 0, "{ran:?}");
    assert!(!leaks(&ran, SECRET), "the result, trace, or stderr holds the secret: {ran:?}");
    let outcome = parse(&ran.stdout).expect("json");
    let result = outcome.get("result").expect("result");
    assert_eq!(result.get("raw").and_then(|raw| raw.get("body")).and_then(Json::as_str), Some("you sent [redacted]"));
    assert_eq!(
        result.get("failed").and_then(|f| f.get("err")).and_then(|e| e.get("message")).and_then(Json::as_str).map(|m| m.contains("rejected [redacted]")),
        Some(true)
    );
    for request in server.requests() {
        assert_eq!(request.header("authorization"), Some(SECRET), "{}", request.target);
    }
    for command in ["explain", "check"] {
        let other = w.sx(command, &program, &[], &[]);
        assert!(!leaks(&other, SECRET), "{command}: {other:?}");
    }
    let explained = w.sx("explain", &program, &["--format", "json"], &[]);
    assert!(!leaks(&explained, SECRET) && !explained.stdout.contains("authorization"), "{explained:?}");
}

#[test]
fn an_env_reference_in_a_grants_file_is_read_from_the_host_environment() {
    let server = Server::start(echoing);
    let w = Workspace::new("env", &server.origin(), "", r#"{"env": "SX_TEST_API_TOKEN"}"#);
    let program = r#"(call store.put {:key "a" :body "hello"})"#;
    let ran = w.sx("run", program, &["--format", "json"], &[("SX_TEST_API_TOKEN", SECRET)]);
    assert_eq!(ran.code, 0, "{ran:?}");
    assert!(!leaks(&ran, SECRET), "{ran:?}");
    assert_eq!(server.requests()[0].header("authorization"), Some(SECRET));
    let missing = w.sx("run", program, &["--format", "json"], &[]);
    assert_eq!(missing.code, 64, "{missing:?}");
    assert!(
        missing.stderr.contains("the environment variable SX_TEST_API_TOKEN is not set"),
        "{}",
        missing.stderr
    );
    assert!(missing.stdout.is_empty());
    assert_eq!(server.connections(), 1);
    let checked = w.sx("check", program, &[], &[]);
    assert_eq!(checked.code, 64, "a grants file is loaded for check too: {checked:?}");
}

#[test]
fn a_header_the_program_sets_for_an_origin_that_injects_it_is_refused() {
    let server = Server::start(echoing);
    let w = Workspace::new("override", &server.origin(), "", &literal(SECRET));
    let origin = &w.origin;
    let program = format!(
        r#"(attempt (call http.get {{:url "{origin}/echo" :headers (assoc {{}} "Authorization" "Bearer mine")}}))"#
    );
    let ran = w.sx("run", &program, &["--format", "json"], &[]);
    assert_eq!(ran.code, 0, "{ran:?}");
    let outcome = parse(&ran.stdout).expect("json");
    let error = outcome.get("result").and_then(|r| r.get("err")).expect("an error");
    assert_eq!(error.get("kind").and_then(Json::as_str), Some("PermissionDenied"));
    assert!(error.get("message").and_then(Json::as_str).is_some_and(|m| m.contains("the grant sets the header authorization")));
    assert!(!leaks(&ran, SECRET));
    assert_eq!(server.connections(), 0);
}

#[test]
fn an_endpoint_that_declares_an_injected_header_is_rejected_by_check() {
    let server = Server::start(echoing);
    let w = Workspace::new("declared", &server.origin(), r#", "headers": {"authorization": "static"}"#, &literal(SECRET));
    let program = r#"(call store.put {:key "a" :body "hello"})"#;
    let checked = w.sx("check", program, &["--format", "json"], &[]);
    assert_eq!(checked.code, 1, "{checked:?}");
    assert!(checked.stdout.contains(r#""code": "out-of-scope""#), "{checked:?}");
    assert!(checked.stdout.contains("the grant sets the header authorization"), "{checked:?}");
    assert!(checked.stdout.contains(r#"\"authorization\" \"[hidden]\""#), "the granted scope is shown with values hidden: {checked:?}");
    assert!(!leaks(&checked, SECRET), "{checked:?}");
    assert_eq!(server.connections(), 0);
}
