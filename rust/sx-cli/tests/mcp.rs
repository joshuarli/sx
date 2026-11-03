use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use sx_kernel::json::{Json, parse};

const SX: &str = env!("CARGO_BIN_EXE_sx");
const FIXTURE: &str = env!("CARGO_BIN_EXE_mcp-fixture");

static SERIAL: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug)]
struct Ran {
    code: i32,
    stdout: String,
    stderr: String,
}

fn sx(args: &[&str]) -> Ran {
    let output = Command::new(SX).args(args).output().expect("sx starts");
    Ran {
        code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn here(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/mcp").join(name).to_string_lossy().into_owned()
}

fn scratch(label: &str) -> PathBuf {
    let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("mcp-{}-{label}-{serial}", std::process::id()));
    fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

fn write(dir: &Path, name: &str, text: &str) -> String {
    let path = dir.join(name);
    fs::write(&path, text).expect("file written");
    path.to_string_lossy().into_owned()
}

fn json(text: &str) -> Json {
    parse(text).unwrap_or_else(|e| panic!("{e}: {text}"))
}

fn imported(name: &str, tools: &str) -> String {
    let ran = sx(&["mcp", "import", "--name", name, "--tools", tools]);
    assert_eq!(ran.code, 0, "{ran:?}");
    ran.stdout
}

struct Harness {
    dir: PathBuf,
    declarations: String,
    tools: String,
}

impl Harness {
    fn new(label: &str) -> Harness {
        let dir = scratch(label);
        let tools = here("run.json");
        let declarations = write(&dir, "declarations.json", &imported("fx", &tools));
        Harness { dir, declarations, tools }
    }

    fn program(&self, source: &str) -> String {
        write(&self.dir, &format!("p{}.sx", SERIAL.fetch_add(1, Ordering::Relaxed)), source)
    }

    fn explain(&self, source: &str) -> Ran {
        let program = self.program(source);
        sx(&["explain", &program, "--declarations", &self.declarations])
    }

    fn run_with(&self, source: &str, extra: &[&str], server: &[&str]) -> Ran {
        let program = self.program(source);
        let mut args: Vec<&str> =
            vec!["mcp", "run", &program, "--declarations", &self.declarations, "--format", "json"];
        args.extend(extra);
        args.push("--");
        args.extend(server);
        sx(&args)
    }

    fn run(&self, source: &str, allow: &[&str], extra: &[&str]) -> Ran {
        let mut options: Vec<&str> = Vec::new();
        for id in allow {
            options.push("--allow");
            options.push(id);
        }
        options.extend(extra);
        self.run_with(source, &options, &[FIXTURE, &self.tools])
    }

    fn result(&self, source: &str, allow: &[&str]) -> Json {
        let ran = self.run(source, allow, &[]);
        assert_eq!(ran.code, 0, "{ran:?}");
        let outcome = json(&ran.stdout);
        assert_eq!(outcome.get("outcome").and_then(Json::as_str), Some("completed"), "{}", ran.stdout);
        outcome.get("result").cloned().expect("a result")
    }
}

fn text_of(value: &Json, key: &str) -> String {
    value.get(key).and_then(Json::as_str).unwrap_or_else(|| panic!("no string {key} in {value:?}")).to_string()
}

#[test]
fn import_matches_the_golden_declarations_and_skip_report() {
    let ran = sx(&["mcp", "import", "--name", "demo", "--tools", &here("schemas.json")]);
    assert_eq!(ran.code, 0, "{ran:?}");
    assert_eq!(ran.stdout, fs::read_to_string(here("schemas.declarations.json")).expect("golden declarations"));
    assert_eq!(ran.stderr, fs::read_to_string(here("schemas.skipped.txt")).expect("golden report"));
}

#[test]
fn golden_declarations_load_and_check_a_program() {
    let dir = scratch("golden");
    let program = write(
        &dir,
        "p.sx",
        r#"(call mcp-demo.scalars {:name "a" :count 1})"#,
    );
    let ran = sx(&["check", &program, "--declarations", &here("schemas.declarations.json"), "--allow", "mcp-demo.scalars"]);
    assert_eq!(ran.code, 0, "{ran:?}");
    assert_eq!(ran.stdout, "ok: Record open {ok: Bool, score: Option<Float>}\n");
}

#[test]
fn import_over_a_paginating_server_matches_the_saved_list() {
    let saved = sx(&["mcp", "import", "--name", "demo", "--tools", &here("schemas.json")]);
    let schemas = here("schemas.json");
    let live = sx(&["mcp", "import", "--name", "demo", "--", FIXTURE, &schemas, "--page-size", "3"]);
    assert_eq!(live.code, 0, "{live:?}");
    assert_eq!(live.stdout, saved.stdout);
    assert_eq!(live.stderr, saved.stderr);
}

#[test]
fn import_refuses_what_it_cannot_name_or_read() {
    let dir = scratch("refuse");
    let collision = write(
        &dir,
        "collision.json",
        r#"{"tools": [{"name": "readFile", "inputSchema": {"type": "object"}}, {"name": "read_file", "inputSchema": {"type": "object"}}]}"#,
    );
    let ran = sx(&["mcp", "import", "--name", "demo", "--tools", &collision]);
    assert_eq!(ran.code, 2, "{ran:?}");
    assert!(ran.stderr.contains("\"readFile\" and \"read_file\" would both be the capability mcp-demo.read-file"), "{ran:?}");
    assert_eq!(ran.stdout, "");

    let duplicate = write(
        &dir,
        "duplicate.json",
        r#"{"tools": [{"name": "a", "inputSchema": {"type": "object"}}, {"name": "a", "inputSchema": {"type": "object"}}]}"#,
    );
    assert_eq!(sx(&["mcp", "import", "--name", "demo", "--tools", &duplicate]).code, 2);

    let paged = write(&dir, "paged.json", r#"{"tools": [], "nextCursor": "more"}"#);
    let ran = sx(&["mcp", "import", "--name", "demo", "--tools", &paged]);
    assert_eq!(ran.code, 64, "{ran:?}");
    assert!(ran.stderr.contains("one page of tools/list"), "{ran:?}");

    let unnamed = write(&dir, "unnamed.json", r#"{"tools": [{"description": "x"}]}"#);
    assert_eq!(sx(&["mcp", "import", "--name", "demo", "--tools", &unnamed]).code, 2);

    let empty = write(&dir, "empty.json", r#"{"tools": []}"#);
    for bad in ["Demo", "demo_x", "1demo", "demo-", ""] {
        let ran = sx(&["mcp", "import", "--name", bad, "--tools", &empty]);
        assert_eq!(ran.code, 64, "{bad:?}: {ran:?}");
    }
    assert_eq!(sx(&["mcp", "import", "--tools", &paged]).code, 64);
    assert_eq!(sx(&["mcp", "import", "--name", "demo"]).code, 64);
    assert_eq!(sx(&["mcp", "import", "--name", "demo", "--tools", &paged, "--", "true"]).code, 64);
    assert_eq!(sx(&["mcp", "import", "--name", "demo", "--tools", &dir.join("missing.json").to_string_lossy()]).code, 64);
}

#[test]
fn import_reports_a_server_that_cannot_be_used() {
    let tools = here("run.json");
    let ran = sx(&["mcp", "import", "--name", "fx", "--", FIXTURE, &tools, "--protocol-version", "1999-01-01"]);
    assert_eq!(ran.code, 2, "{ran:?}");
    assert!(ran.stderr.contains("speaks protocol version \"1999-01-01\""), "{ran:?}");

    let ran = sx(&["mcp", "import", "--name", "fx", "--", FIXTURE, &tools, "--crash-on-start"]);
    assert_eq!(ran.code, 2, "{ran:?}");
    assert!(ran.stderr.contains("initialize failed"), "{ran:?}");

    let ran = sx(&["mcp", "import", "--name", "fx", "--", FIXTURE, &tools, "--garbage-after-initialized"]);
    assert_eq!(ran.code, 2, "{ran:?}");
    assert!(ran.stderr.contains("not JSON"), "{ran:?}");

    let ran = sx(&["mcp", "import", "--name", "fx", "--", "/nonexistent/server"]);
    assert_eq!(ran.code, 2, "{ran:?}");
    assert!(ran.stderr.contains("cannot start /nonexistent/server"), "{ran:?}");

    let ran = sx(&["mcp", "import", "--name", "fx", "--timeout-ms", "300", "--", "/bin/sleep", "30"]);
    assert_eq!(ran.code, 2, "{ran:?}");
    assert!(ran.stderr.contains("did not answer in time"), "{ran:?}");
}

#[test]
fn explain_shows_each_imported_tool_with_its_effect() {
    let harness = Harness::new("explain");
    let ran = harness.explain(
        r#"(let [a (call mcp-fx.add {:a 1 :b 2})
                 s (call mcp-fx.store-put {:key "k"})
                 d (call mcp-fx.sleep-write {:ms 1})]
             [(get a :sum) s d])"#,
    );
    assert_eq!(ran.code, 0, "{ran:?}");
    let text = ran.stdout;
    assert!(text.contains("mcp-fx.add  [read]  not allowed\n    input: {\"a\": 1, \"b\": 2}"), "{text}");
    assert!(text.contains("mcp-fx.store-put  [write, idempotent]"), "{text}");
    assert!(text.contains("mcp-fx.sleep-write  [write, not idempotent]"), "{text}");
}

#[test]
fn a_program_runs_against_the_server_and_types_its_results() {
    let harness = Harness::new("happy");
    let result = harness.result(
        r#"(let [s (call mcp-fx.add {:a 2 :b 3})
                 g (call mcp-fx.greet {:name "Ada"})
                 h (call mcp-fx.greet {:name "Ada" :greeting "Hi"})
                 m (call mcp-fx.multi-text {})
                 l (call mcp-fx.lookup {:id "k"})
                 p (call mcp-fx.profile {})]
             {:sum (get s :sum) :g g :h h :m m :title (get l :title) :nick (get p :nick) :bio (get p :bio) :name (get p :name)})"#,
        &["mcp-fx.add", "mcp-fx.greet", "mcp-fx.multi-text", "mcp-fx.lookup", "mcp-fx.profile"],
    );
    let expected = json(
        r#"{"sum": 5, "g": "Hello, Ada!", "h": "Hi, Ada!", "m": "ab", "title": "Item k", "nick": null, "bio": null, "name": "ada"}"#,
    );
    assert!(sx_kernel::json::python_equal(&result, &expected), "{result:?}");
}

#[test]
fn enums_tuples_lists_maps_and_nested_options_round_trip() {
    let harness = Harness::new("shapes");
    let result = harness.result(
        r#"(let [r (call mcp-fx.shape-echo {:mode "slow-lane" :point [1.5 2.0] :tags ["a" "b"] :labels (assoc {} "k" "v") :home {:street "main"}})]
             {:mode (= (get r :mode) "slow-lane") :point (get r :point) :tags (get r :tags) :labels (get r :labels) :zip (match (get r :home) (some h) (get h :zip) none none)})"#,
        &["mcp-fx.shape-echo"],
    );
    let expected = json(r#"{"mode": true, "point": [1.5, 2.0], "tags": ["a", "b"], "labels": {"k": "v"}, "zip": null}"#);
    assert!(sx_kernel::json::python_equal(&result, &expected), "{result:?}");
}

#[test]
fn none_is_omitted_from_the_arguments_and_a_present_value_is_sent() {
    let harness = Harness::new("arguments");
    let result = harness.result(
        r#"(let [a (call mcp-fx.echo-args {:a "x"})
                 b (call mcp-fx.echo-args {:a "x" :b 2 :c "y"})]
             [(get a :seen) (get b :seen)])"#,
        &["mcp-fx.echo-args"],
    );
    let expected = json(r#"["{\"a\": \"x\"}", "{\"a\": \"x\", \"b\": 2, \"c\": \"y\"}"]"#);
    assert!(sx_kernel::json::python_equal(&result, &expected), "{result:?}");
}

