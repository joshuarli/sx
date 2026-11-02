use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rusqlite::Connection;
use sx_kernel::authority::{Grant, Grants, Screen, Verdict};
use sx_kernel::json::{Json, Layout, dumps, parse};
use sx_kernel::registry::{Effect, Registry, load_declarations_with};
use sx_kernel::types::CallErrorKind;
use sx_kernel::values::{Value, decode, show_value};
use sx_packs::sqlite::{EXECUTE, Limits, execute_with};
use sx_packs::{Declared, DeclaredLoader, Failure, pack};

static SERIAL: AtomicUsize = AtomicUsize::new(0);

const FAST: Limits = Limits { busy_timeout: Duration::from_millis(150), max_rows: 5 };

struct Sandbox {
    root: PathBuf,
    outside: PathBuf,
}

impl Sandbox {
    fn new(label: &str) -> Sandbox {
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("sqlite-{}-{label}-{serial}", std::process::id()));
        if base.exists() {
            fs::remove_dir_all(&base).expect("old sandbox removed");
        }
        fs::create_dir_all(base.join("root")).expect("root");
        fs::create_dir_all(base.join("outside")).expect("outside");
        let base = fs::canonicalize(base).expect("canonical");
        Sandbox { root: base.join("root"), outside: base.join("outside") }
    }

    fn db(&self, name: &str) -> String {
        text(&self.root.join(name)).to_string()
    }

    fn outside_db(&self, name: &str) -> String {
        text(&self.outside.join(name)).to_string()
    }

    fn open(&self, name: &str) -> Connection {
        Connection::open(self.root.join(name)).expect("the database opens")
    }
}

fn text(path: &Path) -> &str {
    path.to_str().expect("UTF-8 path")
}

struct World {
    registry: Registry,
    declared: Declared,
}

fn world(declarations: &Json) -> Result<World, String> {
    let mut registry = Registry::new();
    registry.add_pack(pack("sqlite").expect("the sqlite pack")).expect("registers");
    let mut loader = DeclaredLoader::new();
    load_declarations_with(&mut registry, declarations, &mut loader).map_err(|e| e.to_string())?;
    Ok(World { registry, declared: loader.finish() })
}

fn statement(id: &str, database: &str, sql: &str, input: &str, output: &str, effect: &str) -> Json {
    Json::object([
        ("id", Json::string(id)),
        ("sqlite", Json::object([("database", Json::string(database)), ("statement", Json::string(sql))])),
        ("input", Json::string(input)),
        ("output", Json::string(output)),
        ("effect", Json::string(effect)),
    ])
}

fn declarations(statements: Vec<Json>) -> Json {
    Json::object([("capabilities", Json::Array(statements))])
}

fn one(database: &str, sql: &str, input: &str, output: &str, effect: &str) -> Result<World, String> {
    world(&declarations(vec![statement("db.call", database, sql, input, output, effect)]))
}

fn grants(world: &World, roots: &[&str]) -> Grants {
    let scope = Json::object([("roots", Json::Array(roots.iter().map(|r| Json::string(*r)).collect()))]);
    let mut all = vec![(EXECUTE.to_string(), scope.clone())];
    all.extend(world.declared.statements.ids().map(|id| (id.to_string(), scope.clone())));
    Grants::load(&world.registry, &Json::Object(all)).expect("the grants load")
}

fn scope_of(grants: &Grants, id: &str) -> Value {
    match grants.get(id) {
        Some(Grant::Scoped { scope, .. }) => scope.clone(),
        other => panic!("{id} is not granted with a scope: {other:?}"),
    }
}

fn input(world: &World, id: &str, json: &str) -> Value {
    let capability = world.registry.capability(id).expect("declared");
    decode(&parse(json).expect("JSON"), &capability.input, "$").expect("the input decodes")
}

fn call(world: &World, grants: &Grants, json: &str) -> Result<Value, Failure> {
    call_as(world, grants, "db.call", json)
}

fn call_as(world: &World, grants: &Grants, id: &str, json: &str) -> Result<Value, Failure> {
    let statement = world.declared.statements.get(id).expect("bound");
    statement.perform_with(&FAST, &scope_of(grants, id), &input(world, id, json))
}

fn run_execute(world: &World, grants: &Grants, database: &str, sql: &str) -> Result<Value, Failure> {
    let json = dumps(&Json::object([("database", Json::string(database)), ("sql", Json::string(sql))]), Layout::Spaced)
        .expect("JSON");
    execute_with(&FAST, &scope_of(grants, EXECUTE), &input(world, EXECUTE, &json))
}

fn verdict(world: &World, grants: &Grants, id: &str, json: &str) -> Verdict {
    match grants.screen(id, &input(world, id, json)) {
        Screen::Verdict(verdict) => verdict,
        other => panic!("no scope check: {other:?}"),
    }
}

