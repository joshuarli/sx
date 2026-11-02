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
        let ids = [
            "fs.read-text",
            "fs.read-bytes",
            "fs.list",
            "fs.stat",
            "fs.write-text",
            "fs.make-directory",
            "fs.move",
            "fs.delete",
        ];
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

fn tree(root: &Path) -> Vec<String> {
    fn walk(directory: &Path, root: &Path, out: &mut Vec<String>) {
        let mut entries: Vec<_> = fs::read_dir(directory).expect("directory").map(|e| e.expect("entry").path()).collect();
        entries.sort();
        for path in entries {
            let relative = path.strip_prefix(root).expect("inside").display().to_string();
            let metadata = fs::symlink_metadata(&path).expect("metadata");
            if metadata.file_type().is_symlink() {
                out.push(format!("{relative} -> {}", fs::read_link(&path).expect("link").display()));
            } else if metadata.is_dir() {
                out.push(format!("{relative}/"));
                walk(&path, root, out);
            } else {
                out.push(format!("{relative} = {}", fs::read_to_string(&path).expect("text")));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out
}

fn contents(s: &Sandbox) -> (Vec<String>, Vec<String>) {
    (tree(&s.root), tree(&s.outside))
}

fn attempted(s: &Sandbox, call: &str) -> (String, String) {
    let result = s.result(&format!("(attempt {call})"));
    assert!(result.get("err").is_some(), "{call} succeeded");
    let (kind, message) = error_kind(&result);
    (kind.to_string(), message.to_string())
}

#[test]
fn make_directory_creates_parents_and_is_idempotent() {
    let s = Sandbox::new("mkdir");
    let nested = s.at("a/b/c");
    let source = format!("(let [_ (call fs.make-directory {{:path {p}}}) _ (call fs.make-directory {{:path {p}}})] (call fs.stat {{:path {p}}}))", p = quoted(&nested));
    assert!(shown(&s.result(&source)).starts_with(r#"{"kind": "Directory""#));
    assert!(s.root.join("a/b/c").is_dir());
    s.result(&format!("(call fs.make-directory {{:path {}}})", quoted(&s.at("sub"))));
    let explained = s.sx("explain", &format!("(call fs.make-directory {{:path {}}})", quoted(&nested)), &[]);
    assert!(explained.stdout.contains("fs.make-directory  [write, idempotent]  allowed"), "{}", explained.stdout);
}

#[test]
fn make_directory_refuses_what_is_not_a_plain_new_or_existing_directory() {
    let s = Sandbox::new("mkdir-refused");
    symlink(&s.outside, s.root.join("escape")).expect("link");
    symlink(s.root.join("sub"), s.root.join("sub-link")).expect("link");
    let before = contents(&s);
    for path in [
        s.at("note.txt"),
        s.at("sub-link"),
        s.at("note.txt/child"),
    ] {
        let (kind, message) = attempted(&s, &format!("(call fs.make-directory {{:path {}}})", quoted(&path)));
        assert_eq!(kind, "Invalid", "{path}: {message}");
    }
    let denied = [
        text(&s.base).to_string(),
        text(&s.outside.join("new")).to_string(),
        s.at("escape/new"),
        s.at("escape/deeper/new"),
        s.at("missing/../escaped"),
        format!("{}-old/x", text(&s.root)),
    ];
    for path in &denied {
        let (kind, message) = attempted(&s, &format!("(call fs.make-directory {{:path (str {})}})", quoted(path)));
        assert_eq!(kind, "PermissionDenied", "{path}: {message}");
    }
    assert_eq!(contents(&s), before);
}

#[test]
fn make_directory_of_a_granted_root_succeeds_and_changes_nothing() {
    let s = Sandbox::new("mkdir-root");
    let before = contents(&s);
    let root = text(&s.root).to_string();
    for path in [root.clone(), format!("{root}/"), s.at("sub/..")] {
        let literal = s.result(&format!("(call fs.make-directory {{:path {}}})", quoted(&path)));
        assert_eq!(shown(&literal), "null", "{path}");
        let computed = s.result(&format!("(call fs.make-directory {{:path (str {})}})", quoted(&path)));
        assert_eq!(shown(&computed), "null", "{path}");
    }
    let checked = s.sx("check", &format!("(call fs.make-directory {{:path {}}})", quoted(&root)), &[]);
    assert_eq!((checked.code, checked.stdout.as_str()), (0, "ok: Unit\n"));
    for source in [
        format!("(call fs.delete {{:path (str {})}})", quoted(&root)),
        format!("(call fs.move {{:from (str {}) :to {}}})", quoted(&root), quoted(&s.at("elsewhere"))),
        format!("(call fs.write-text {{:path (str {}) :text \"x\"}})", quoted(&root)),
    ] {
        let (kind, message) = attempted(&s, &source);
        assert_eq!(kind, "PermissionDenied", "{source}: {message}");
    }
    assert_eq!(contents(&s), before);
}

#[test]
fn move_renames_files_symlinks_and_directories_without_following() {
    let s = Sandbox::new("move");
    symlink(s.outside.join("secret.txt"), s.root.join("link")).expect("link");
    fs::write(s.root.join("sub/inner.txt"), "inner").expect("inner");
    s.result(&format!("(call fs.move {{:from {} :to {}}})", quoted(&s.at("note.txt")), quoted(&s.at("sub/moved.txt"))));
    s.result(&format!("(call fs.move {{:from {} :to {}}})", quoted(&s.at("link")), quoted(&s.at("renamed-link"))));
    s.result(&format!("(call fs.move {{:from {} :to {}}})", quoted(&s.at("sub")), quoted(&s.at("dir"))));
    assert_eq!(
        tree(&s.root),
        [
            "dir/".to_string(),
            "dir/inner.txt = inner".to_string(),
            "dir/moved.txt = inside".to_string(),
            format!("renamed-link -> {}", s.outside.join("secret.txt").display()),
        ]
    );
    assert_eq!(fs::read_to_string(s.outside.join("secret.txt")).expect("secret"), "secret");
    let explained = s.sx("explain", &format!("(call fs.move {{:from {} :to {}}})", quoted(&s.at("a")), quoted(&s.at("b"))), &[]);
    assert!(explained.stdout.contains("fs.move  [write, not idempotent]  allowed"), "{}", explained.stdout);
}

#[test]
fn move_never_replaces_and_leaves_both_sides_intact() {
    let s = Sandbox::new("move-conflict");
    fs::write(s.root.join("other.txt"), "other").expect("other");
    fs::create_dir(s.root.join("empty")).expect("empty");
    let before = contents(&s);
    for to in ["other.txt", "empty", "sub"] {
        let (kind, message) =
            attempted(&s, &format!("(call fs.move {{:from {} :to {}}})", quoted(&s.at("note.txt")), quoted(&s.at(to))));
        assert_eq!(kind, "Conflict", "{to}: {message}");
    }
    let (kind, _) = attempted(&s, &format!("(call fs.move {{:from {p} :to {p}}})", p = quoted(&s.at("note.txt"))));
    assert_eq!(kind, "Conflict");
    let (kind, _) = attempted(&s, &format!("(call fs.move {{:from {} :to {}}})", quoted(&s.at("absent")), quoted(&s.at("new"))));
    assert_eq!(kind, "NotFound");
    let (kind, _) = attempted(&s, &format!("(call fs.move {{:from {} :to {}}})", quoted(&s.at("note.txt")), quoted(&s.at("nowhere/new"))));
    assert_eq!(kind, "NotFound");
    let (kind, _) = attempted(&s, &format!("(call fs.move {{:from {} :to {}}})", quoted(&s.at("sub")), quoted(&s.at("sub/inside"))));
    assert_eq!(kind, "Invalid");
    assert_eq!(contents(&s), before);
}

#[test]
fn move_outside_the_roots_is_refused_on_either_side_before_anything_runs() {
    let s = Sandbox::new("move-refused");
    symlink(&s.outside, s.root.join("escape")).expect("link");
    symlink(s.root.join("note.txt"), s.root.join("note-link")).expect("link");
    let secret = text(&s.outside.join("secret.txt")).to_string();
    let before = contents(&s);
    let pairs = [
        (s.at("note.txt"), format!("{}/stolen.txt", text(&s.outside))),
        (secret.clone(), s.at("taken.txt")),
        (s.at("escape/secret.txt"), s.at("taken.txt")),
        (s.at("note.txt"), s.at("escape/stolen.txt")),
        (text(&s.root).to_string(), s.at("elsewhere")),
        (s.at("note.txt"), text(&s.root).to_string()),
        (s.at("note.txt"), s.at("missing/../x")),
        (s.at("note.txt"), s.at("note-link")),
        (s.at("note.txt"), s.at("sub/..")),
    ];
    for (from, to) in &pairs {
        let (kind, message) =
            attempted(&s, &format!("(call fs.move {{:from (str {}) :to (str {})}})", quoted(from), quoted(to)));
        assert_eq!(kind, "PermissionDenied", "{from} -> {to}: {message}");
    }
    assert_eq!(contents(&s), before);
}

#[test]
fn a_literal_move_out_of_scope_is_a_static_error_and_explain_judges_both_fields() {
    let s = Sandbox::new("move-static");
    let outside = text(&s.outside.join("stolen.txt")).to_string();
    let secret = text(&s.outside.join("secret.txt")).to_string();
    let before = contents(&s);
    for (from, to) in [(s.at("note.txt"), outside.clone()), (secret.clone(), s.at("taken.txt"))] {
        let source = format!("(call fs.move {{:from {} :to {}}})", quoted(&from), quoted(&to));
        let checked = s.sx("check", &source, &[]);
        assert_eq!(checked.code, 1, "{checked:?}");
        assert!(checked.stdout.starts_with("error[out-of-scope]"), "{}", checked.stdout);
        assert!(checked.stdout.contains("outside every granted root"), "{}", checked.stdout);
        let ran = s.sx("run", &source, &["--format", "json"]);
        assert_eq!(ran.code, 1, "{ran:?}");
        assert!(ran.stdout.contains(r#""code": "out-of-scope""#), "{}", ran.stdout);
    }
    let source = format!(
        "[(call fs.move {{:from {} :to {}}}) (call fs.move {{:from {} :to {}}}) (call fs.move {{:from {} :to {}}})]",
        quoted(&s.at("note.txt")),
        quoted(&s.at("new.txt")),
        quoted(&s.at("note.txt")),
        quoted(&outside),
        quoted(&s.at("note.txt")),
        quoted(&s.at("new.txt")),
    );
    let explained = s.sx("explain", &source, &["--format", "json"]);
    let report = parse(&explained.stdout).expect("json");
    let tool = report.get("tools").and_then(Json::as_array).and_then(|tools| tools.first()).expect("a tool");
    let verdicts: Vec<&str> = tool
        .get("verdicts")
        .and_then(Json::as_array)
        .expect("verdicts")
        .iter()
        .map(|v| v.get("verdict").and_then(Json::as_str).expect("verdict"))
        .collect();
    assert_eq!(verdicts, ["allowed", "denied", "allowed"]);
    let text_form = s.sx("explain", &source, &[]);
    assert!(text_form.stdout.contains("scope: denied: ") && text_form.stdout.contains("outside every granted root"), "{}", text_form.stdout);
    assert_eq!(contents(&s), before);
}

#[test]
fn delete_removes_files_symlinks_and_empty_directories_but_never_a_target() {
    let s = Sandbox::new("delete");
    symlink(s.outside.join("secret.txt"), s.root.join("link")).expect("link");
    symlink(&s.outside, s.root.join("dir-link")).expect("dir link");
    fs::create_dir(s.root.join("empty")).expect("empty");
    for name in ["note.txt", "link", "dir-link", "empty"] {
        s.result(&format!("(call fs.delete {{:path {}}})", quoted(&s.at(name))));
    }
    assert_eq!(tree(&s.root), ["sub/".to_string()]);
    assert_eq!(tree(&s.outside), ["secret.txt = secret".to_string()]);
    let (kind, _) = attempted(&s, &format!("(call fs.delete {{:path {}}})", quoted(&s.at("note.txt"))));
    assert_eq!(kind, "NotFound");
    let explained = s.sx("explain", &format!("(call fs.delete {{:path {}}})", quoted(&s.at("note.txt"))), &[]);
    assert!(explained.stdout.contains("fs.delete  [write, idempotent]  allowed"), "{}", explained.stdout);
}

#[test]
fn delete_of_a_non_empty_directory_fails_and_removes_nothing() {
    let s = Sandbox::new("delete-nonempty");
    fs::create_dir_all(s.root.join("sub/deep")).expect("deep");
    fs::write(s.root.join("sub/deep/x.txt"), "x").expect("x");
    let before = contents(&s);
    let (kind, message) = attempted(&s, &format!("(call fs.delete {{:path {}}})", quoted(&s.at("sub"))));
    assert_eq!(kind, "Invalid", "{message}");
    assert_eq!(contents(&s), before);
}

#[test]
fn delete_outside_the_roots_is_refused_and_touches_nothing() {
    let s = Sandbox::new("delete-refused");
    symlink(&s.outside, s.root.join("escape")).expect("link");
    let before = contents(&s);
    let paths = [
        text(&s.outside.join("secret.txt")).to_string(),
        s.at("escape/secret.txt"),
        text(&s.root).to_string(),
        text(&s.base).to_string(),
        s.at("sub/.."),
        s.at("missing/../note.txt"),
        format!("{}-old/note.txt", text(&s.root)),
    ];
    for path in &paths {
        let (kind, message) = attempted(&s, &format!("(call fs.delete {{:path (str {})}})", quoted(path)));
        assert_eq!(kind, "PermissionDenied", "{path}: {message}");
    }
    let literal = s.sx("check", &format!("(call fs.delete {{:path {}}})", quoted(&paths[0])), &[]);
    assert_eq!(literal.code, 1, "{literal:?}");
    assert!(literal.stdout.starts_with("error[out-of-scope]"), "{}", literal.stdout);
    assert_eq!(contents(&s), before);
}
