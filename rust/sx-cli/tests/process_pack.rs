use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use sx_kernel::json::{Json, Layout, dumps, parse};

static SERIAL: AtomicUsize = AtomicUsize::new(0);

const GRANTS: &str = r#"{
  "process.run": {
    "commands": [
      {"program": "/bin/echo", "args": [{"tag": "Exact", "value": "hello"}, "AnyOne", "Rest"]},
      {"program": "/bin/echo", "args": [{"tag": "OneOf", "value": ["red", "green"]}]},
      {"program": "/bin/sh", "args": [{"tag": "Exact", "value": "-c"}, {"tag": "Exact", "value": "exit 3"}]},
      {"program": "/bin/sh", "args": [{"tag": "Exact", "value": "-c"}, {"tag": "Exact", "value": "touch granted-marker"}]},
      {"program": "/usr/bin/env", "args": []}
    ],
    "roots": ["ROOT"]
  }
}"#;

struct Sandbox {
    base: PathBuf,
    root: PathBuf,
    outside: PathBuf,
}

impl Sandbox {
    fn new(label: &str) -> Sandbox {
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("process-pack-{}-{label}-{serial}", std::process::id()));
        if base.exists() {
            fs::remove_dir_all(&base).expect("old sandbox removed");
        }
        fs::create_dir_all(&base).expect("sandbox");
        let base = fs::canonicalize(base).expect("canonical sandbox");
        let root = base.join("root");
        let outside = base.join("outside");
        fs::create_dir_all(&root).expect("root");
        fs::create_dir_all(&outside).expect("outside");
        fs::write(base.join("declarations.json"), r#"{"packs": ["process"]}"#).expect("declarations");
        fs::write(base.join("grants.json"), GRANTS.replace("ROOT", text(&root))).expect("grants");
        Sandbox { base, root, outside }
    }

    fn program(&self, source: &str) -> String {
        let path = self.base.join(format!("program-{}.sx", SERIAL.fetch_add(1, Ordering::Relaxed)));
        fs::write(&path, source).expect("program");
        text(&path).to_string()
    }

    fn sx(&self, command: &str, source: &str, extra: &[&str]) -> Ran {
        let program = self.program(source);
        let declarations = self.base.join("declarations.json");
        let grants = self.base.join("grants.json");
        let mut args = vec![command, &program, "--declarations", text(&declarations), "--grants", text(&grants)];
        args.extend(extra);
        run_sx(&args, &[])
    }

    fn run(&self, source: &str) -> Json {
        let ran = self.sx("run", source, &["--format", "json"]);
        assert!(ran.code == 0 || ran.code == 2, "sx run failed: {ran:?}");
        parse(&ran.stdout).unwrap_or_else(|e| panic!("{e}: {}", ran.stdout))
    }

    fn result(&self, source: &str) -> Json {
        let outcome = self.run(source);
        assert_eq!(outcome.get("outcome").and_then(Json::as_str), Some("completed"), "{}", shown(&outcome));
        outcome.get("result").cloned().expect("a result")
    }

