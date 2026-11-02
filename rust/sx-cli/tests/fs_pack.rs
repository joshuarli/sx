use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use sx_kernel::json::{Json, Layout, dumps, parse};

static SERIAL: AtomicUsize = AtomicUsize::new(0);

struct Sandbox {
    base: PathBuf,
    root: PathBuf,
    outside: PathBuf,
}

impl Sandbox {
    fn new(label: &str) -> Sandbox {
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("fs-pack-{}-{label}-{serial}", std::process::id()));
        if base.exists() {
            fs::remove_dir_all(&base).expect("old sandbox removed");
        }
        fs::create_dir_all(&base).expect("sandbox");
        let base = fs::canonicalize(base).expect("canonical sandbox");
        let root = base.join("root");
        let outside = base.join("outside");
        fs::create_dir_all(root.join("sub")).expect("root");
        fs::create_dir_all(&outside).expect("outside");
        fs::write(root.join("note.txt"), "inside").expect("note");
        fs::write(outside.join("secret.txt"), "secret").expect("secret");
        fs::write(base.join("declarations.json"), r#"{"packs": ["fs"]}"#).expect("declarations");
        let scope = Json::object([("roots", Json::Array(vec![Json::string(text(&root))]))]);
        let ids = ["fs.read-text", "fs.read-bytes", "fs.list", "fs.stat", "fs.write-text"];
        let grants = Json::Object(ids.iter().map(|id| ((*id).to_string(), scope.clone())).collect());
        fs::write(base.join("grants.json"), dumps(&grants, Layout::Spaced).expect("grants")).expect("grants file");
        Sandbox { base, root, outside }
    }

