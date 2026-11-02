use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use sx_kernel::authority::{Grant, GrantError, Grants, Screen, Verdict};
use sx_kernel::json::parse;
use sx_kernel::registry::Registry;
use sx_kernel::types::CallErrorKind;
use sx_kernel::values::{RecordValue, Value};
use sx_packs::process::{Invocation, Limits, RUN, TIMEOUT, execute};
use sx_packs::{Failure, pack, performer};

static SERIAL: AtomicUsize = AtomicUsize::new(0);

struct Tree {
    base: PathBuf,
    root: PathBuf,
    outside: PathBuf,
}

fn tree(label: &str) -> Tree {
    let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("process-{}-{label}-{serial}", std::process::id()));
    if directory.exists() {
        fs::remove_dir_all(&directory).expect("old sandbox removed");
    }
    fs::create_dir_all(&directory).expect("sandbox");
    let base = fs::canonicalize(directory).expect("canonical sandbox");
    let root = base.join("root");
    let outside = base.join("outside");
    fs::create_dir_all(root.join("sub")).expect("root");
    fs::create_dir_all(&outside).expect("outside");
    Tree { base, root, outside }
}

fn text(path: &Path) -> &str {
    path.to_str().expect("test paths are UTF-8")
}

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.add_pack(pack("process").expect("the process pack")).expect("the process pack registers");
    registry
}