#[test]
fn nothing_is_granted_by_importing() {
    let harness = Harness::new("ungranted");
    let ran = harness.run(r#"(call mcp-fx.add {:a 1 :b 2})"#, &[], &[]);
    assert_eq!(ran.code, 1, "{ran:?}");
    assert!(ran.stdout.contains("unauthorized"), "{ran:?}");
}

fn failure_of(harness: &Harness, call: &str, allow: &str, extra: &[&str]) -> (String, String) {
    let ran = harness.run(&format!("(attempt {call})"), &[allow], extra);
    assert_eq!(ran.code, 0, "{call}: {ran:?}");
    let outcome = json(&ran.stdout);
    let result = outcome.get("result").unwrap_or_else(|| panic!("{call}: {}", ran.stdout));
    let error = result.get("err").unwrap_or_else(|| panic!("{call}: expected an error, got {}", ran.stdout));
    (text_of(error, "kind"), text_of(error, "message"))
}

#[test]
fn every_failure_maps_to_one_call_error_kind() {
    let harness = Harness::new("errors");
    let slow = ["--timeout-ms", "300"];
    let rows: [(&str, &str, &[&str], &str, &str); 20] = [
        ("(call mcp-fx.bad-read {})", "mcp-fx.bad-read", &[], "Invalid", "does not match the declared output"),
        ("(call mcp-fx.bad-write {})", "mcp-fx.bad-write", &[], "Failed", "does not match the declared output"),
        ("(call mcp-fx.fail-read {})", "mcp-fx.fail-read", &[], "Failed", "disk on fire"),
        ("(call mcp-fx.fail-write {})", "mcp-fx.fail-write", &[], "Failed", "disk on fire"),
        ("(call mcp-fx.rpc-invalid-read {})", "mcp-fx.rpc-invalid-read", &[], "Invalid", "-32602"),
        ("(call mcp-fx.rpc-invalid-write {})", "mcp-fx.rpc-invalid-write", &[], "Failed", "-32602"),
        ("(call mcp-fx.rpc-other {})", "mcp-fx.rpc-other", &[], "Failed", "-32000"),
        ("(call mcp-fx.sleep-read {:ms 5000})", "mcp-fx.sleep-read", &slow, "Timeout", "300 ms"),
        ("(call mcp-fx.sleep-write {:ms 5000})", "mcp-fx.sleep-write", &slow, "Timeout", "300 ms"),
        ("(call mcp-fx.die-read {})", "mcp-fx.die-read", &[], "Unavailable", "closed"),
        ("(call mcp-fx.die-write {})", "mcp-fx.die-write", &[], "Failed", "closed"),
        ("(call mcp-fx.image-read {})", "mcp-fx.image-read", &[], "Invalid", "image content block"),
        ("(call mcp-fx.image-write {})", "mcp-fx.image-write", &[], "Failed", "image content block"),
        ("(call mcp-fx.unstructured-read {})", "mcp-fx.unstructured-read", &[], "Invalid", "no structuredContent"),
        ("(call mcp-fx.unstructured-write {})", "mcp-fx.unstructured-write", &[], "Failed", "no structuredContent"),
        ("(call mcp-fx.missing-on-server {})", "mcp-fx.missing-on-server", &[], "Invalid", "unknown tool"),
        ("(call mcp-fx.store-put {:key \"k\"})", "mcp-fx.store-put", &[], "Failed", "unknown tool"),
        ("(call mcp-fx.rpc-other {})", "mcp-fx.rpc-other", &[], "Failed", "server trouble"),
        ("(call mcp-fx.fail-read {})", "mcp-fx.fail-read", &[], "Failed", "mcp-fx.fail-read"),
        ("(call mcp-fx.bad-read {})", "mcp-fx.bad-read", &[], "Invalid", "$.sum"),
    ];
    for (call, allow, extra, kind, fragment) in rows {
        let (got, message) = failure_of(&harness, call, allow, extra);
        assert_eq!(got, kind, "{call}: {message}");
        assert!(message.contains(fragment), "{call}: {message}");
    }
}

#[test]
fn a_timeout_cancels_the_request_and_later_calls_still_work() {
    let harness = Harness::new("cancel");
    let ran = harness.run(
        r#"(let [slow (attempt (call mcp-fx.sleep-read {:ms 5000}))
                 seen (call mcp-fx.cancellations {})]
             {:slow slow :cancelled (get seen :count)})"#,
        &["mcp-fx.sleep-read", "mcp-fx.cancellations"],
        &["--timeout-ms", "300"],
    );
    assert_eq!(ran.code, 0, "{ran:?}");
    let result = json(&ran.stdout).get("result").cloned().expect("a result");
    assert_eq!(result.get("cancelled").and_then(Json::as_i64), Some(1), "{result:?}");
    assert_eq!(text_of(result.get("slow").and_then(|s| s.get("err")).expect("an err"), "kind"), "Timeout");
}