    fn at(&self, relative: &str) -> String {
        text(&self.root.join(relative)).to_string()
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
        run_sx(&args)
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

fn run_sx(args: &[&str]) -> Ran {
    let output = Command::new(env!("CARGO_BIN_EXE_sx")).args(args).output().expect("sx runs");
    Ran {
        code: output.status.code().expect("exit status"),
        stdout: String::from_utf8(output.stdout).expect("utf-8"),
        stderr: String::from_utf8(output.stderr).expect("utf-8"),
    }
}

fn quoted(path: &str) -> String {
    format!("\"{path}\"")
}

fn error_kind(result: &Json) -> (&str, &str) {
    let error = result.get("err").unwrap_or_else(|| panic!("expected an error: {}", shown(result)));
    (error.get("kind").and_then(Json::as_str).expect("kind"), error.get("message").and_then(Json::as_str).expect("message"))
}

#[test]
fn every_capability_runs_for_real_inside_its_roots() {
    let s = Sandbox::new("capabilities");
    fs::write(s.root.join("sub/b.txt"), "bee").expect("b");
    symlink(s.root.join("note.txt"), s.root.join("sub/a-link")).expect("link");
    let read = s.result(&format!("(call fs.read-text {{:path {}}})", quoted(&s.at("note.txt"))));
    assert_eq!(shown(&read), "\"inside\"");
    let bytes = s.result(&format!("(call fs.read-bytes {{:path {}}})", quoted(&s.at("note.txt"))));
    assert_eq!(shown(&bytes), "\"aW5zaWRl\"");
    let listed = s.result(&format!("(call fs.list {{:path {}}})", quoted(&s.at("sub"))));
    assert_eq!(
        shown(&listed),
        r#"[{"name": "a-link", "kind": "Symlink"}, {"name": "b.txt", "kind": "File"}]"#
    );
    let stat = s.result(&format!(
        "[(call fs.stat {{:path {}}}) (call fs.stat {{:path {}}}) (call fs.stat {{:path {}}})]",
        quoted(&s.at("note.txt")),
        quoted(&s.at("sub")),
        quoted(&s.at("absent.txt"))
    ));
    let shown_stat = shown(&stat);
    assert!(shown_stat.starts_with(r#"[{"kind": "File", "size": 6}, {"kind": "Directory", "size": "#), "{shown_stat}");
    assert!(shown_stat.ends_with("}, null]"), "{shown_stat}");
    let target = s.at("sub/written.txt");
    let written = s.result(&format!(
        "(let [_ (call fs.write-text {{:path {p} :text \"one\"}}) _ (call fs.write-text {{:path {p} :text \"two\"}})] (call fs.read-text {{:path {p}}}))",
        p = quoted(&target)
    ));
    assert_eq!(shown(&written), "\"two\"");
    let leftovers: Vec<String> = fs::read_dir(s.root.join("sub"))
        .expect("sub")
        .map(|e| e.expect("entry").file_name().into_string().expect("utf-8"))
        .filter(|name| name.starts_with(".sx-write"))
        .collect();
    assert!(leftovers.is_empty(), "temporary files remain: {leftovers:?}");
}

#[test]
fn a_rewrite_keeps_the_file_mode() {
    let s = Sandbox::new("mode");
    let path = s.root.join("script.sh");
    fs::write(&path, "old").expect("script");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o750)).expect("mode");
    s.result(&format!("(call fs.write-text {{:path {} :text \"new\"}})", quoted(text(&path))));
    assert_eq!(fs::read_to_string(&path).expect("script"), "new");
    assert_eq!(fs::metadata(&path).expect("metadata").permissions().mode() & 0o777, 0o750);
}

#[test]
fn os_failures_map_to_call_error_kinds() {
    let s = Sandbox::new("errors");
    fs::write(s.root.join("binary.dat"), [0xff, 0xfe, 0x00]).expect("binary");
    let locked = s.root.join("locked.txt");
    fs::write(&locked, "x").expect("locked");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).expect("lock");
    let cases = [
        (format!("(attempt (call fs.read-text {{:path {}}}))", quoted(&s.at("missing.txt"))), "NotFound", "No such file"),
        (format!("(attempt (call fs.read-bytes {{:path {}}}))", quoted(&s.at("sub/missing"))), "NotFound", "No such file"),
        (format!("(attempt (call fs.list {{:path {}}}))", quoted(&s.at("nothing"))), "NotFound", "No such file"),
        (format!("(attempt (call fs.read-text {{:path {}}}))", quoted(text(&locked))), "PermissionDenied", "ermission denied"),
        (format!("(attempt (call fs.read-text {{:path {}}}))", quoted(&s.at("sub"))), "Invalid", "is a directory"),
        (format!("(attempt (call fs.read-text {{:path {}}}))", quoted(&s.at("binary.dat"))), "Invalid", "not valid UTF-8"),
        (format!("(attempt (call fs.list {{:path {}}}))", quoted(&s.at("note.txt"))), "Invalid", "ot a directory"),
        (format!("(attempt (call fs.write-text {{:path {} :text \"x\"}}))", quoted(&s.at("sub"))), "Invalid", "is a directory"),
        (format!("(attempt (call fs.write-text {{:path {} :text \"x\"}}))", quoted(&s.at("nowhere/x.txt"))), "NotFound", "No such file"),
        (
            format!("(attempt (call fs.read-text {{:path (str {} \"/secret.txt\")}}))", quoted(text(&s.outside))),
            "PermissionDenied",
            "outside every granted root",
        ),
    ];
    let mut failures = Vec::new();
    for (source, kind, message) in &cases {
        let result = s.result(source);
        let (found_kind, found_message) = error_kind(&result);
        if found_kind != *kind || !found_message.contains(message) {
            failures.push(format!("{source}: {found_kind}: {found_message}"));
        }
    }
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o644)).expect("unlock");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn a_computed_path_outside_the_roots_is_refused_without_being_read() {
    let s = Sandbox::new("refused");
    let source = format!("(call fs.read-text {{:path (str {} \"/secret.txt\")}})", quoted(text(&s.outside)));
    let outcome = s.run(&source);
    assert_eq!(outcome.get("outcome").and_then(Json::as_str), Some("failed"));
    let failure = outcome.get("failure").expect("failure");
    assert_eq!(failure.get("kind").and_then(Json::as_str), Some("call-failed"));
    let trace = shown(outcome.get("trace").expect("trace"));
    assert!(trace.contains(r#""kind": "PermissionDenied""#) && trace.contains("outside every granted root"), "{trace}");
    let retried = s.result(&format!(
        "(attempt (retry {{:delays [1ms 1ms] :on [PermissionDenied]}} (call fs.read-text {{:path (str {} \"/secret.txt\")}})))",
        quoted(text(&s.outside))
    ));
    assert_eq!(error_kind(&retried).0, "PermissionDenied");
}

#[test]
fn par_map_reads_run_through_the_native_host_in_order() {
    let s = Sandbox::new("parallel");
    for i in 0..12 {
        fs::write(s.root.join(format!("f{i}.txt")), format!("{i}")).expect("file");
    }
    let source = format!(
        "(par-map {{:limit 4}} (fn [i] (call fs.read-text {{:path (str {} \"/f\" i \".txt\")}})) [0 1 2 3 4 5 6 7 8 9 10 11])",
        quoted(text(&s.root))
    );
    let result = s.result(&source);
    let expected: Vec<Json> = (0..12).map(|i| Json::string(i.to_string())).collect();
    assert_eq!(shown(&result), shown(&Json::Array(expected)));
}

#[test]
fn an_out_of_scope_literal_is_rejected_before_anything_runs() {
    let s = Sandbox::new("static");
    let secret = text(&s.outside.join("secret.txt")).to_string();
    let source = format!("(call fs.read-text {{:path {}}})", quoted(&secret));
    let checked = s.sx("check", &source, &[]);
    assert_eq!(checked.code, 1, "{checked:?}");
    assert!(checked.stdout.starts_with("error[out-of-scope] 1:20-"), "{}", checked.stdout);
    assert!(checked.stdout.contains("The host's grant for fs.read-text does not cover this input"), "{}", checked.stdout);
    assert!(checked.stdout.contains("outside every granted root"), "{}", checked.stdout);
    assert!(checked.stdout.contains("Help: sx help fs.read-text"), "{}", checked.stdout);
    let ran = s.sx("run", &source, &["--format", "json"]);
    assert_eq!(ran.code, 1, "{ran:?}");
    assert!(ran.stdout.contains(r#""code": "out-of-scope""#), "{}", ran.stdout);
    let allowed = s.sx("check", &format!("(call fs.read-text {{:path {}}})", quoted(&s.at("note.txt"))), &[]);
    assert_eq!((allowed.code, allowed.stdout.as_str()), (0, "ok: String\n"));
}

#[test]
fn explain_shows_each_call_site_with_its_verdict() {
    let s = Sandbox::new("explain");
    let source = format!(
        "[(call fs.read-text {{:path {}}}) (call fs.read-text {{:path {}}}) (call fs.read-text {{:path (str {} \"/x\")}})]",
        quoted(&s.at("note.txt")),
        quoted(text(&s.outside.join("secret.txt"))),
        quoted(text(&s.root))
    );
    let explained = s.sx("explain", &source, &[]);
    assert_eq!(explained.code, 0, "{explained:?}");
    let lines: Vec<&str> = explained.stdout.lines().collect();
    assert_eq!(lines.first(), Some(&"tools:"));
    assert_eq!(lines.get(1), Some(&"  fs.read-text  [read]  allowed"));
    assert_eq!(lines.get(2).copied(), Some(format!("    input: {{\"path\": \"{}\"}}  scope: allowed", s.at("note.txt")).as_str()));
    let denied = lines.get(3).expect("the denied call");
    assert!(denied.contains("scope: denied: ") && denied.contains("outside every granted root"), "{denied}");
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
    let ungranted = run_sx(&[
        "explain",
        &s.program(&format!("(call fs.stat {{:path {}}})", quoted(&s.at("note.txt")))),
        "--declarations",
        text(&s.base.join("declarations.json")),
    ]);
    assert!(ungranted.stdout.contains("  fs.stat  [read]  not allowed\n    input: "), "{ungranted:?}");
    assert!(ungranted.stdout.contains("scope: not granted"), "{ungranted:?}");
}

#[test]
fn grant_and_host_misuse_are_usage_errors() {
    let s = Sandbox::new("usage");
    let declarations = text(&s.base.join("declarations.json")).to_string();
    let program = s.program(&format!("(call fs.read-text {{:path {}}})", quoted(&s.at("note.txt"))));
    let allowed = run_sx(&["check", &program, "--declarations", &declarations, "--allow", "fs.read-text"]);
    assert_eq!(allowed.code, 64);
    assert!(allowed.stderr.contains("declares a scope of type Record {roots: List<String>}"), "{}", allowed.stderr);
    let missing = s.base.join("missing-root.json");
    fs::write(&missing, format!(r#"{{"fs.read-text": {{"roots": ["{}"]}}}}"#, text(&s.base.join("gone")))).expect("grants");
    let refused = run_sx(&["run", &program, "--declarations", &declarations, "--grants", text(&missing)]);
    assert_eq!(refused.code, 64);
    assert!(refused.stderr.contains("does not exist"), "{}", refused.stderr);
    let mixed = s.base.join("mixed.json");
    fs::write(&mixed, r#"{"packs": ["fs"], "capabilities": [{"id": "store.get", "effect": "read", "input": "Record {id: String}", "output": "String", "description": "Gets one."}]}"#).expect("mixed");
    let grants = s.base.join("mixed-grants.json");
    fs::write(&grants, r#"{"store.get": null}"#).expect("grants");
    let foreign = s.program("(call store.get {:id \"a\"})");
    let native = run_sx(&["run", &foreign, "--declarations", text(&mixed), "--grants", text(&grants)]);
    assert_eq!(native.code, 64, "{native:?}");
    assert!(native.stderr.contains("no pack implements store.get"), "{}", native.stderr);
}

#[test]
fn a_fixture_run_with_grants_refuses_out_of_scope_requests_before_the_fixtures() {
    let s = Sandbox::new("fixture");
    let responses = s.base.join("responses.json");
    let secret = text(&s.outside.join("secret.txt")).to_string();
    fs::write(
        &responses,
        format!(r#"[{{"capability": "fs.read-text", "input": {{"path": "{secret}"}}, "results": [{{"output": "leaked"}}]}}]"#),
    )
    .expect("responses");
    let source = format!("(attempt (call fs.read-text {{:path (str {} \"/secret.txt\")}}))", quoted(text(&s.outside)));
    let ran = s.sx("run", &source, &["--responses", text(&responses), "--format", "json"]);
    let outcome = parse(&ran.stdout).expect("json");
    let result = outcome.get("result").expect("result");
    assert_eq!(error_kind(result).0, "PermissionDenied", "{}", ran.stdout);
}
