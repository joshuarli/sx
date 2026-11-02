use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use sx_kernel::help::EXTENSIONS;
use sx_kernel::json::{Json, parse};

static SERIAL: AtomicUsize = AtomicUsize::new(0);

struct Ran {
    code: i32,
    stdout: String,
    stderr: String,
}

fn scratch() -> PathBuf {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("script-run-{}", std::process::id()));
    fs::create_dir_all(&directory).expect("scratch directory");
    directory
}

fn file(name: &str, contents: &str) -> String {
    let path = scratch().join(format!("{}-{name}", SERIAL.fetch_add(1, Ordering::Relaxed)));
    fs::write(&path, contents).expect("scratch file");
    path.to_string_lossy().into_owned()
}

fn sx(args: &[&str]) -> Ran {
    let output = Command::new(env!("CARGO_BIN_EXE_sx")).args(args).output().expect("the sx binary runs");
    Ran {
        code: output.status.code().expect("exit status"),
        stdout: String::from_utf8(output.stdout).expect("utf-8 stdout"),
        stderr: String::from_utf8(output.stderr).expect("utf-8 stderr"),
    }
}

const REPORT: &str = r#"(script {:inputs {:owners "List<String>" :limit "Int" :note "Option<String>"}}
  {:count (count owners) :first (take limit owners) :note (match note (some n) n none "none")})"#;

fn report() -> String {
    file("report.sx", REPORT)
}