fn shown(result: Result<Value, Failure>) -> String {
    match result {
        Ok(value) => show_value(&value),
        Err(failure) => panic!("expected a value: {failure}"),
    }
}

fn refused(result: Result<Value, Failure>) -> Failure {
    match result {
        Ok(value) => panic!("expected a failure, got {}", show_value(&value)),
        Err(failure) => failure,
    }
}

fn setup(sandbox: &Sandbox, sql: &str) {
    sandbox.open("app.db").execute_batch(sql).expect("setup SQL runs");
}

const REPO_TABLE: &str = "CREATE TABLE repo (name TEXT PRIMARY KEY, stars INTEGER NOT NULL, note TEXT, score REAL, active INTEGER NOT NULL DEFAULT 1, raw BLOB)";

#[test]
fn a_statement_registers_as_a_scoped_capability() {
    let s = Sandbox::new("register");
    let world = one(&s.db("app.db"), "SELECT 1 AS one", "Record {}", "List<Record {one: Int}>", "read").expect("declares");
    let capability = world.registry.capability("db.call").expect("registered");
    assert_eq!(capability.effect, Effect::Read);
    assert!(capability.scope.is_some());
    assert!(world.registry.scope_check("db.call").is_some());
    assert_eq!(capability.description, "SELECT 1 AS one");
    assert_eq!(world.declared.statements.ids().collect::<Vec<_>>(), ["db.call"]);
}

#[test]
fn declaration_errors_are_reported_at_load() {
    let db = "/data/app.db";
    let cases: Vec<(Result<World, String>, &str)> = vec![
        (one(db, "INSERT INTO t(a) VALUES (:a)", "Record {}", "Int", "write"), "parameter :a is not a field of the input"),
        (one(db, "INSERT INTO t(a) VALUES (1)", "Record {a: Int}", "Int", "write"), "input field 'a' is not used by a :a parameter"),
        (one(db, "INSERT INTO t(a) VALUES (?)", "Record {}", "Int", "write"), "starts a parameter form this pack does not bind"),
        (one(db, "INSERT INTO t(a) VALUES (@a)", "Record {a: Int}", "Int", "write"), "starts a parameter form this pack does not bind"),
        (one(db, "INSERT INTO t(a) VALUES (:a)", "Record {a: List<Int>}", "Int", "write"), "input field 'a' has type List<Int>"),
        (one(db, "SELECT :a", "Record {a: Record {b: Int}}", "List<Record {x: Int}>", "read"), "input field 'a' has type"),
        (one(db, "DELETE FROM t", "Record {}", "Int", "read"), "the effect must be write"),
        (one(db, "SELECT 1", "Record {}", "String", "read"), "the output must be Int or List<Record"),
        (one(db, "SELECT 1", "Record {}", "List<Int>", "read"), "the output must be Int or List<Record"),
        (one(db, "SELECT 1 AS x", "Record {}", "List<Record {x: List<Int>}>", "read"), "output field 'x' has type List<Int>"),
        (one(db, "SELECT 1", "String", "Int", "write"), "the input must be a record"),
        (one("relative.db", "SELECT 1", "Record {}", "Int", "write"), "is not an absolute path"),
        (one(db, "   ", "Record {}", "Int", "write"), "the statement is empty"),
        (one(db, "SELECT 1", "Record {}", "Int", "maybe"), "is not a valid Effect"),
        (one(db, "SELECT :\u{e9}", "Record {}", "Int", "write"), "not a name of letters"),
    ];
    for (outcome, expected) in cases {
        match outcome {
            Ok(_) => panic!("expected a load error containing {expected:?}"),
            Err(message) => {
                assert!(message.contains(expected), "{message:?} lacks {expected:?}");
                assert!(message.starts_with("statement 'db.call': "), "{message}");
            }
        }
    }
}

#[test]
fn malformed_declarations_name_the_mistake() {
    let mut entry = statement("db.call", "/data/app.db", "SELECT 1", "Record {}", "Int", "write");
    let Json::Object(fields) = &mut entry else { panic!("object") };
    fields.push(("scope".to_string(), Json::string("Record {roots: List<String>}")));
    let message = world(&declarations(vec![entry])).err().expect("refused");
    assert!(message.contains("scope is implied"), "{message}");
    let extra = Json::object([
        ("id", Json::string("db.call")),
        ("sqlite", Json::object([("database", Json::string("/d.db")), ("statement", Json::string("SELECT 1")), ("mode", Json::string("x"))])),
        ("input", Json::string("Record {}")),
        ("output", Json::string("Int")),
        ("effect", Json::string("write")),
    ]);
    let message = world(&declarations(vec![extra])).err().expect("refused");
    assert!(message.contains("sqlite has an unknown key 'mode'"), "{message}");
    let missing = Json::object([("id", Json::string("db.call")), ("sqlite", Json::Object(Vec::new()))]);
    let message = world(&declarations(vec![missing])).err().expect("refused");
    assert!(message.contains("a statement needs 'effect'"), "{message}");
}