#[test]
fn concurrent_calls_share_one_connection_and_answer_out_of_order() {
    let harness = Harness::new("concurrent");
    let started = Instant::now();
    let result = harness.result(
        r#"(par-map {:limit 8}
             (fn [[tag ms]] (let [r (call mcp-fx.echo-delay {:tag tag :ms ms})] [(get r :tag) (get r :order)]))
             [["slow" 600] ["mid" 400] ["fast" 200] ["quick" 50]])"#,
        &["mcp-fx.echo-delay"],
    );
    let expected = json(r#"[["slow", 4], ["mid", 3], ["fast", 2], ["quick", 1]]"#);
    assert!(sx_kernel::json::python_equal(&result, &expected), "{result:?}");
    assert!(started.elapsed() < Duration::from_millis(1900), "the calls ran one after another: {:?}", started.elapsed());
}

#[test]
fn the_concurrency_budget_bounds_the_calls_in_flight() {
    let harness = Harness::new("budget");
    let calls = (0..20).map(|_| "300").collect::<Vec<_>>().join(" ");
    let result = harness.result(
        &format!(r#"(par-map {{:limit 16}} (fn [ms] (get (call mcp-fx.peak {{:ms ms}}) :peak)) [{calls}])"#),
        &["mcp-fx.peak"],
    );
    let peaks: Vec<i64> = result.as_array().expect("a list").iter().filter_map(Json::as_i64).collect();
    assert_eq!(peaks.len(), 20);
    let highest = peaks.iter().max().copied().unwrap_or(0);
    assert!((12..=16).contains(&highest), "peak in flight was {highest}");
}

fn alive(pid: &str) -> bool {
    Command::new("kill").args(["-0", pid]).output().map(|o| o.status.success()).unwrap_or(false)
}

fn gone(pid: &str) -> bool {
    let until = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until {
        if !alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn the_server_and_its_children_are_killed_on_every_way_out() {
    let harness = Harness::new("kill");
    let cases: [(&str, &str, &[&str]); 3] = [
        (r#"(call mcp-fx.add {:a 1 :b 2})"#, "mcp-fx.add", &[]),
        (r#"(attempt (call mcp-fx.sleep-read {:ms 60000}))"#, "mcp-fx.sleep-read", &["--timeout-ms", "200"]),
        (r#"(call mcp-fx.fail-read {})"#, "mcp-fx.fail-read", &[]),
    ];
    for (source, allow, extra) in cases {
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let pid_file = harness.dir.join(format!("pid-{serial}"));
        let child_file = harness.dir.join(format!("child-{serial}"));
        let (pid_path, child_path) = (pid_file.to_string_lossy().into_owned(), child_file.to_string_lossy().into_owned());
        let mut options = vec!["--allow", allow];
        options.extend(extra);
        let server = [FIXTURE, &harness.tools, "--linger", "--pid-file", &pid_path, "--child-pid-file", &child_path];
        let ran = harness.run_with(source, &options, &server);
        assert!(ran.code == 0 || ran.code == 2, "{source}: {ran:?}");
        for file in [&pid_file, &child_file] {
            let pid = fs::read_to_string(file).expect("the server wrote a pid");
            assert!(gone(&pid), "{source}: process {pid} outlived the run");
        }
    }

    let pid_file = harness.dir.join("pid-import");
    let child_file = harness.dir.join("child-import");
    let (pid_path, child_path) = (pid_file.to_string_lossy().into_owned(), child_file.to_string_lossy().into_owned());
    let ran = sx(&[
        "mcp", "import", "--name", "fx", "--", FIXTURE, &harness.tools, "--linger", "--pid-file", &pid_path,
        "--child-pid-file", &child_path,
    ]);
    assert_eq!(ran.code, 0, "{ran:?}");
    for file in [&pid_file, &child_file] {
        let pid = fs::read_to_string(file).expect("the server wrote a pid");
        assert!(gone(&pid), "process {pid} outlived the import");
    }
}

#[test]
fn a_rejected_program_never_starts_the_server() {
    let harness = Harness::new("rejected");
    let pid_file = harness.dir.join("pid-rejected");
    let pid_path = pid_file.to_string_lossy().into_owned();
    let ran = harness.run_with(
        r#"(call mcp-fx.add {:a 1 :b 2})"#,
        &[],
        &[FIXTURE, &harness.tools, "--pid-file", &pid_path],
    );
    assert_eq!(ran.code, 1, "{ran:?}");
    assert!(!pid_file.exists());

    let ran = harness.run_with(
        r#"(+ 1 2)"#,
        &[],
        &[FIXTURE, &harness.tools, "--pid-file", &pid_path],
    );
    assert_eq!(ran.code, 0, "{ran:?}");
    assert!(!pid_file.exists(), "a program with no MCP call started the server");
}

#[test]
fn a_server_that_cannot_start_stops_the_run_before_any_call() {
    let harness = Harness::new("nostart");
    let ran = harness.run_with(r#"(call mcp-fx.add {:a 1 :b 2})"#, &["--allow", "mcp-fx.add"], &["/nonexistent/server"]);
    assert_eq!(ran.code, 64, "{ran:?}");
    assert!(ran.stderr.contains("cannot connect to the MCP server: cannot start /nonexistent/server"), "{ran:?}");
}

#[test]
fn several_servers_in_the_declarations_need_a_name() {
    let dir = scratch("several");
    let tools = here("run.json");
    let both = {
        let first = json(&imported("one", &tools));
        let second = json(&imported("two", &tools));
        let mut capabilities: Vec<Json> = first.get("capabilities").and_then(Json::as_array).expect("list").to_vec();
        capabilities.extend(second.get("capabilities").and_then(Json::as_array).expect("list").iter().cloned());
        let mut enums: Vec<(String, Json)> = first.get("enums").and_then(Json::as_object).expect("enums").to_vec();
        enums.extend(second.get("enums").and_then(Json::as_object).expect("enums").iter().cloned());
        sx_kernel::json::dumps(
            &Json::object([("enums", Json::Object(enums)), ("capabilities", Json::Array(capabilities))]),
            sx_kernel::json::Layout::Spaced,
        )
        .expect("declarations")
    };
    let declarations = write(&dir, "both.json", &both);
    let program = write(&dir, "p.sx", r#"(call mcp-one.add {:a 1 :b 2})"#);
    let run = |extra: &[&str]| {
        let mut args = vec!["mcp", "run", &program, "--declarations", &declarations, "--allow", "mcp-one.add"];
        args.extend(extra);
        args.extend(["--", FIXTURE, &tools]);
        sx(&args)
    };
    let ran = run(&[]);
    assert_eq!(ran.code, 64, "{ran:?}");
    assert!(ran.stderr.contains("several MCP servers (one, two); choose one with --name"), "{ran:?}");
    assert_eq!(run(&["--name", "one"]).code, 0);
    let ran = run(&["--name", "two"]);
    assert_eq!(ran.code, 64, "{ran:?}");
    assert!(ran.stderr.contains("no pack implements mcp-one.add"), "{ran:?}");
    let ran = run(&["--name", "three"]);
    assert_eq!(ran.code, 64, "{ran:?}");
}

#[test]
fn the_command_line_is_checked() {
    assert_eq!(sx(&["mcp"]).code, 64);
    assert_eq!(sx(&["mcp", "dance"]).code, 64);
    assert_eq!(sx(&["mcp", "--help"]).code, 0);
    let harness = Harness::new("usage");
    let program = harness.program("1");
    assert_eq!(sx(&["mcp", "run", &program, "--declarations", &harness.declarations]).code, 64);
    assert_eq!(sx(&["mcp", "run", &program, "--", FIXTURE]).code, 64);
    assert_eq!(
        sx(&["mcp", "run", &program, "--declarations", &harness.declarations, "--timeout-ms", "0", "--", FIXTURE]).code,
        64
    );
}

#[test]
fn a_typescript_program_runs_against_the_server_and_an_unknown_language_is_a_usage_error() {
    let harness = Harness::new("typescript");
    let program = write(&harness.dir, "program.ts", "const s = await tools.mcpFx.add({ a: 2, b: 3 });\nreturn s.sum;");
    let ran = sx(&[
        "mcp", "run", &program, "--declarations", &harness.declarations, "--allow", "mcp-fx.add", "--language", "ts",
        "--format", "json", "--", FIXTURE, &harness.tools,
    ]);
    assert_eq!(ran.code, 0, "{ran:?}");
    assert_eq!(json(&ran.stdout).get("result").and_then(Json::as_i64), Some(5), "{}", ran.stdout);
    let refused = sx(&["mcp", "run", &program, "--declarations", &harness.declarations, "--language", "python"]);
    assert_eq!(refused.code, 64);
}