#[test]
fn a_script_runs_with_its_inputs_and_no_declarations() {
    let program = report();
    let ran = sx(&["run", &program, "--input", r#"owners=["acme","beta","cyan"]"#, "--input", "limit=2"]);
    assert_eq!((ran.code, ran.stderr.as_str()), (0, ""), "{}", ran.stdout);
    assert_eq!(ran.stdout, "{:count 3 :first [\"acme\" \"beta\"] :note \"none\"}\n");
    let json = sx(&["run", &program, "--format", "json", "--input", r#"owners=["a"]"#, "--input", "limit=1", "--input", r#"note="hi""#]);
    let json = parse(&json.stdout).expect("json output");
    assert_eq!(json.get("outcome").and_then(Json::as_str), Some("completed"));
    let result = json.get("result").expect("result");
    assert_eq!(result.get("note").and_then(Json::as_str), Some("hi"));
}

#[test]
fn an_option_input_may_be_omitted_or_null_and_a_repeated_input_takes_the_last_value() {
    let program = report();
    let omitted = sx(&["run", &program, "--input", r#"owners=[]"#, "--input", "limit=0"]);
    assert_eq!(omitted.stdout, "{:count 0 :first [] :note \"none\"}\n");
    let null = sx(&["run", &program, "--input", r#"owners=[]"#, "--input", "limit=0", "--input", "note=null"]);
    assert_eq!(null.stdout, omitted.stdout);
    let repeated = sx(&["run", &program, "--input", r#"owners=["x"]"#, "--input", "limit=0", "--input", "limit=1"]);
    assert_eq!(repeated.stdout, "{:count 1 :first [\"x\"] :note \"none\"}\n");
}

#[test]
fn a_missing_required_input_is_a_usage_error_that_lists_the_required_inputs_and_their_types() {
    let program = report();
    let ran = sx(&["run", &program, "--input", "limit=1"]);
    assert_eq!(ran.code, 64);
    assert_eq!(ran.stdout, "");
    assert_eq!(
        ran.stderr,
        "sx: the script needs a value for input owners; its required inputs are limit: Int, owners: List<String>; pass each as --input NAME=JSON\n"
    );
    let none = sx(&["run", &program]);
    assert!(none.stderr.contains("input limit, owners;"), "{}", none.stderr);
}

#[test]
fn an_unknown_input_and_a_mistyped_input_are_usage_errors() {
    let program = report();
    let unknown = sx(&["run", &program, "--input", "limit=1", "--input", "owners=[]", "--input", "extra=1"]);
    assert_eq!(unknown.code, 64);
    assert_eq!(unknown.stderr, "sx: --input extra is not an input of the script, whose inputs are limit, note, owners\n");
    let mistyped = sx(&["run", &program, "--input", r#"owners=["a",2]"#, "--input", "limit=1"]);
    assert_eq!(mistyped.code, 64);
    assert_eq!(mistyped.stderr, "sx: --input owners: $owners[1]: expected String, found an integer\n");
    let malformed = sx(&["run", &program, "--input", "limit"]);
    assert_eq!(malformed.stderr, "sx: --input takes NAME=JSON, not 'limit'\n");
    let not_json = sx(&["run", &program, "--input", "limit=one"]);
    assert_eq!(not_json.code, 64);
    assert!(not_json.stderr.starts_with("sx: --input limit: "), "{}", not_json.stderr);
}

#[test]
fn a_script_with_no_inputs_rejects_every_input() {
    let program = file("none.sx", "(script {:inputs {}} (+ 1 2))");
    assert_eq!(sx(&["run", &program]).stdout, "3\n");
    let ran = sx(&["run", &program, "--input", "a=1"]);
    assert_eq!(ran.stderr, "sx: --input a is not an input, because the script declares none\n");
}

#[test]
fn explain_shows_every_declared_input_with_its_type() {
    let program = file("explain.sx", r#"(script {:inputs {:owners "List<String>" :limit "Int"}} (count owners))"#);
    let text = sx(&["explain", &program]);
    assert_eq!(text.code, 0);
    assert_eq!(text.stdout, "tools:\ninputs: limit: Int, owners: List<String>\nresult: Int\n");
    let json = parse(&sx(&["explain", &program, "--format", "json"]).stdout).expect("json");
    assert_eq!(json.get("inputs").and_then(Json::as_array).map(<[Json]>::len), Some(2));
    let types = json.get("input_types").expect("input_types");
    assert_eq!(types.get("limit").and_then(Json::as_str), Some("Int"));
    assert_eq!(types.get("owners").and_then(Json::as_str), Some("List<String>"));
}

#[test]
fn a_bad_header_is_rejected_with_its_diagnostic_and_nothing_runs() {
    let program = file("bad.sx", r#"(script {:inputs {:owners "List<Strng>"}} (count owners))"#);
    let ran = sx(&["run", &program, "--input", "owners=[]", "--format", "json"]);
    assert_eq!(ran.code, 1);
    let json = parse(&ran.stdout).expect("json");
    let diagnostics = json.get("diagnostics").and_then(Json::as_array).expect("diagnostics");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].get("code").and_then(Json::as_str), Some("input-type"));
    assert!(diagnostics[0].get("detail").and_then(Json::as_str).is_some_and(|d| d.contains("unknown type 'Strng'")));
}

const STORE: &str = r#"{"inputs": {"id": "String"}, "capabilities": [{"id": "store.get", "effect": "read", "input": "Record {id: String}", "output": "Record {id: String, size: Int}", "description": "Reads one item."}]}"#;
const STORE_PROGRAM: &str = r#"(script {:inputs {:id "String"}} (get (call store.get {:id id}) :size))"#;
const STORE_RESPONSES: &str = r#"[{"capability": "store.get", "input": {"id": "a"}, "results": [{"output": {"id": "a", "size": 7}}]}]"#;

#[test]
fn a_host_that_declares_the_same_inputs_runs_the_script() {
    let (program, declarations, responses) = (file("store.sx", STORE_PROGRAM), file("store.json", STORE), file("responses.json", STORE_RESPONSES));
    let ran = sx(&["run", &program, "--declarations", &declarations, "--responses", &responses, "--allow", "store.get", "--input", r#"id="a""#]);
    assert_eq!((ran.code, ran.stdout.as_str(), ran.stderr.as_str()), (0, "7\n", ""));
}

#[test]
fn a_host_declaration_that_differs_from_the_header_is_a_usage_error_naming_the_difference() {
    let program = file("echo.sx", r#"(script {:inputs {:id "String"}} id)"#);
    let disagreements = [
        (
            r#"{"inputs": {"id": "Int"}}"#,
            "input id is String in the script's header and Int in the declarations",
        ),
        (
            r#"{"inputs": {"id": "String", "extra": "Bool"}}"#,
            "the declarations declare input extra (Bool), which the script's header does not",
        ),
        (
            r#"{"inputs": {"other": "String"}}"#,
            "the script's header declares input id (String), which the declarations do not",
        ),
    ];
    for (declarations, difference) in disagreements {
        let declarations = file("declarations.json", declarations);
        for command in ["check", "explain", "run"] {
            let ran = sx(&[command, &program, "--declarations", &declarations]);
            assert_eq!(ran.code, 64, "{command}: {}", ran.stdout);
            assert!(ran.stderr.contains(difference), "{command}: {}", ran.stderr);
            assert!(ran.stderr.starts_with("sx: the script's inputs and the declarations' inputs disagree: "), "{}", ran.stderr);
        }
    }
    let none = file("none.json", r#"{"capabilities": []}"#);
    assert_eq!(sx(&["check", &program, "--declarations", &none]).stdout, "ok: String\n");
}

#[test]
fn a_script_takes_decimal_inputs_as_json_strings() {
    let declarations = file("decimal.json", r#"{"cores": ["decimal"]}"#);
    let program = file(
        "total.sx",
        r#"(script {:inputs {:price "decimal.Decimal" :qty "Int"}} (decimal.format (decimal.mul price (decimal.from-int qty)) 2))"#,
    );
    let ran = sx(&["run", &program, "--declarations", &declarations, "--input", r#"price="12.50""#, "--input", "qty=3"]);
    assert_eq!((ran.code, ran.stdout.as_str()), (0, "\"37.50\"\n"), "{}", ran.stderr);
    let number = sx(&["run", &program, "--declarations", &declarations, "--input", "price=12.5", "--input", "qty=3"]);
    assert_eq!(number.code, 64);
    assert_eq!(number.stderr, "sx: --input price: $price: expected decimal.Decimal (a string), found a number with a fraction\n");
    let overflow = sx(&["run", &program, "--declarations", &declarations, "--input", r#"price="99999999999999999999999999999999999999""#, "--input", "qty=2", "--format", "json"]);
    assert_eq!(overflow.code, 2);
    let json = parse(&overflow.stdout).expect("json");
    assert_eq!(json.get("failure").and_then(|f| f.get("kind")).and_then(Json::as_str), Some("arithmetic"));
}

#[test]
fn the_help_entry_for_script_has_an_example_that_runs_as_documented() {
    let entry = EXTENSIONS.iter().find(|entry| entry.name == "script").expect("a script entry");
    let help = sx(&["help", "script"]);
    assert_eq!(help.code, 0);
    assert!(help.stdout.contains(entry.example) && help.stdout.contains(entry.result), "{}", help.stdout);
    let program = file("example.sx", entry.example);
    let ran = sx(&["run", &program, "--input", r#"owners=["acme","beta","cyan"]"#, "--input", "limit=2"]);
    assert_eq!(ran.stdout.trim_end(), entry.result);
}
