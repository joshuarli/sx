use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sx_kernel::json::{Json, Layout, dumps, parse};

fn scratch(name: &str, contents: &str) -> String {
    let directory: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("typescript-maps-{}", std::process::id()));
    fs::create_dir_all(&directory).expect("scratch directory");
    let path = directory.join(name);
    fs::write(&path, contents).expect("scratch file");
    path.to_string_lossy().into_owned()
}

fn run(tag: &str, source: &str) -> String {
    let program = scratch(&format!("{tag}.ts"), source);
    let output = Command::new(env!("CARGO_BIN_EXE_sx")).args(["run", &program, "--format", "json"]).output().expect("the sx binary runs");
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    assert!(output.status.success(), "{tag} should run:\n{source}\n{stdout}{}", String::from_utf8_lossy(&output.stderr));
    let found = parse(&stdout).expect("json");
    match found.get("result") {
        Some(value) => dumps(value, Layout::Spaced).expect("json").replace(' ', ""),
        None => panic!("{tag} did not complete: {stdout}"),
    }
}

fn trap_of(tag: &str, source: &str) -> String {
    let program = scratch(&format!("{tag}.ts"), source);
    let output = Command::new(env!("CARGO_BIN_EXE_sx")).args(["run", &program, "--format", "json"]).output().expect("the sx binary runs");
    let found = parse(&String::from_utf8(output.stdout).expect("utf-8")).expect("json");
    match found.get("failure").and_then(|failure| failure.get("kind")) {
        Some(Json::String(kind)) => kind.to_string(),
        _ => panic!("{tag} did not fail: {found:?}"),
    }
}