    fn call(&self, program: &str, args: &[&str], cwd: &Path) -> String {
        let args: Vec<String> = args.iter().map(|arg| format!("\"{arg}\"")).collect();
        format!(r#"(call process.run {{:program "{program}" :args [{}] :cwd "{}"}})"#, args.join(" "), text(cwd))
    }
}

fn text(path: &Path) -> &str {
    path.to_str().expect("test paths are UTF-8")
}

fn shown(json: &Json) -> String {
    dumps(json, Layout::Spaced).expect("serializable")
}

#[derive(Debug)]
struct Ran {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run_sx(args: &[&str], environment: &[(&str, &str)]) -> Ran {
    let output = Command::new(env!("CARGO_BIN_EXE_sx")).args(args).envs(environment.iter().copied()).output().expect("sx runs");
    Ran {
        code: output.status.code().expect("exit status"),
        stdout: String::from_utf8(output.stdout).expect("utf-8"),
        stderr: String::from_utf8(output.stderr).expect("utf-8"),
    }
}

#[test]
fn a_granted_program_runs_end_to_end() {
    let s = Sandbox::new("run");
    let result = s.result(&s.call("/bin/echo", &["hello", "big", "world"], &s.root));
    assert_eq!(shown(&result), r#"{"status": 0, "stdout": "hello big world\n", "stderr": ""}"#);
    let sourced = s.result(&format!(
        "(let [out (call process.run {{:program \"/bin/echo\" :args [\"red\"] :cwd \"{}\" :stdin (some \"ignored\")}})] (get out :stdout))",
        text(&s.root)
    ));
    assert_eq!(shown(&sourced), "\"red\\n\"");
}

#[test]
fn a_nonzero_status_is_data_the_program_can_branch_on() {
    let s = Sandbox::new("status");
    let source = format!(
        "(let [out (call process.run {{:program \"/bin/sh\" :args [\"-c\" \"exit 3\"] :cwd \"{}\"}})] (if (= (get out :status) 3) \"three\" \"other\"))",
        text(&s.root)
    );
    assert_eq!(shown(&s.result(&source)), "\"three\"");
}

#[test]
fn the_child_gets_no_environment() {
    let s = Sandbox::new("environment");
    let program = s.program(&s.call("/usr/bin/env", &[], &s.root));
    let declarations = s.base.join("declarations.json");
    let grants = s.base.join("grants.json");
    let ran = run_sx(
        &["run", &program, "--declarations", text(&declarations), "--grants", text(&grants), "--format", "json"],
        &[("SX_LEAK", "secret"), ("PATH", "/usr/bin:/bin"), ("HOME", "/nonexistent")],
    );
    assert_eq!(ran.code, 0, "{ran:?}");
    let outcome = parse(&ran.stdout).expect("json");
    assert_eq!(shown(outcome.get("result").expect("result")), r#"{"status": 0, "stdout": "", "stderr": ""}"#);
}

#[test]
fn an_out_of_scope_literal_is_rejected_before_anything_runs() {
    let s = Sandbox::new("static");
    let source = s.call("/bin/sh", &["-c", "touch ungranted-marker"], &s.root);
    let checked = s.sx("check", &source, &[]);
    assert_eq!(checked.code, 1, "{checked:?}");
    assert!(checked.stdout.starts_with("error[out-of-scope] 1:19-"), "{}", checked.stdout);
    assert!(checked.stdout.contains("The host's grant for process.run does not cover this input"), "{}", checked.stdout);
    assert!(checked.stdout.contains(r#"the arguments ["-c" "touch ungranted-marker"] match no granted argument list for /bin/sh"#), "{}", checked.stdout);
    assert!(checked.stdout.contains("(Exact \"hello\") AnyOne Rest"), "{}", checked.stdout);
    assert!(checked.stdout.contains("Help: sx help process.run"), "{}", checked.stdout);
    let ran = s.sx("run", &source, &["--format", "json"]);
    assert_eq!(ran.code, 1, "{ran:?}");
    assert!(ran.stdout.contains(r#""code": "out-of-scope""#), "{}", ran.stdout);
    for denied in [
        s.call("/bin/cat", &[], &s.root),
        s.call("echo", &["hello", "x"], &s.root),
        s.call("/bin/echo", &["hello", "x"], &s.outside),
        s.call("/bin/echo", &["hello"], &s.root),
    ] {
        let checked = s.sx("check", &denied, &[]);
        assert_eq!(checked.code, 1, "{denied}: {checked:?}");
        assert!(checked.stdout.contains("error[out-of-scope]"), "{denied}: {}", checked.stdout);
    }
    assert!(!s.root.join("ungranted-marker").exists());
    let allowed = s.sx("check", &s.call("/bin/echo", &["hello", "x"], &s.root), &[]);
    assert_eq!((allowed.code, allowed.stdout.as_str()), (0, "ok: process.Output\n"));
}

#[test]
fn explain_shows_a_verdict_per_call_site() {
    let s = Sandbox::new("explain");
    let source = format!(
        "[{} {} (call process.run {{:program (str \"/bin/\" \"echo\") :args [\"red\"] :cwd \"{}\"}})]",
        s.call("/bin/echo", &["red"], &s.root),
        s.call("/bin/echo", &["blue"], &s.root),
        text(&s.root)
    );
    let explained = s.sx("explain", &source, &[]);
    assert_eq!(explained.code, 0, "{explained:?}");
    let lines: Vec<&str> = explained.stdout.lines().collect();
    assert_eq!(lines.first(), Some(&"tools:"));
    assert_eq!(lines.get(1), Some(&"  process.run  [write, not idempotent]  allowed"));
    assert_eq!(
        lines.get(2).copied(),
        Some(format!(r#"    input: {{"program": "/bin/echo", "args": ["red"], "cwd": "{}", "stdin": null}}  scope: allowed"#, text(&s.root)).as_str())
    );
    let denied = lines.get(3).expect("the denied call");
    assert!(denied.contains("scope: denied: the arguments [\"blue\"] match no granted argument list"), "{denied}");
    assert_eq!(lines.get(4), Some(&"    input: computed at run time  scope: checked per request"));
    let json = s.sx("explain", &source, &["--format", "json"]);
    let report = parse(&json.stdout).expect("json");
    let tool = report.get("tools").and_then(Json::as_array).and_then(|tools| tools.first()).expect("a tool");
    let verdicts: Vec<&str> = tool
        .get("verdicts")
        .and_then(Json::as_array)
        .expect("verdicts")
        .iter()
        .map(|v| v.get("verdict").and_then(Json::as_str).expect("verdict"))
        .collect();
    assert_eq!(verdicts, ["allowed", "denied", "checked per request"]);
    let declarations = s.base.join("declarations.json");
    let ungranted = run_sx(&["explain", &s.program(&s.call("/bin/echo", &["red"], &s.root)), "--declarations", text(&declarations)], &[]);
    assert!(ungranted.stdout.contains("scope: not granted"), "{ungranted:?}");
}

#[test]
fn a_computed_request_outside_the_scope_is_refused_without_spawning() {
    let s = Sandbox::new("refused");
    let marker = s.root.join("granted-marker");
    let by_args = format!(
        "(attempt (call process.run {{:program \"/bin/sh\" :args [\"-c\" (str \"touch ungranted\" \"-marker\")] :cwd \"{}\"}}))",
        text(&s.root)
    );
    let result = s.result(&by_args);
    let error = result.get("err").unwrap_or_else(|| panic!("expected an error: {}", shown(&result)));
    assert_eq!(error.get("kind").and_then(Json::as_str), Some("PermissionDenied"));
    assert!(error.get("message").and_then(Json::as_str).expect("message").contains("match no granted argument list"));
    let by_cwd = format!(
        "(attempt (call process.run {{:program \"/bin/sh\" :args [\"-c\" \"touch granted-marker\"] :cwd (str \"{}\" \"\")}}))",
        text(&s.outside)
    );
    let result = s.result(&by_cwd);
    assert_eq!(result.get("err").and_then(|e| e.get("kind")).and_then(Json::as_str), Some("PermissionDenied"));
    let retried = s.result(&format!(
        "(attempt (retry {{:delays [1ms 1ms] :on [PermissionDenied]}} (call process.run {{:program \"/bin/sh\" :args [\"-c\" (str \"touch ungranted\" \"-marker\")] :cwd \"{}\"}})))",
        text(&s.root)
    ));
    assert_eq!(retried.get("err").and_then(|e| e.get("kind")).and_then(Json::as_str), Some("PermissionDenied"));
    assert!(!s.root.join("ungranted-marker").exists() && !marker.exists() && !s.outside.join("granted-marker").exists());
}

#[test]
fn a_granted_computed_request_runs() {
    let s = Sandbox::new("computed");
    let source = format!(
        "(call process.run {{:program (str \"/bin/\" \"sh\") :args [\"-c\" (str \"touch granted\" \"-marker\")] :cwd \"{}\"}})",
        text(&s.root)
    );
    let result = s.result(&source);
    assert_eq!(shown(&result), r#"{"status": 0, "stdout": "", "stderr": ""}"#);
    assert!(s.root.join("granted-marker").exists());
}

#[test]
fn malformed_grants_are_usage_errors() {
    let s = Sandbox::new("usage");
    let declarations = text(&s.base.join("declarations.json")).to_string();
    let program = s.program(&s.call("/bin/echo", &["hello", "x"], &s.root));
    let allowed = run_sx(&["check", &program, "--declarations", &declarations, "--allow", "process.run"], &[]);
    assert_eq!(allowed.code, 64);
    assert!(allowed.stderr.contains("declares a scope of type"), "{}", allowed.stderr);
    let bad: [(&str, &str); 4] = [
        (r#"{"commands": [{"program": "/bin/echo", "args": ["Rest", "AnyOne"]}], "roots": ["ROOT"]}"#, "only as the last argument pattern"),
        (r#"{"commands": [{"program": "echo", "args": []}], "roots": ["ROOT"]}"#, "never searches PATH"),
        (r#"{"commands": [{"program": "/bin/echo", "args": [{"Exact": "x"}]}], "roots": ["ROOT"]}"#, "expected process.Arg"),
        (r#"{"commands": [{"program": "/bin/echo", "args": []}], "roots": []}"#, "at least one root"),
    ];
    for (scope, expected) in bad {
        let path = s.base.join("bad-grants.json");
        fs::write(&path, format!(r#"{{"process.run": {}}}"#, scope.replace("ROOT", text(&s.root)))).expect("grants");
        let refused = run_sx(&["run", &program, "--declarations", &declarations, "--grants", text(&path)], &[]);
        assert_eq!(refused.code, 64, "{scope}: {refused:?}");
        assert!(refused.stderr.contains(expected), "{scope}: {}", refused.stderr);
    }
}