#[test]
fn every_statement_kind_runs_in_its_own_transaction() {
    let s = Sandbox::new("kinds");
    let db = s.db("app.db");
    let all = world(&declarations(vec![
        statement(
            "db.add",
            &db,
            "INSERT INTO repo(name, stars) VALUES (:name, :stars)",
            "Record {name: String, stars: Int}",
            "Int",
            "write",
        ),
        statement("db.bump", &db, "UPDATE repo SET stars = stars + :by", "Record {by: Int}", "Int", "write"),
        statement("db.drop", &db, "DELETE FROM repo WHERE stars < :below", "Record {below: Int}", "Int", "write"),
        statement("db.all", &db, "SELECT name, stars FROM repo ORDER BY name", "Record {}", "List<Record {name: String, stars: Int}>", "read"),
        statement(
            "db.add-returning",
            &db,
            "INSERT INTO repo(name, stars) VALUES (:name, 0) RETURNING name, stars",
            "Record {name: String}",
            "List<Record {name: String, stars: Int}>",
            "write",
        ),
    ]))
    .expect("declares");
    let g = grants(&all, &[text(&s.root)]);
    assert_eq!(shown(run_execute(&all, &g, &db, REPO_TABLE)), "unit");
    assert_eq!(shown(call_as(&all, &g, "db.add", r#"{"name": "a", "stars": 1}"#)), "1");
    assert_eq!(shown(call_as(&all, &g, "db.add", r#"{"name": "b", "stars": 5}"#)), "1");
    assert_eq!(shown(call_as(&all, &g, "db.add-returning", r#"{"name": "c"}"#)), r#"[{:name "c" :stars 0}]"#);
    assert_eq!(
        shown(call_as(&all, &g, "db.all", "{}")),
        r#"[{:name "a" :stars 1} {:name "b" :stars 5} {:name "c" :stars 0}]"#
    );
    assert_eq!(shown(call_as(&all, &g, "db.bump", r#"{"by": 10}"#)), "3");
    assert_eq!(shown(call_as(&all, &g, "db.drop", r#"{"below": 12}"#)), "2");
    assert_eq!(shown(call_as(&all, &g, "db.drop", r#"{"below": 12}"#)), "0");
    assert_eq!(shown(call_as(&all, &g, "db.all", "{}")), r#"[{:name "b" :stars 15}]"#);
}

#[test]
fn a_statement_with_effect_and_idempotence_declared_reports_them() {
    let s = Sandbox::new("idempotent");
    let mut entry = statement("db.call", &s.db("app.db"), "DELETE FROM t", "Record {}", "Int", "write");
    let Json::Object(fields) = &mut entry else { panic!("object") };
    fields.push(("idempotent".to_string(), Json::Bool(true)));
    let declared = world(&declarations(vec![entry])).expect("declares");
    assert!(declared.registry.capability("db.call").expect("registered").idempotent);
    let execute = declared.registry.capability(EXECUTE).expect("registered");
    assert_eq!((execute.effect, execute.idempotent), (Effect::Write, false));
}

#[test]
fn parameters_bind_by_name_with_every_type_and_never_as_sql() {
    let s = Sandbox::new("binding");
    let db = s.db("app.db");
    let all = world(&declarations(vec![
        statement(
            "db.put",
            &db,
            "INSERT INTO repo(name, stars, note, score, active, raw) VALUES (:name, :stars, :note, :score, :active, :raw)",
            "Record {name: String, stars: Int, note: Option<String>, score: Option<Float>, active: Bool, raw: Bytes}",
            "Int",
            "write",
        ),
        statement(
            "db.get",
            &db,
            "SELECT name, stars, note, score, active, raw FROM repo ORDER BY name",
            "Record {}",
            "List<Record {name: String, stars: Int, note: Option<String>, score: Option<Float>, active: Bool, raw: Option<Bytes>}>",
            "read",
        ),
        statement(
            "db.twice",
            &db,
            "SELECT :n + :n AS doubled, :n AS once",
            "Record {n: Int}",
            "List<Record {doubled: Int, once: Int}>",
            "read",
        ),
    ]))
    .expect("declares");
    let g = grants(&all, &[text(&s.root)]);
    run_execute(&all, &g, &db, REPO_TABLE).expect("table");
    let hostile = "x'); DROP TABLE repo; --";
    let first = format!(
        r#"{{"name": "{hostile}", "stars": -9223372036854775808, "note": "café", "score": 2.5, "active": false, "raw": "AAEC"}}"#
    );
    assert_eq!(shown(call_as(&all, &g, "db.put", &first)), "1");
    assert_eq!(
        shown(call_as(&all, &g, "db.put", r#"{"name": "b", "stars": 9223372036854775807, "note": null, "score": null, "active": true, "raw": ""}"#)),
        "1"
    );
    let rows = shown(call_as(&all, &g, "db.get", "{}"));
    assert_eq!(
        rows,
        format!(
            r#"[{{:name "b" :stars 9223372036854775807 :note none :score none :active true :raw (some <bytes "">)}} {{:name "{hostile}" :stars -9223372036854775808 :note (some "café") :score (some 2.5) :active false :raw (some <bytes "AAEC">)}}]"#
        )
    );
    assert_eq!(shown(call_as(&all, &g, "db.twice", r#"{"n": 21}"#)), "[{:doubled 42 :once 21}]");
    let stored: i64 = s.open("app.db").query_row("SELECT count(*) FROM repo", [], |row| row.get(0)).expect("count");
    assert_eq!(stored, 2);
}

#[test]
fn rows_decode_by_column_name_with_typed_columns() {
    let s = Sandbox::new("decode");
    setup(&s, &format!("{REPO_TABLE}; INSERT INTO repo(name, stars, note, score) VALUES ('a', 1, NULL, 3), ('b', 2, 'n', 0.5)"));
    let db = s.db("app.db");
    let open = one(&db, "SELECT name AS n, stars, note, score FROM repo ORDER BY name", "Record {}", "List<Record open {score: Float, n: String, note: Option<String>}>", "read")
        .expect("declares");
    let g = grants(&open, &[text(&s.root)]);
    assert_eq!(
        shown(call(&open, &g, "{}")),
        r#"[{:score 3.0 :n "a" :note none} {:score 0.5 :n "b" :note (some "n")}]"#
    );
    let empty = one(&db, "SELECT name FROM repo WHERE 0", "Record {}", "List<Record {name: String}>", "read").expect("declares");
    assert_eq!(shown(call(&empty, &g, "{}")), "[]");
}

#[test]
fn row_decoding_failures_name_the_row_and_column() {
    let s = Sandbox::new("mismatch");
    setup(&s, &format!("{REPO_TABLE}; INSERT INTO repo(name, stars, note, active) VALUES ('a', 1, NULL, 1), ('b', 2, 'n', 7)"));
    let db = s.db("app.db");
    let cases = [
        ("SELECT name FROM repo", "List<Record {name: Int}>", "row 1, column 'name': expected Int, found TEXT"),
        ("SELECT note FROM repo", "List<Record {note: String}>", "row 1, column 'note': expected String, found NULL"),
        ("SELECT stars FROM repo", "List<Record {stars: String}>", "row 1, column 'stars': expected String, found INTEGER"),
        ("SELECT active FROM repo ORDER BY name", "List<Record {active: Bool}>", "row 2, column 'active': expected Bool, found the INTEGER 7"),
        ("SELECT 1.5 AS score", "List<Record {score: Option<Int>}>", "row 1, column 'score': expected Option<Int>, found REAL"),
        ("SELECT 1 AS x", "List<Record {x: Bytes}>", "expected Bytes, found INTEGER"),
        ("SELECT name, stars FROM repo", "List<Record {name: String}>", "returns a column 'stars' that the output record does not declare"),
        ("SELECT name FROM repo", "List<Record {name: String, stars: Int}>", "declares 'stars' (Int), but the statement returns no such column"),
        ("SELECT name, name FROM repo", "List<Record {name: String}>", "a column named 'name' twice"),
        ("SELECT 9007199254740993 AS big", "List<Record {big: Float}>", "an INTEGER too large for an exact Float"),
        ("SELECT 1e999 AS big", "List<Record {big: Float}>", "outside the finite Float range"),
        ("SELECT x'ff' AS t", "List<Record {t: String}>", "expected String, found BLOB"),
        ("SELECT CAST(x'ff' AS TEXT) AS t", "List<Record {t: String}>", "TEXT that is not valid UTF-8"),
    ];
    for (sql, output, expected) in cases {
        let world = one(&db, sql, "Record {}", output, "read").expect("declares");
        let g = grants(&world, &[text(&s.root)]);
        let failure = refused(call(&world, &g, "{}"));
        assert_eq!(failure.kind, CallErrorKind::Invalid, "{sql}: {failure}");
        assert!(failure.message.contains(expected), "{sql}: {failure}");
    }
}

#[test]
fn a_statement_and_a_declared_shape_that_disagree_fail_before_any_row() {
    let s = Sandbox::new("shape");
    setup(&s, REPO_TABLE);
    let db = s.db("app.db");
    let rows_as_count = one(&db, "SELECT name FROM repo", "Record {}", "Int", "write").expect("declares");
    let g = grants(&rows_as_count, &[text(&s.root)]);
    let failure = refused(call(&rows_as_count, &g, "{}"));
    assert_eq!(failure.kind, CallErrorKind::Failed);
    assert!(failure.message.contains("returns rows, so its output must be a List"), "{failure}");
    let count_as_rows = one(&db, "DELETE FROM repo", "Record {}", "List<Record {x: Int}>", "write").expect("declares");
    let failure = refused(call(&count_as_rows, &g, "{}"));
    assert_eq!(failure.kind, CallErrorKind::Failed);
    assert!(failure.message.contains("returns no rows"), "{failure}");
}

#[test]
fn a_failed_write_rolls_the_whole_call_back() {
    let s = Sandbox::new("rollback");
    setup(&s, REPO_TABLE);
    let db = s.db("app.db");
    let mismatch = one(
        &db,
        "INSERT INTO repo(name, stars) VALUES (:name, 3) RETURNING stars",
        "Record {name: String}",
        "List<Record {stars: String}>",
        "write",
    )
    .expect("declares");
    let g = grants(&mismatch, &[text(&s.root)]);
    let failure = refused(call(&mismatch, &g, r#"{"name": "kept-out"}"#));
    assert_eq!(failure.kind, CallErrorKind::Failed, "{failure}");
    let rows: i64 = s.open("app.db").query_row("SELECT count(*) FROM repo", [], |row| row.get(0)).expect("count");
    assert_eq!(rows, 0, "the insert was rolled back with the failed decode");
    let batch = run_execute(&mismatch, &g, &db, "CREATE TABLE one (a); CREATE TABLE two (a); CREATE TABLE one (a)");
    assert_eq!(refused(batch).kind, CallErrorKind::Failed);
    let tables: i64 = s
        .open("app.db")
        .query_row("SELECT count(*) FROM sqlite_master WHERE name IN ('one', 'two')", [], |row| row.get(0))
        .expect("count");
    assert_eq!(tables, 0, "no statement of a failed batch survives");
}

#[test]
fn constraint_violations_are_conflicts_and_change_nothing() {
    let s = Sandbox::new("conflict");
    setup(
        &s,
        "PRAGMA foreign_keys = OFF; CREATE TABLE owner (id INTEGER PRIMARY KEY); CREATE TABLE repo (name TEXT PRIMARY KEY, stars INTEGER NOT NULL CHECK (stars >= 0), owner INTEGER REFERENCES owner(id)); INSERT INTO repo VALUES ('a', 1, NULL)",
    );
    let db = s.db("app.db");
    let cases = [
        ("INSERT INTO repo(name, stars) VALUES ('a', 2)", "UNIQUE constraint failed"),
        ("INSERT INTO repo(name, stars) VALUES ('z', NULL)", "NOT NULL constraint failed"),
        ("INSERT INTO repo(name, stars) VALUES ('z', -1)", "CHECK constraint failed"),
        ("INSERT INTO repo(name, stars, owner) VALUES ('z', 1, 99)", "FOREIGN KEY constraint failed"),
    ];
    for (sql, expected) in cases {
        let world = one(&db, sql, "Record {}", "Int", "write").expect("declares");
        let g = grants(&world, &[text(&s.root)]);
        let failure = refused(call(&world, &g, "{}"));
        assert_eq!(failure.kind, CallErrorKind::Conflict, "{sql}: {failure}");
        assert!(failure.message.contains(expected), "{failure}");
    }
    let rows: i64 = s.open("app.db").query_row("SELECT count(*) FROM repo", [], |row| row.get(0)).expect("count");
    assert_eq!(rows, 1);
}

#[test]
fn busy_databases_are_unavailable_for_reads_and_writes() {
    let s = Sandbox::new("busy");
    setup(&s, &format!("{REPO_TABLE}; INSERT INTO repo(name, stars) VALUES ('a', 1)"));
    let db = s.db("app.db");
    let holder = s.open("app.db");
    holder.execute_batch("BEGIN EXCLUSIVE").expect("exclusive lock");
    let write = one(&db, "INSERT INTO repo(name, stars) VALUES ('b', 1)", "Record {}", "Int", "write").expect("declares");
    let g = grants(&write, &[text(&s.root)]);
    let failure = refused(call(&write, &g, "{}"));
    assert_eq!(failure.kind, CallErrorKind::Unavailable, "{failure}");
    let read = one(&db, "SELECT name FROM repo", "Record {}", "List<Record {name: String}>", "read").expect("declares");
    let failure = refused(call(&read, &g, "{}"));
    assert_eq!(failure.kind, CallErrorKind::Unavailable, "{failure}");
    let failure = refused(run_execute(&read, &g, &db, "CREATE TABLE late (a)"));
    assert_eq!(failure.kind, CallErrorKind::Unavailable, "{failure}");
    holder.execute_batch("ROLLBACK").expect("released");
    assert_eq!(shown(call(&write, &g, "{}")), "1");
}

#[test]
fn sql_errors_are_invalid_for_reads_and_failed_for_writes() {
    let s = Sandbox::new("syntax");
    setup(&s, REPO_TABLE);
    let db = s.db("app.db");
    let reads = [
        ("SELEC name FROM repo", "syntax error"),
        ("SELECT nope FROM repo", "no such column"),
        ("SELECT name FROM missing", "no such table"),
        ("SELECT 1; SELECT 2", "multiple statements"),
    ];
    for (sql, expected) in reads {
        let world = one(&db, sql, "Record {}", "List<Record {x: Int}>", "read").expect("declares");
        let g = grants(&world, &[text(&s.root)]);
        let failure = refused(call(&world, &g, "{}"));
        assert_eq!(failure.kind, CallErrorKind::Invalid, "{sql}: {failure}");
        assert!(failure.message.to_lowercase().contains(expected), "{sql}: {failure}");
    }
    let writes = [("INSER INTO repo VALUES (1)", "syntax error"), ("INSERT INTO missing VALUES (1)", "no such table")];
    for (sql, expected) in writes {
        let world = one(&db, sql, "Record {}", "Int", "write").expect("declares");
        let g = grants(&world, &[text(&s.root)]);
        let failure = refused(call(&world, &g, "{}"));
        assert_eq!(failure.kind, CallErrorKind::Failed, "{sql}: {failure}");
        assert!(failure.message.contains(expected), "{sql}: {failure}");
    }
}

#[test]
fn a_read_that_would_change_the_database_is_refused() {
    let s = Sandbox::new("readonly-statement");
    setup(&s, &format!("{REPO_TABLE}; INSERT INTO repo(name, stars) VALUES ('a', 1)"));
    let db = s.db("app.db");
    let world = one(&db, "DELETE FROM repo RETURNING name", "Record {}", "List<Record {name: String}>", "read").expect("declares");
    let g = grants(&world, &[text(&s.root)]);
    let failure = refused(call(&world, &g, "{}"));
    assert_eq!(failure.kind, CallErrorKind::Invalid, "{failure}");
    assert!(failure.message.contains("cannot be a read"), "{failure}");
    let rows: i64 = s.open("app.db").query_row("SELECT count(*) FROM repo", [], |row| row.get(0)).expect("count");
    assert_eq!(rows, 1);
}

#[test]
fn a_database_the_process_cannot_write_is_permission_denied() {
    let s = Sandbox::new("permission");
    setup(&s, REPO_TABLE);
    let path = s.root.join("app.db");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).expect("read-only file");
    let world = one(&s.db("app.db"), "INSERT INTO repo(name, stars) VALUES ('a', 1)", "Record {}", "Int", "write").expect("declares");
    let g = grants(&world, &[text(&s.root)]);
    let failure = refused(call(&world, &g, "{}"));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("restored");
    assert_eq!(failure.kind, CallErrorKind::PermissionDenied, "{failure}");
}

#[test]
fn missing_and_unusable_databases_map_to_not_found_invalid_and_failed() {
    let s = Sandbox::new("missing");
    let read = one(&s.db("absent.db"), "SELECT 1 AS x", "Record {}", "List<Record {x: Int}>", "read").expect("declares");
    let g = grants(&read, &[text(&s.root)]);
    let failure = refused(call(&read, &g, "{}"));
    assert_eq!(failure.kind, CallErrorKind::NotFound, "{failure}");
    assert!(!s.root.join("absent.db").exists(), "a read never creates the database");
    let write = one(&s.db("no-dir/app.db"), "DELETE FROM t", "Record {}", "Int", "write").expect("declares");
    let failure = refused(call(&write, &g, "{}"));
    assert_eq!(failure.kind, CallErrorKind::NotFound, "{failure}");
    fs::create_dir(s.root.join("folder.db")).expect("directory");
    let directory = one(&s.db("folder.db"), "DELETE FROM t", "Record {}", "Int", "write").expect("declares");
    let failure = refused(call(&directory, &g, "{}"));
    assert_eq!(failure.kind, CallErrorKind::Invalid, "{failure}");
    fs::write(s.root.join("garbage.db"), "this is not a database, just a long enough line of text to look at").expect("file");
    let garbage = one(&s.db("garbage.db"), "DELETE FROM t", "Record {}", "Int", "write").expect("declares");
    let failure = refused(call(&garbage, &g, "{}"));
    assert_eq!(failure.kind, CallErrorKind::Failed, "{failure}");
    assert!(failure.message.contains("not a database"), "{failure}");
}

#[test]
fn a_result_over_the_row_cap_fails_instead_of_truncating() {
    let s = Sandbox::new("cap");
    setup(&s, "CREATE TABLE n (v INTEGER); INSERT INTO n VALUES (1),(2),(3),(4),(5),(6)");
    let db = s.db("app.db");
    let world = one(&db, "SELECT v FROM n", "Record {}", "List<Record {v: Int}>", "read").expect("declares");
    let g = grants(&world, &[text(&s.root)]);
    let failure = refused(call(&world, &g, "{}"));
    assert_eq!(failure.kind, CallErrorKind::Failed);
    assert!(failure.message.contains("more than 5 rows"), "{failure}");
    let limited = one(&db, "SELECT v FROM n LIMIT 5", "Record {}", "List<Record {v: Int}>", "read").expect("declares");
    assert_eq!(shown(call(&limited, &g, "{}")).matches(":v").count(), 5);
}

#[test]
fn execute_runs_ddl_in_one_transaction_and_creates_the_database() {
    let s = Sandbox::new("execute");
    let world = world(&declarations(vec![])).expect("declares");
    let g = grants(&world, &[text(&s.root)]);
    let db = s.db("fresh.db");
    assert_eq!(shown(run_execute(&world, &g, &db, "CREATE TABLE a (x); CREATE INDEX ax ON a(x); INSERT INTO a VALUES (1)")), "unit");
    let count: i64 = s.open("fresh.db").query_row("SELECT count(*) FROM a", [], |row| row.get(0)).expect("count");
    assert_eq!(count, 1);
    let cases = [
        ("SELECT 1", "return no rows"),
        ("INSERT INTO a VALUES (:x)", "binds no parameters"),
        ("INSERT INTO a VALUES (?)", "binds no parameters"),
    ];
    for (sql, expected) in cases {
        let failure = refused(run_execute(&world, &g, &db, sql));
        assert_eq!(failure.kind, CallErrorKind::Failed, "{sql}: {failure}");
        assert!(failure.message.contains(expected), "{sql}: {failure}");
    }
    let failure = refused(run_execute(&world, &g, &db, "CREATE TABL b (x)"));
    assert_eq!(failure.kind, CallErrorKind::Failed);
}

#[test]
fn foreign_keys_are_enforced_and_attach_and_vacuum_into_cannot_leave_the_grant() {
    let s = Sandbox::new("escape");
    let world = world(&declarations(vec![])).expect("declares");
    let g = grants(&world, &[text(&s.root)]);
    let db = s.db("app.db");
    run_execute(&world, &g, &db, "CREATE TABLE p (id INTEGER PRIMARY KEY); CREATE TABLE c (p REFERENCES p(id))").expect("schema");
    let failure = refused(run_execute(&world, &g, &db, "INSERT INTO c VALUES (1)"));
    assert_eq!(failure.kind, CallErrorKind::Conflict, "{failure}");
    let outside = s.outside_db("stolen.db");
    for (sql, expected) in [
        (format!("ATTACH DATABASE '{outside}' AS other; CREATE TABLE other.t (a)"), "too many attached databases"),
        (format!("VACUUM INTO '{outside}'"), "cannot VACUUM from within a transaction"),
        (format!("CREATE TABLE t (a); ATTACH '{outside}' AS other"), "too many attached databases"),
    ] {
        let failure = refused(run_execute(&world, &g, &db, &sql));
        assert_eq!(failure.kind, CallErrorKind::Failed, "{sql}: {failure}");
        assert!(failure.message.contains(expected), "{sql}: {failure}");
        assert!(!s.outside.join("stolen.db").exists(), "{sql} wrote outside the grant");
    }
}

#[test]
fn a_literal_call_outside_the_grant_is_denied_before_and_during_the_run() {
    let s = Sandbox::new("scope");
    let denied_database = s.outside_db("secret.db");
    let world = world(&declarations(vec![
        statement("db.inside", &s.db("app.db"), "DELETE FROM t", "Record {}", "Int", "write"),
        statement("db.outside", &denied_database, "DELETE FROM t", "Record {}", "Int", "write"),
        statement("db.outside-read", &denied_database, "SELECT 1 AS x", "Record {}", "List<Record {x: Int}>", "read"),
        statement("db.sibling", &format!("{}-evil/app.db", text(&s.root)), "DELETE FROM t", "Record {}", "Int", "write"),
        statement("db.dotdot", &format!("{}/../outside/app.db", text(&s.root)), "DELETE FROM t", "Record {}", "Int", "write"),
    ]))
    .expect("declares");
    let g = grants(&world, &[text(&s.root)]);
    assert_eq!(verdict(&world, &g, "db.inside", "{}"), Verdict::Allowed);
    for id in ["db.outside", "db.outside-read", "db.sibling", "db.dotdot"] {
        match verdict(&world, &g, id, "{}") {
            Verdict::Denied(reason) => assert!(reason.contains("outside every granted root") || reason.contains("resolves to"), "{id}: {reason}"),
            Verdict::Allowed => panic!("{id} was allowed"),
        }
        let failure = refused(call_as(&world, &g, id, "{}"));
        assert_eq!(failure.kind, CallErrorKind::PermissionDenied, "{id}: {failure}");
    }
    assert!(!s.outside.join("secret.db").exists(), "a denied call creates no file");
    let execute = |database: &str| {
        let json = dumps(&Json::object([("database", Json::string(database)), ("sql", Json::string("CREATE TABLE t (a)"))]), Layout::Spaced).expect("JSON");
        verdict(&world, &g, EXECUTE, &json)
    };
    assert_eq!(execute(&s.db("ok.db")), Verdict::Allowed);
    assert!(matches!(execute(&denied_database), Verdict::Denied(_)));
    assert!(matches!(execute("relative.db"), Verdict::Denied(reason) if reason.contains("not an absolute path")));
    let failure = refused(run_execute(&world, &g, &denied_database, "CREATE TABLE t (a)"));
    assert_eq!(failure.kind, CallErrorKind::PermissionDenied);
    assert!(!s.outside.join("secret.db").exists());
}

#[test]
fn symlinks_cannot_carry_a_database_out_of_the_grant() {
    let s = Sandbox::new("symlink");
    fs::write(s.outside.join("secret.db"), "").expect("outside file");
    Connection::open(s.outside.join("secret.db")).expect("opens").execute_batch("CREATE TABLE secret (a); INSERT INTO secret VALUES ('hidden')").expect("seeded");
    symlink(s.outside.join("secret.db"), s.root.join("link.db")).expect("file link");
    symlink(&s.outside, s.root.join("dirlink")).expect("directory link");
    setup(&s, "CREATE TABLE inside (a); INSERT INTO inside VALUES ('shown')");
    symlink(s.root.join("app.db"), s.root.join("inner.db")).expect("inner link");
    let world = world(&declarations(vec![
        statement("db.read-link", &s.db("link.db"), "SELECT a FROM secret", "Record {}", "List<Record {a: String}>", "read"),
        statement("db.write-link", &s.db("link.db"), "DELETE FROM secret", "Record {}", "Int", "write"),
        statement("db.dir-link", &s.db("dirlink/secret.db"), "SELECT a FROM secret", "Record {}", "List<Record {a: String}>", "read"),
        statement("db.dir-link-new", &s.db("dirlink/new.db"), "DELETE FROM t", "Record {}", "Int", "write"),
        statement("db.inner-read", &s.db("inner.db"), "SELECT a FROM inside", "Record {}", "List<Record {a: String}>", "read"),
        statement("db.inner-write", &s.db("inner.db"), "DELETE FROM inside", "Record {}", "Int", "write"),
    ]))
    .expect("declares");
    let g = grants(&world, &[text(&s.root)]);
    for id in ["db.read-link", "db.write-link", "db.dir-link", "db.dir-link-new", "db.inner-write"] {
        assert!(matches!(verdict(&world, &g, id, "{}"), Verdict::Denied(_)), "{id} should be denied statically");
        let failure = refused(call_as(&world, &g, id, "{}"));
        assert_eq!(failure.kind, CallErrorKind::PermissionDenied, "{id}: {failure}");
    }
    assert!(!s.outside.join("new.db").exists());
    assert_eq!(verdict(&world, &g, "db.inner-read", "{}"), Verdict::Allowed);
    assert_eq!(shown(call_as(&world, &g, "db.inner-read", "{}")), r#"[{:a "shown"}]"#);
    let secret: String = Connection::open(s.outside.join("secret.db")).expect("opens").query_row("SELECT a FROM secret", [], |r| r.get(0)).expect("row");
    assert_eq!(secret, "hidden");
}

#[test]
fn a_grant_needs_absolute_existing_directory_roots() {
    let s = Sandbox::new("grant");
    let world = one(&s.db("app.db"), "SELECT 1 AS x", "Record {}", "List<Record {x: Int}>", "read").expect("declares");
    let grant = |roots: Json| Grants::load(&world.registry, &Json::object([("db.call", Json::object([("roots", roots)]))]));
    assert!(grant(Json::Array(vec![Json::string("relative")])).is_err());
    assert!(grant(Json::Array(vec![])).is_err());
    assert!(grant(Json::Array(vec![Json::string(s.db("absent"))])).is_err());
    assert!(grant(Json::Array(vec![Json::string(text(&s.root))])).is_ok());
    assert!(Grants::load(&world.registry, &Json::object([("db.call", Json::Null)])).is_err());
}