fn codes(tag: &str, source: &str) -> Vec<String> {
    let program = scratch(&format!("{tag}.ts"), source);
    let output = Command::new(env!("CARGO_BIN_EXE_sx")).args(["check", &program, "--format", "json"]).output().expect("the sx binary runs");
    let found = parse(&String::from_utf8(output.stdout).expect("utf-8")).expect("json");
    match found.get("diagnostics") {
        Some(Json::Array(items)) => items
            .iter()
            .filter_map(|item| match item.get("code") {
                Some(Json::String(code)) => Some(code.to_string()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn accepts(tag: &str, source: &str) {
    let found = codes(tag, source);
    assert!(found.is_empty(), "{tag} should check, found {found:?}:\n{source}");
}

fn rejects(tag: &str, code: &str, source: &str) {
    let found = codes(tag, source);
    assert_eq!(found, vec![code.to_string()], "{tag}:\n{source}");
}

#[test]
fn a_map_is_read_in_the_loop_that_fills_it() {
    let source = "const lines = [\"a\", \"b\", \"a\"];\nconst counts = {};\nfor (const p of lines) { counts[p] = (counts[p] ?? 0) + 1; }\nreturn Object.keys(counts).sort();";
    assert_eq!(run("count", source), "[\"a\",\"b\"]");
}

#[test]
fn an_annotated_record_is_a_map_in_the_same_loop() {
    let source = "const lines = [\"a\", \"b\", \"a\"];\nconst counts: Record<string, number> = {};\nfor (const p of lines) { counts[p] = (counts[p] ?? 0) + 1; }\nreturn [counts[\"a\"] ?? 0, counts[\"b\"] ?? 0];";
    assert_eq!(run("annotated", source), "[2,1]");
}

#[test]
fn a_map_is_read_after_the_loop_that_filled_it() {
    let source = "const lines = [\"a\", \"b\", \"a\"];\nconst counts = {};\nfor (const p of lines) { counts[p] = (counts[p] ?? 0) + 1; }\nconst k = \"a\";\nconst missing = \"z\";\nreturn [counts[k], counts[missing] ?? -1];";
    assert_eq!(run("after", source), "[2,-1]");
}

#[test]
fn a_read_without_a_default_is_absent_for_a_missing_key() {
    let source = "const m = {};\nfor (const k of [\"a\"]) { m[k] = 1; }\nconst z = \"z\";\nreturn m[z] == null;";
    assert_eq!(run("absent", source), "true");
}

#[test]
fn a_key_may_be_a_field_path_a_call_or_a_template() {
    let source = "const people = [{ id: \"x\", n: 1 }, { id: \"y\", n: 2 }];\nconst byId = {};\nfor (const p of people) { byId[p.id] = p; }\nconst byName = {};\nfor (const p of people) { byName[`${p.id}-${p.n}`] = p.n; byName[p.id.toUpperCase()] = p.n; }\nlet total = 0;\nfor (const p of people) { const q = byId[p.id]; if (q == null) { continue; } total += q.n; }\nconst t = `x-1`;\nreturn [total, byName[t] ?? 0, byName[\"Y\".toUpperCase()] ?? 0];";
    assert_eq!(run("keys", source), "[3,1,2]");
}

#[test]
fn a_map_filled_from_a_field_path_is_read_in_a_later_loop() {
    let source = "const people = [{ id: \"x\", n: 1 }, { id: \"y\", n: 2 }];\nconst staffById = {};\nfor (const m of people) { staffById[m.id] = m; }\nlet total = 0;\nfor (const id of [\"x\", \"y\", \"w\"]) {\n  const member = staffById[id];\n  if (member == null) { continue; }\n  total += member.n;\n}\nreturn total;";
    assert_eq!(run("by-id", source), "3");
}

#[test]
fn a_group_is_extended_with_concat() {
    let source = "const words = [\"ab\", \"cd\", \"ae\"];\nconst groups = {};\nfor (const w of words) { const k = w.slice(0, 1); groups[k] = (groups[k] ?? []).concat([w]); }\nconst a = \"a\";\nreturn (groups[a] ?? []).length;";
    assert_eq!(run("groups", source), "2");
}

#[test]
fn a_flag_map_tests_presence_before_it_is_filled() {
    let source = "const seen = {};\nlet out = 0;\nfor (const k of [\"a\", \"b\", \"a\"]) {\n  if (seen[k] == null) { out += 1; }\n  seen[k] = true;\n}\nreturn out;";
    assert_eq!(run("seen", source), "2");
}

#[test]
fn two_empty_arrays_filled_in_one_loop_keep_their_own_element_types() {
    let source = "const rows = [{ name: \"a\", n: 3 }];\nconst names = [];\nconst counts = [];\nfor (const row of rows) { names.push(row.name); counts.push(row.n); }\nreturn [names, counts];";
    assert_eq!(run("two-arrays", source), "[[\"a\"],[3]]");
}

#[test]
fn using_a_possibly_absent_read_as_present_is_rejected() {
    rejects("bare-arith", "type-mismatch", "const m = {};\nfor (const k of [\"a\"]) { m[k] = 1; }\nconst a = \"a\";\nreturn m[a] + 1;");
    rejects("bare-field", "not-a-record", "const m = {};\nfor (const k of [\"a\"]) { m[k] = { n: 1 }; }\nconst a = \"a\";\nreturn m[a].n;");
}

#[test]
fn a_map_value_must_be_one_type() {
    rejects("two-values", "type-mismatch", "const m = {};\nfor (const k of [\"a\"]) { m[k] = 1; m[k] = \"x\"; }\nreturn 0;");
}

#[test]
fn a_map_key_must_be_a_string() {
    accepts("string-key", "const m = {};\nfor (const k of [\"a\"]) { m[k] = 1; }\nreturn 0;");
    rejects("number-key", "type-mismatch", "const m = {};\nfor (const k of [\"a\"]) { m[k] = 1; }\nfor (const i of [1]) { m[i] = 2; }\nreturn 0;");
}

#[test]
fn a_record_is_not_read_by_a_computed_key() {
    rejects("record-key", "ts-dynamic-key", "const r = { a: 1 };\nconst k = \"a\";\nreturn r[k];");
}

#[test]
fn a_string_is_not_indexed() {
    rejects("string-index", "ts-string-index", "const s = \"abc\";\nreturn s[1];");
}

#[test]
fn an_array_element_is_not_assigned() {
    rejects("array-assign", "ts-mutation-pattern", "const xs = [1, 2, 3];\nxs[1] = 5;\nreturn xs;");
    rejects("array-compound", "ts-mutation-pattern", "const xs = [1, 2, 3];\nfor (let i = 0; i < 3; i++) { xs[i] += 1; }\nreturn xs;");
}

#[test]
fn a_key_that_object_prototype_names_is_a_trap_when_the_map_lacks_it() {
    let read = "const m = {};\nfor (const k of [\"a\"]) { m[k] = 1; }\nconst words = [\"a\", \"constructor\"];\nlet total = 0;\nfor (const w of words) { total += m[w] ?? 0; }\nreturn total;";
    assert_eq!(trap_of("inherited-read", read), "out-of-range");
    let write = "const m = {};\nfor (const k of [\"__proto__\"]) { m[k] = 1; }\nreturn Object.keys(m).length;";
    assert_eq!(trap_of("inherited-write", write), "out-of-range");
}

#[test]
fn a_key_that_object_prototype_names_reads_as_its_own_value_once_written() {
    let source = "const m = {};\nfor (const k of [\"constructor\", \"a\"]) { m[k] = (m[k] ?? 0) + 1; }\nreturn 0;";
    assert_eq!(trap_of("own-read", source), "out-of-range");
    let written = "const m = {};\nfor (const w of [\"constructor\"]) { m[w] = 5; }\nconst k = \"constructor\";\nreturn m[k] ?? 0;";
    assert_eq!(run("own-written", written), "5");
}
