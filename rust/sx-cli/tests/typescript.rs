use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static SERIAL: AtomicUsize = AtomicUsize::new(0);

struct Ran {
    code: i32,
    stdout: String,
    stderr: String,
}

fn file(name: &str, contents: &str) -> String {
    let directory: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("typescript-{}", std::process::id()));
    fs::create_dir_all(&directory).expect("scratch directory");
    let path = directory.join(format!("{}-{name}", SERIAL.fetch_add(1, Ordering::Relaxed)));
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

const DECLARATIONS: &str = r#"{"capabilities": [
  {"id": "fs.list", "effect": "read", "input": "Record {path: String}", "output": "List<Record {name: String, size: Int}>", "description": "Lists a directory."}
]}"#;

const RESPONSES: &str = r#"[{"capability": "fs.list", "input": {"path": "."}, "results": [{"output": [{"name": "a", "size": 5}, {"name": "stop", "size": 9}]}]}]"#;

const PROGRAM: &str = "const files = await tools.fs.list({ path: \".\" });\nlet found = \"\";\nfor (const f of files) {\n  if (f.name === \"stop\") break;\n  found = f.name;\n}\nreturn found;\n";

#[test]
fn a_ts_file_runs_against_recorded_responses_without_a_language_flag() {
    let (program, declarations, responses) = (file("find.ts", PROGRAM), file("decl.json", DECLARATIONS), file("responses.json", RESPONSES));
    let ran = sx(&["run", &program, "--declarations", &declarations, "--responses", &responses, "--allow", "fs.list"]);
    assert_eq!((ran.code, ran.stderr.as_str(), ran.stdout.as_str()), (0, "", "\"a\"\n"));
}

#[test]
fn the_language_flag_selects_typescript_for_any_file_name() {
    let (program, declarations, responses) = (file("find.txt", PROGRAM), file("decl.json", DECLARATIONS), file("responses.json", RESPONSES));
    let ran = sx(&["run", &program, "--language", "ts", "--declarations", &declarations, "--responses", &responses, "--allow", "fs.list"]);
    assert_eq!((ran.code, ran.stdout.as_str()), (0, "\"a\"\n"));
}

#[test]
fn emit_sx_prints_the_lowered_program() {
    let (program, declarations) = (file("find.ts", "const files = await tools.fs.list({ path: \".\" });\nreturn files.map((f) => f.name);\n"), file("decl.json", DECLARATIONS));
    let ran = sx(&["check", &program, "--emit", "sx", "--declarations", &declarations]);
    assert_eq!(
        (ran.code, ran.stdout.as_str()),
        (0, "(let [files (call fs.list {:path \".\"})] (map (fn [f] (get f :name)) files))\n")
    );
}

#[test]
fn emit_sx_needs_a_typescript_program() {
    let program = file("one.sx", "1");
    let ran = sx(&["check", &program, "--emit", "sx"]);
    assert_eq!(ran.code, 64);
    assert!(ran.stderr.contains("--emit sx prints the lowering of a TypeScript program"), "{}", ran.stderr);
}

#[test]
fn a_rejection_points_at_the_typescript_source() {
    let program = file("bad.ts", "const n = 1;\nwhile (n < 3) {\n  break;\n}\n");
    let ran = sx(&["check", &program]);
    assert_eq!(ran.code, 1);
    assert!(ran.stdout.contains("error[ts-while] 2:1-2:6"), "{}", ran.stdout);
    assert!(ran.stdout.contains("  while (n < 3) {\n  ^^^^^"), "{}", ran.stdout);
    assert!(ran.stdout.contains("Loops must be bounded"), "{}", ran.stdout);
}

#[test]
fn a_checker_diagnostic_on_the_lowered_tree_keeps_the_typescript_span() {
    let (program, declarations) = (file("typo.ts", "const files = await tools.fs.list({ path: \".\" });\nreturn files.map((f) => f.nme);\n"), file("decl.json", DECLARATIONS));
    let ran = sx(&["check", &program, "--declarations", &declarations, "--allow", "fs.list"]);
    assert_eq!(ran.code, 1);
    assert!(ran.stdout.contains("2:"), "{}", ran.stdout);
    assert!(ran.stdout.contains("return files.map((f) => f.nme);"), "{}", ran.stdout);
}

#[test]
fn explain_lists_the_tools_a_typescript_program_calls() {
    let (program, declarations) = (file("find.ts", PROGRAM), file("decl.json", DECLARATIONS));
    let ran = sx(&["explain", &program, "--declarations", &declarations]);
    assert_eq!(ran.code, 0, "{}", ran.stderr);
    assert!(ran.stdout.contains("fs.list  [read]"), "{}", ran.stdout);
    assert!(ran.stdout.contains("result: String"), "{}", ran.stdout);
}

const PAGED: &str = r#"{"capabilities": [
  {"id": "hr.staff", "effect": "read", "input": "Record {cursor: Option<String>}", "output": "Record {items: List<String>, next: Option<String>}", "description": "One page."}
]}"#;

const PAGED_RESPONSES: &str = r#"[{"capability": "hr.staff", "input": {"cursor": null}, "results": [{"output": {"items": ["a"], "next": null}}]}]"#;

#[test]
fn an_empty_object_variable_is_a_tool_input_whose_fields_are_all_optional() {
    let program = file("paged.ts", "const input = {};\nconst page = await tools.hr.staff(input);\nreturn page.items;\n");
    let (declarations, responses) = (file("paged-decl.json", PAGED), file("paged-responses.json", PAGED_RESPONSES));
    let ran = sx(&["run", &program, "--declarations", &declarations, "--responses", &responses, "--allow", "hr.staff"]);
    assert_eq!((ran.code, ran.stderr.as_str(), ran.stdout.as_str()), (0, "", "[\"a\"]\n"));
}

#[test]
fn objects_with_different_keys_do_not_join_and_the_message_says_what_to_write() {
    let program = file("join.ts", "const cursor: string | null = null;\nconst input = cursor == null ? {} : { cursor };\nreturn input;\n");
    let declarations = file("paged-decl.json", PAGED);
    let ran = sx(&["check", &program, "--declarations", &declarations]);
    assert_eq!(ran.code, 1);
    assert!(ran.stdout.contains("branch-mismatch") && ran.stdout.contains("{ cursor: null }"), "{}{}", ran.stdout, ran.stderr);
}
