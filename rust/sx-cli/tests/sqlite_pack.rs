use std::fs;
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
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("sqlite-pack-{}-{label}-{serial}", std::process::id()));
        if base.exists() {
            fs::remove_dir_all(&base).expect("old sandbox removed");
        }
        fs::create_dir_all(base.join("root")).expect("root");
        fs::create_dir_all(base.join("outside")).expect("outside");
        let base = fs::canonicalize(base).expect("canonical sandbox");
        let sandbox = Sandbox { root: base.join("root"), outside: base.join("outside"), base };
        sandbox.write_declarations();
        sandbox.write_grants();
        sandbox
    }

    fn database(&self) -> String {
        text(&self.root.join("store.db")).to_string()
    }

    fn write_declarations(&self) {
        let db = self.database();
        let outside = text(&self.outside.join("secret.db")).to_string();
        let statement = |id: &str, database: &str, sql: &str, input: &str, output: &str, effect: &str, idempotent: bool| {
            Json::object([
                ("id", Json::string(id)),
                ("sqlite", Json::object([("database", Json::string(database)), ("statement", Json::string(sql))])),
                ("input", Json::string(input)),
                ("output", Json::string(output)),
                ("effect", Json::string(effect)),
                ("idempotent", Json::Bool(idempotent)),
            ])
        };
        let declarations = Json::object([
            ("packs", Json::Array(vec![Json::string("sqlite")])),
            (
                "capabilities",
                Json::Array(vec![
                    statement(
                        "store.add-repo",
                        &db,
                        "INSERT INTO repo(name, stars, note) VALUES (:name, :stars, :note) ON CONFLICT(name) DO UPDATE SET stars = :stars",
                        "Record {name: String, stars: Int, note: Option<String>}",
                        "Int",
                        "write",
                        true,
                    ),
                    statement(
                        "store.repos",
                        &db,
                        "SELECT name, stars, note FROM repo WHERE stars >= :min ORDER BY name",
                        "Record {min: Int}",
                        "List<Record {name: String, stars: Int, note: Option<String>}>",
                        "read",
                        false,
                    ),
                    statement("store.secret", &outside, "SELECT 1 AS one", "Record {}", "List<Record {one: Int}>", "read", false),
                ]),
            ),
        ]);
        fs::write(self.base.join("declarations.json"), dumps(&declarations, Layout::Spaced).expect("declarations"))
            .expect("declarations file");
    }

    fn write_grants(&self) {
        let scope = Json::object([("roots", Json::Array(vec![Json::string(text(&self.root))]))]);
        let ids = ["sqlite.execute", "store.add-repo", "store.repos", "store.secret"];
        let grants = Json::Object(ids.iter().map(|id| ((*id).to_string(), scope.clone())).collect());
        fs::write(self.base.join("grants.json"), dumps(&grants, Layout::Spaced).expect("grants")).expect("grants file");
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

    fn outcome(&self, source: &str) -> Json {
        let ran = self.sx("run", source, &["--format", "json"]);
        assert!(ran.code == 0 || ran.code == 2, "sx run failed: {ran:?}");
        parse(&ran.stdout).unwrap_or_else(|e| panic!("{e}: {}", ran.stdout))
    }

    fn result(&self, source: &str) -> Json {
        let outcome = self.outcome(source);
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

fn schema(s: &Sandbox) -> String {
    format!(
        "(call sqlite.execute {{:database \"{}\" :sql \"CREATE TABLE IF NOT EXISTS repo (name TEXT PRIMARY KEY, stars INTEGER NOT NULL, note TEXT)\"}})",
        s.database()
    )
}

#[test]
fn a_program_creates_a_schema_writes_and_reads_typed_rows() {
    let s = Sandbox::new("workflow");
    let source = format!(
        "(let [_ {schema} _ (call store.add-repo {{:name \"sx\" :stars 3 :note (some \"lang\")}}) _ (call store.add-repo {{:name \"kache\" :stars 9}}) n (call store.add-repo {{:name \"sx\" :stars 4 :note none}})] {{:changed n :all (call store.repos {{:min 0}}) :popular (call store.repos {{:min 5}})}})",
        schema = schema(&s)
    );
    let result = s.result(&source);
    assert_eq!(
        shown(&result),
        r#"{"changed": 1, "all": [{"name": "kache", "stars": 9, "note": null}, {"name": "sx", "stars": 4, "note": "lang"}], "popular": [{"name": "kache", "stars": 9, "note": null}]}"#
    );
    assert!(Path::new(&s.database()).exists());
}

fn error_kind(result: &Json) -> (&str, &str) {
    let error = result.get("err").unwrap_or_else(|| panic!("expected an error: {}", shown(result)));
    (error.get("kind").and_then(Json::as_str).expect("kind"), error.get("message").and_then(Json::as_str).expect("message"))
}

#[test]
fn database_errors_reach_the_program_as_call_error_kinds() {
    let s = Sandbox::new("errors");
    let db = s.database();
    let absent = s.result("(attempt (call store.repos {:min 0}))");
    assert_eq!(error_kind(&absent).0, "NotFound");
    let other = s.result(&format!("(attempt (call sqlite.execute {{:database \"{db}\" :sql \"CREATE TABLE other (a UNIQUE); INSERT INTO other VALUES (1)\"}}))"));
    assert_eq!(shown(&other), r#"{"ok": null}"#);
    let no_table = s.result("(attempt (call store.repos {:min 0}))");
    let (kind, message) = error_kind(&no_table);
    assert_eq!(kind, "Invalid");
    assert!(message.contains("no such table"), "{message}");
    let duplicate = s.result(&format!("(attempt (call sqlite.execute {{:database \"{db}\" :sql \"INSERT INTO other VALUES (1)\"}}))"));
    let (kind, message) = error_kind(&duplicate);
    assert_eq!(kind, "Conflict");
    assert!(message.contains("UNIQUE constraint failed"), "{message}");
    let bad_sql = s.result(&format!("(attempt (call sqlite.execute {{:database \"{db}\" :sql \"CREAT TABLE x (a)\"}}))"));
    assert_eq!(error_kind(&bad_sql).0, "Failed");
    let kept = s.result(&format!("(attempt (retry {{:delays [1ms 1ms] :on [Conflict]}} (call sqlite.execute {{:database \"{db}\" :sql \"INSERT INTO other VALUES (1)\"}})))"));
    assert_eq!(error_kind(&kept).0, "Conflict");
}

#[test]
fn a_literal_database_outside_the_grant_is_rejected_before_anything_runs() {
    let s = Sandbox::new("static");
    let secret = text(&s.outside.join("secret.db")).to_string();
    for source in [
        "(call store.secret {})".to_string(),
        format!("(call sqlite.execute {{:database \"{secret}\" :sql \"CREATE TABLE t (a)\"}})"),
    ] {
        let checked = s.sx("check", &source, &[]);
        assert_eq!(checked.code, 1, "{checked:?}");
        assert!(checked.stdout.contains("error[out-of-scope]"), "{}", checked.stdout);
        assert!(checked.stdout.contains("outside every granted root"), "{}", checked.stdout);
        let ran = s.sx("run", &source, &["--format", "json"]);
        assert_eq!(ran.code, 1, "{ran:?}");
        assert!(ran.stdout.contains(r#""code": "out-of-scope""#), "{}", ran.stdout);
    }
    assert!(!s.outside.join("secret.db").exists());
    let computed = format!("(attempt (call sqlite.execute {{:database (str \"{}\" \"/secret.db\") :sql \"CREATE TABLE t (a)\"}}))", text(&s.outside));
    let result = s.result(&computed);
    let (kind, message) = error_kind(&result);
    assert_eq!(kind, "PermissionDenied");
    assert!(message.contains("outside every granted root"), "{message}");
    assert!(!s.outside.join("secret.db").exists());
}

#[test]
fn explain_reports_each_statement_with_its_effect_and_verdict() {
    let s = Sandbox::new("explain");
    let source = format!(
        "[(call store.add-repo {{:name \"a\" :stars 1 :note none}}) (call store.repos {{:min 0}}) (call store.secret {{}}) {schema}]",
        schema = schema(&s)
    );
    let explained = s.sx("explain", &source, &[]);
    assert_eq!(explained.code, 0, "{explained:?}");
    let out = &explained.stdout;
    assert!(out.contains("  store.add-repo  [write, idempotent]  allowed\n    input: "), "{out}");
    assert!(out.contains("  store.repos  [read]  allowed\n"), "{out}");
    assert!(out.contains("  sqlite.execute  [write, not idempotent]  allowed\n"), "{out}");
    assert!(out.contains("  store.secret  [read]  allowed\n    input: {}  scope: denied: "), "{out}");
}

#[test]
fn declaration_mistakes_are_usage_errors_naming_the_statement() {
    let s = Sandbox::new("usage");
    let bad = s.base.join("bad.json");
    fs::write(
        &bad,
        r#"{"packs": ["sqlite"], "capabilities": [{"id": "store.bad", "sqlite": {"database": "/d.db", "statement": "SELECT :id"}, "input": "Record {}", "output": "String", "effect": "read"}]}"#,
    )
    .expect("declarations");
    let program = s.program("1");
    let ran = run_sx(&["check", &program, "--declarations", text(&bad)]);
    assert_eq!(ran.code, 64, "{ran:?}");
    assert!(ran.stderr.contains("statement 'store.bad': parameter :id is not a field of the input"), "{}", ran.stderr);
}