fn try_grant(commands: &str, roots: &[&Path]) -> Result<Grants, GrantError> {
    let roots: Vec<String> = roots.iter().map(|root| format!("\"{}\"", text(root))).collect();
    let json = parse(&format!(r#"{{"process.run": {{"commands": {commands}, "roots": [{}]}}}}"#, roots.join(", ")))
        .expect("grants JSON");
    Grants::load(&registry(), &json)
}

fn grant(commands: &str, roots: &[&Path]) -> Grants {
    try_grant(commands, roots).expect("the grants load")
}

fn grant_error(commands: &str, roots: &[&Path]) -> String {
    match try_grant(commands, roots) {
        Err(error) => error.to_string(),
        Ok(_) => panic!("the grant {commands} loaded"),
    }
}

fn input(program: &str, args: &[&str], cwd: &str, stdin: Option<&str>) -> Value {
    Value::Record(RecordValue::new(vec![
        (Arc::from("program"), Value::string(program)),
        (Arc::from("args"), Value::list(args.iter().map(|arg| Value::string(arg)).collect())),
        (Arc::from("cwd"), Value::string(cwd)),
        (Arc::from("stdin"), stdin.map_or(Value::None, |s| Value::some(Value::string(s)))),
    ]))
}

fn verdict(grants: &Grants, request: &Value) -> Verdict {
    match grants.screen(RUN, request) {
        Screen::Verdict(verdict) => verdict,
        other => panic!("process.run has no scope check: {other:?}"),
    }
}

fn allowed(grants: &Grants, program: &str, args: &[&str], cwd: &Path) -> bool {
    verdict(grants, &input(program, args, text(cwd), None)) == Verdict::Allowed
}

fn reason(grants: &Grants, request: &Value) -> String {
    match verdict(grants, request) {
        Verdict::Denied(reason) => reason,
        Verdict::Allowed => panic!("the request was allowed"),
    }
}

fn perform(grants: &Grants, request: &Value) -> Result<Value, Failure> {
    let scope = match grants.get(RUN) {
        Some(Grant::Scoped { scope, .. }) => scope.clone(),
        other => panic!("process.run is not granted with a scope: {other:?}"),
    };
    performer(RUN).expect("the process pack performs it")(&scope, request)
}

fn run(grants: &Grants, program: &str, args: &[&str], cwd: &Path, stdin: Option<&str>) -> Result<(i64, String, String), Failure> {
    let value = perform(grants, &input(program, args, text(cwd), stdin))?;
    let Value::Record(record) = &value else { panic!("not a record: {value:?}") };
    let status = match record.get("status") {
        Some(Value::Int(status)) => *status,
        other => panic!("no status: {other:?}"),
    };
    let stream = |name: &str| match record.get(name) {
        Some(Value::String(s)) => s.to_string(),
        other => panic!("no {name}: {other:?}"),
    };
    Ok((status, stream("stdout"), stream("stderr")))
}

fn failure(result: Result<(i64, String, String), Failure>) -> Failure {
    result.expect_err("the request failed")
}

const SH: &str = "/bin/sh";

fn shell_grant(root: &Path) -> Grants {
    grant(r#"[{"program": "/bin/sh", "args": [{"tag": "Exact", "value": "-c"}, "Rest"]}]"#, &[root])
}

#[test]
fn every_argument_pattern_matches_by_position() {
    let t = tree("patterns");
    let echo = r#"{"program": "/bin/echo", "args": "#;
    let exact = grant(&format!(r#"[{echo}[{{"tag": "Exact", "value": "a"}}]}}]"#), &[&t.root]);
    assert!(allowed(&exact, "/bin/echo", &["a"], &t.root));
    assert!(!allowed(&exact, "/bin/echo", &["b"], &t.root));
    assert!(!allowed(&exact, "/bin/echo", &[], &t.root));
    assert!(!allowed(&exact, "/bin/echo", &["a", "a"], &t.root));
    assert!(!allowed(&exact, "/bin/echo", &["A"], &t.root));

    let one_of = grant(&format!(r#"[{echo}[{{"tag": "OneOf", "value": ["x", "y"]}}, "AnyOne"]}}]"#), &[&t.root]);
    assert!(allowed(&one_of, "/bin/echo", &["x", "anything"], &t.root));
    assert!(allowed(&one_of, "/bin/echo", &["y", ""], &t.root));
    assert!(!allowed(&one_of, "/bin/echo", &["z", "anything"], &t.root));
    assert!(!allowed(&one_of, "/bin/echo", &["x"], &t.root));
    assert!(!allowed(&one_of, "/bin/echo", &["x", "a", "b"], &t.root));

    let rest = grant(&format!(r#"[{echo}[{{"tag": "Exact", "value": "go"}}, "Rest"]}}]"#), &[&t.root]);
    assert!(allowed(&rest, "/bin/echo", &["go"], &t.root));
    assert!(allowed(&rest, "/bin/echo", &["go", "1", "2", "3"], &t.root));
    assert!(!allowed(&rest, "/bin/echo", &[], &t.root));
    assert!(!allowed(&rest, "/bin/echo", &["stop", "1"], &t.root));

    let nothing = grant(&format!(r#"[{echo}[]}}]"#), &[&t.root]);
    assert!(allowed(&nothing, "/bin/echo", &[], &t.root));
    assert!(!allowed(&nothing, "/bin/echo", &["x"], &t.root));

    let anything = grant(&format!(r#"[{echo}["Rest"]}}]"#), &[&t.root]);
    assert!(allowed(&anything, "/bin/echo", &[], &t.root));
    assert!(allowed(&anything, "/bin/echo", &["--any", "thing"], &t.root));
    let nul = reason(&anything, &input("/bin/echo", &["a\0b"], text(&t.root), None));
    assert!(nul.contains("argument 1 contains a NUL"), "{nul}");

    let several = grant(
        &format!(r#"[{echo}[{{"tag": "Exact", "value": "one"}}]}}, {echo}[{{"tag": "Exact", "value": "two"}}, "AnyOne"]}}]"#),
        &[&t.root],
    );
    assert!(allowed(&several, "/bin/echo", &["one"], &t.root));
    assert!(allowed(&several, "/bin/echo", &["two", "x"], &t.root));
    assert!(!allowed(&several, "/bin/echo", &["one", "x"], &t.root));
    let denied = reason(&several, &input("/bin/echo", &["three"], text(&t.root), None));
    assert!(denied.contains(r#"the arguments ["three"] match no granted argument list for "#), "{denied}");
}

#[test]
fn malformed_scopes_are_refused_when_the_grant_loads() {
    let t = tree("scopes");
    let script = t.base.join("script.sh");
    fs::write(&script, "#!/bin/sh\n").expect("script");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o644)).expect("mode");
    let cases: Vec<(String, &str)> = vec![
        (r#"[{"program": "/bin/echo", "args": ["Rest", "AnyOne"]}]"#.to_string(), "only as the last argument pattern"),
        (r#"[{"program": "/bin/echo", "args": ["Rest", "Rest"]}]"#.to_string(), "only as the last argument pattern"),
        (r#"[{"program": "/bin/echo", "args": [{"tag": "OneOf", "value": []}]}]"#.to_string(), "matches nothing"),
        (r#"[{"program": "/bin/echo", "args": [{"tag": "Exact", "value": "a\u0000b"}]}]"#.to_string(), "NUL"),
        (r#"[{"program": "/bin/echo", "args": ["Anything"]}]"#.to_string(), "expected process.Arg"),
        (r#"[{"program": "/bin/echo", "args": [{"tag": "Exact"}]}]"#.to_string(), "Exact"),
        (r#"[{"program": "/bin/echo", "args": [{"tag": "AnyOne", "value": "x"}]}]"#.to_string(), "AnyOne"),
        (r#"[{"program": "/bin/echo", "args": [{"tag": "OneOf", "value": "x"}]}]"#.to_string(), "expected List<String>"),
        ("[]".to_string(), "at least one command"),
        (r#"[{"program": "echo", "args": []}]"#.to_string(), "never searches PATH"),
        (r#"[{"program": "./echo", "args": []}]"#.to_string(), "not an absolute path"),
        (r#"[{"program": "/nonexistent/program", "args": []}]"#.to_string(), "does not exist"),
        (format!(r#"[{{"program": "{}", "args": []}}]"#, text(&t.base)), "not a regular file"),
        (format!(r#"[{{"program": "{}", "args": []}}]"#, text(&script)), "not executable"),
        (r#"[{"program": "/bin/ec\u0000ho", "args": []}]"#.to_string(), "NUL"),
    ];
    let mut wrong = Vec::new();
    for (commands, expected) in &cases {
        let message = grant_error(commands, &[&t.root]);
        if !message.contains(expected) {
            wrong.push(format!("{commands}: {message}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    let echo = r#"[{"program": "/bin/echo", "args": []}]"#;
    assert!(grant_error(echo, &[&t.base.join("gone")]).contains("does not exist"));
    assert!(grant_error(echo, &[]).contains("at least one root"));
    assert!(grant_error(echo, &[&script]).contains("is not a directory"));
}

#[test]
fn programs_are_absolute_canonical_and_never_searched() {
    let t = tree("programs");
    let grants = grant(r#"[{"program": "/bin/echo", "args": ["Rest"]}]"#, &[&t.root]);
    for denied in ["echo", "./echo", "env", "bin/echo", "", "/bin/echo\0", "/bin/nonexistent"] {
        let request = input(denied, &[], text(&t.root), None);
        assert!(matches!(verdict(&grants, &request), Verdict::Denied(_)), "{denied:?} was allowed");
    }
    let search = reason(&grants, &input("echo", &[], text(&t.root), None));
    assert!(search.contains("never searches PATH"), "{search}");
    let other = reason(&grants, &input("/bin/cat", &[], text(&t.root), None));
    assert!(other.contains("is not a granted program"), "{other}");
    assert!(allowed(&grants, "/bin/../bin/echo", &[], &t.root));

    symlink("/bin/cat", t.root.join("echo")).expect("link to an ungranted program");
    let ungranted = reason(&grants, &input(text(&t.root.join("echo")), &[], text(&t.root), None));
    assert!(ungranted.contains("resolves to") && ungranted.contains("not a granted program"), "{ungranted}");

    let echo = fs::canonicalize("/bin/echo").expect("echo");
    symlink(&echo, t.root.join("alias")).expect("link to a granted program");
    assert!(allowed(&grants, text(&t.root.join("alias")), &["x"], &t.root));

    let through_link = grant(&format!(r#"[{{"program": "{}", "args": ["Rest"]}}]"#, text(&t.root.join("alias"))), &[&t.root]);
    assert!(allowed(&through_link, "/bin/echo", &["x"], &t.root));
    assert!(!allowed(&through_link, "/bin/cat", &[], &t.root));

    if Path::new("/BIN/ECHO").exists() {
        assert!(allowed(&grants, "/BIN/ECHO", &[], &t.root));
    }
}

#[test]
fn a_working_directory_must_resolve_inside_a_root() {
    let t = tree("cwd");
    fs::create_dir_all(t.base.join("root-old")).expect("sibling");
    symlink(&t.outside, t.root.join("escape")).expect("escape");
    symlink(t.root.join("sub"), t.root.join("inside")).expect("inside");
    let grants = grant(r#"[{"program": "/bin/echo", "args": ["Rest"]}]"#, &[&t.root]);
    let at = |cwd: &str| verdict(&grants, &input("/bin/echo", &[], cwd, None));
    assert_eq!(at(text(&t.root)), Verdict::Allowed);
    assert_eq!(at(text(&t.root.join("sub"))), Verdict::Allowed);
    assert_eq!(at(text(&t.root.join("inside"))), Verdict::Allowed);
    assert_eq!(at(&format!("{}/sub/../sub", text(&t.root))), Verdict::Allowed);
    let denied = [
        text(&t.outside).to_string(),
        text(&t.base).to_string(),
        format!("{}/..", text(&t.root)),
        format!("{}/sub/../..", text(&t.root)),
        text(&t.root.join("escape")).to_string(),
        format!("{}/escape/anything", text(&t.root)),
        text(&t.base.join("root-old")).to_string(),
        "relative".to_string(),
        String::new(),
        format!("{}\0", text(&t.root)),
        "/".to_string(),
    ];
    for cwd in &denied {
        assert!(matches!(at(cwd), Verdict::Denied(_)), "{cwd:?} was allowed");
    }
    assert!(reason(&grants, &input("/bin/echo", &[], text(&t.outside), None)).contains("outside every granted root"));
}

#[test]
fn a_working_directory_that_is_not_a_directory_fails_without_running() {
    let t = tree("cwd-kinds");
    fs::write(t.root.join("file"), "x").expect("file");
    let grants = grant(r#"[{"program": "/bin/echo", "args": ["Rest"]}]"#, &[&t.root]);
    let missing = failure(run(&grants, "/bin/echo", &[], &t.root.join("nowhere"), None));
    assert_eq!(missing.kind, CallErrorKind::NotFound, "{missing}");
    let file = failure(run(&grants, "/bin/echo", &[], &t.root.join("file"), None));
    assert_eq!(file.kind, CallErrorKind::Invalid, "{file}");
}

#[test]
fn a_denied_request_never_spawns() {
    let t = tree("denied");
    let grants = grant(
        &format!(r#"[{{"program": "{SH}", "args": [{{"tag": "Exact", "value": "-c"}}, {{"tag": "Exact", "value": "echo run > marker"}}]}}]"#),
        &[&t.root],
    );
    let by_args = failure(run(&grants, SH, &["-c", "echo bad > marker"], &t.root, None));
    assert_eq!(by_args.kind, CallErrorKind::PermissionDenied);
    let by_cwd = failure(run(&grants, SH, &["-c", "echo run > marker"], &t.outside, None));
    assert_eq!(by_cwd.kind, CallErrorKind::PermissionDenied);
    let by_program = failure(run(&grants, "/bin/cat", &[], &t.root, None));
    assert_eq!(by_program.kind, CallErrorKind::PermissionDenied);
    let by_name = failure(run(&grants, "sh", &["-c", "echo run > marker"], &t.root, None));
    assert_eq!(by_name.kind, CallErrorKind::PermissionDenied);
    assert!(!t.root.join("marker").exists() && !t.outside.join("marker").exists());
    assert_eq!(run(&grants, SH, &["-c", "echo run > marker"], &t.root, None).expect("the granted request runs").0, 0);
    assert!(t.root.join("marker").exists());
}

#[test]
fn arguments_reach_the_program_unchanged_and_no_shell_interprets_them() {
    let t = tree("exact");
    let grants = grant(r#"[{"program": "/bin/echo", "args": ["Rest"]}]"#, &[&t.root]);
    let (status, stdout, stderr) =
        run(&grants, "/bin/echo", &["a b", "$HOME", "*", "x;touch hacked", "$(touch hacked)", "`touch hacked`", "\n"], &t.root, None)
            .expect("echo runs");
    assert_eq!((status, stderr.as_str()), (0, ""));
    assert_eq!(stdout, "a b $HOME * x;touch hacked $(touch hacked) `touch hacked` \n\n");
    assert!(!t.root.join("hacked").exists());
    fs::write(t.root.join("present"), "").expect("file for the glob");
    let (_, globbed, _) = run(&grants, "/bin/echo", &["*"], &t.root, None).expect("echo runs");
    assert_eq!(globbed, "*\n");
}

#[test]
fn the_environment_is_empty_and_the_working_directory_is_the_resolved_one() {
    let t = tree("environment");
    let grants = grant(
        r#"[{"program": "/usr/bin/env", "args": ["Rest"]}, {"program": "/bin/sh", "args": ["Rest"]}]"#,
        &[&t.root],
    );
    let (status, stdout, _) = run(&grants, "/usr/bin/env", &[], &t.root, None).expect("env runs");
    assert_eq!((status, stdout.as_str()), (0, ""));
    let (_, pwd, _) = run(&grants, SH, &["-c", "pwd"], &t.root.join("sub"), None).expect("pwd");
    assert_eq!(pwd, format!("{}\n", text(&t.root.join("sub"))));
    let (_, shell_env, _) = run(&grants, SH, &["-c", "echo \"[$HOME][$USER][$LANG]\""], &t.root, None).expect("sh");
    assert_eq!(shell_env, "[][][]\n");
}

#[test]
fn stdin_comes_from_the_input_or_is_empty() {
    let t = tree("stdin");
    let grants = grant(r#"[{"program": "/bin/cat", "args": ["Rest"]}]"#, &[&t.root]);
    let given = run(&grants, "/bin/cat", &[], &t.root, Some("héllo\nworld")).expect("cat runs");
    assert_eq!(given.1, "héllo\nworld");
    assert_eq!(run(&grants, "/bin/cat", &[], &t.root, None).expect("cat runs").1, "");
    assert_eq!(run(&grants, "/bin/cat", &[], &t.root, Some("")).expect("cat runs").1, "");
    let big = "0123456789abcdef".repeat(32 * 1024);
    let echoed = run(&grants, "/bin/cat", &[], &t.root, Some(&big)).expect("cat echoes more than a pipe holds");
    assert_eq!(echoed.1.len(), big.len());
    assert_eq!(echoed.1, big);
}

#[test]
fn a_program_that_ignores_its_stdin_is_not_an_error() {
    let t = tree("stdin-ignored");
    let grants = grant(r#"[{"program": "/bin/echo", "args": ["Rest"]}]"#, &[&t.root]);
    let big = "x".repeat(4 << 20);
    let started = Instant::now();
    let (status, stdout, _) = run(&grants, "/bin/echo", &["done"], &t.root, Some(&big)).expect("echo runs");
    assert_eq!((status, stdout.as_str()), (0, "done\n"));
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn a_nonzero_status_is_a_value_and_both_streams_are_captured() {
    let t = tree("status");
    let grants = shell_grant(&t.root);
    let (status, stdout, stderr) =
        run(&grants, SH, &["-c", "echo out; echo err >&2; exit 7"], &t.root, None).expect("a nonzero exit is not an error");
    assert_eq!((status, stdout.as_str(), stderr.as_str()), (7, "out\n", "err\n"));
    assert_eq!(run(&grants, SH, &["-c", "exit 255"], &t.root, None).expect("exit").0, 255);
}

#[test]
fn a_signal_is_a_failure_that_names_it() {
    let t = tree("signal");
    let grants = shell_grant(&t.root);
    for (script, name) in [("kill -KILL $$", "SIGKILL"), ("kill -TERM $$", "SIGTERM"), ("kill -USR1 $$", "SIGUSR1")] {
        let failed = failure(run(&grants, SH, &["-c", script], &t.root, None));
        assert_eq!(failed.kind, CallErrorKind::Failed, "{failed}");
        assert!(failed.message.contains(name), "{failed}");
    }
}

#[test]
fn output_that_is_not_utf8_fails_after_the_process_ran() {
    let t = tree("utf8");
    let grants = shell_grant(&t.root);
    for script in ["printf '\\377'", "printf '\\377' >&2"] {
        let failed = failure(run(&grants, SH, &["-c", script], &t.root, None));
        assert_eq!(failed.kind, CallErrorKind::Failed, "{failed}");
        assert!(failed.message.contains("not valid UTF-8"), "{failed}");
    }
}

#[test]
fn a_program_that_cannot_start_maps_to_the_error_table() {
    let t = tree("spawn");
    let blind = t.base.join("blind.sh");
    fs::write(&blind, "#!/nonexistent/interpreter\n").expect("script");
    fs::set_permissions(&blind, fs::Permissions::from_mode(0o755)).expect("mode");
    let locked = t.base.join("locked.sh");
    fs::write(&locked, "#!/bin/sh\n").expect("script");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).expect("mode");
    let grants = grant(
        &format!(
            r#"[{{"program": "{}", "args": ["Rest"]}}, {{"program": "{}", "args": ["Rest"]}}]"#,
            text(&blind),
            text(&locked)
        ),
        &[&t.root],
    );
    let missing_interpreter = failure(run(&grants, text(&blind), &[], &t.root, None));
    assert_eq!(missing_interpreter.kind, CallErrorKind::NotFound, "{missing_interpreter}");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).expect("mode");
    let not_executable = failure(run(&grants, text(&locked), &[], &t.root, None));
    assert_eq!(not_executable.kind, CallErrorKind::PermissionDenied, "{not_executable}");
    fs::remove_file(&locked).expect("remove");
    let removed = failure(run(&grants, text(&locked), &[], &t.root, None));
    assert_eq!(removed.kind, CallErrorKind::PermissionDenied, "{removed}");
    assert!(removed.message.contains("does not exist"), "{removed}");
}

fn invocation(program: &str, args: &[&str], cwd: &Path) -> Invocation {
    Invocation {
        program: PathBuf::from(program),
        arg0: program.to_string(),
        args: args.iter().map(|arg| arg.to_string()).collect(),
        cwd: cwd.to_path_buf(),
        stdin: None,
    }
}

fn limits(timeout_ms: u64, output: usize) -> Limits {
    Limits { timeout: Duration::from_millis(timeout_ms), output }
}

fn alive(pid: &str) -> bool {
    let listed = Command::new("/bin/sh").args(["-c", &format!("kill -0 {pid} 2>/dev/null")]).status().expect("sh runs").success();
    let zombie = Command::new("ps")
        .args(["-o", "stat=", "-p", pid])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim_start().starts_with('Z'))
        .unwrap_or(false);
    listed && !zombie
}

fn gone_soon(pid: &str) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if !alive(pid) {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

fn pid_in(path: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(content) = fs::read_to_string(path)
            && content.ends_with('\n')
        {
            return content.trim().to_string();
        }
        assert!(Instant::now() < deadline, "no pid in {}", path.display());
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_standard_limits_are_a_minute_and_a_mebibyte() {
    assert_eq!(Limits::STANDARD, Limits { timeout: Duration::from_secs(60), output: 1_048_576 });
    assert_eq!(TIMEOUT, Duration::from_secs(60));
}

#[test]
fn a_timeout_kills_the_whole_process_group() {
    let t = tree("timeout");
    let started = Instant::now();
    let failed = execute(&invocation("/bin/sleep", &["30"], &t.root), limits(300, 1 << 20)).expect_err("the sleep times out");
    assert_eq!(failed.kind, CallErrorKind::Timeout, "{failed}");
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());

    let pidfile = t.root.join("pid");
    let script = "sleep 30 & echo $! > pid; sleep 30";
    let started = Instant::now();
    let failed = execute(&invocation(SH, &["-c", script], &t.root), limits(500, 1 << 20)).expect_err("the shell times out");
    assert_eq!(failed.kind, CallErrorKind::Timeout, "{failed}");
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(gone_soon(&pid_in(&pidfile)), "the background sleep survived the timeout");
}

#[test]
fn nothing_outlives_a_process_that_exits_normally() {
    let t = tree("background");
    let script = "sleep 30 & echo $! > pid";
    let started = Instant::now();
    let output = execute(&invocation(SH, &["-c", script], &t.root), limits(20_000, 1 << 20)).expect("the shell exits");
    assert_eq!(output.status, 0);
    assert!(started.elapsed() < Duration::from_secs(5), "the call waited for the background process: {:?}", started.elapsed());
    assert!(gone_soon(&pid_in(&t.root.join("pid"))), "the background sleep survived the request");
}

#[test]
fn output_over_the_limit_fails_and_kills_the_process() {
    let t = tree("cap");
    let started = Instant::now();
    for stream in ["1", "2"] {
        let _ = fs::remove_file(t.root.join("pid"));
        let script = format!("yes >&{stream} & echo $! > pid; wait");
        let failed = execute(&invocation(SH, &["-c", &script], &t.root), limits(20_000, 1000)).expect_err("yes overflows");
        assert_eq!(failed.kind, CallErrorKind::Failed, "{failed}");
        assert!(failed.message.contains(if stream == "1" { "stdout" } else { "stderr" }), "{failed}");
        assert!(failed.message.contains("1000 bytes"), "{failed}");
        assert!(gone_soon(&pid_in(&t.root.join("pid"))), "yes survived the cap");
    }
    assert!(started.elapsed() < Duration::from_secs(10));
    let at_the_limit = execute(&invocation("/bin/echo", &["12345678"], &t.root), limits(20_000, 9)).expect("exactly at the limit");
    assert_eq!(at_the_limit.stdout, "12345678\n");
    let over = execute(&invocation("/bin/echo", &["12345678"], &t.root), limits(20_000, 8)).expect_err("one byte over");
    assert_eq!(over.kind, CallErrorKind::Failed);
}

#[test]
fn the_standard_output_cap_applies_to_a_real_request() {
    let t = tree("cap-standard");
    let grants = grant(r#"[{"program": "/usr/bin/yes", "args": ["Rest"]}]"#, &[&t.root]);
    let started = Instant::now();
    let failed = failure(run(&grants, "/usr/bin/yes", &[], &t.root, None));
    assert_eq!(failed.kind, CallErrorKind::Failed, "{failed}");
    assert!(failed.message.contains("1048576 bytes"), "{failed}");
    assert!(started.elapsed() < Duration::from_secs(20));
}
