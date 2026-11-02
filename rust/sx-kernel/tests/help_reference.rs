use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sx_kernel::help::{
    GuideLevel, entries, entry_names, example_declarations, example_responses, guide, lookup_entry,
    not_found_message,
};
use sx_kernel::json::{Json, parse, python_equal};

const REFERENCE: &str = r#"
import contextlib
import io
import json
import sys

from sx import help as sx_help
from sx.cli import main

cases = [{"name": None, "level": level} for level in sx_help.GUIDE_LEVELS]
cases += [{"name": name, "level": "standard"} for name in sx_help.ENTRIES]
cases += [
    {"name": name, "level": "standard"}
    for name in ["lett", "zzzzzzzz", "Optoin", "", "ma", "recure", "Map<", "+ ", "nothing", "if", "let ", "sleeep", "nth2"]
]
results = []
for case in cases:
    argv = ["help", "--level", case["level"]] + ([] if case["name"] is None else [case["name"]])
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        code = main(argv)
    results.append({**case, "stdout": out.getvalue(), "stderr": err.getvalue(), "code": code})
entries = [
    {"name": e.name, "signature": e.signature, "description": e.description, "example": e.example, "result": e.result}
    for e in sx_help.ENTRIES.values()
]
json.dump(
    {
        "results": results,
        "entries": entries,
        "declarations": sx_help.EXAMPLE_DECLARATIONS,
        "responses": list(sx_help.EXAMPLE_RESPONSES),
    },
    sys.stdout,
)
"#;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn python_reference() -> Json {
    let directory = root().join("rust/target/help-reference");
    fs::create_dir_all(&directory).expect("create the reference directory");
    let script = directory.join("reference.py");
    fs::write(&script, REFERENCE).expect("write the reference script");
    let output = Command::new("uv")
        .args(["run", "python"])
        .arg(&script)
        .current_dir(root())
        .output()
        .expect("uv is on PATH");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    parse(std::str::from_utf8(&output.stdout).expect("utf-8 output")).expect("json output")
}

fn text<'a>(value: &'a Json, key: &str) -> &'a str {
    value.get(key).and_then(Json::as_str).expect("text field")
}

#[test]
fn every_help_output_is_byte_identical_to_python() {
    let reference = python_reference();
    let results = reference.get("results").and_then(Json::as_array).expect("results");
    assert!(results.len() > 80);
    let candidates = entry_names();
    let mut failures = Vec::new();
    for case in results {
        let level = GuideLevel::from_name(text(case, "level")).expect("known level");
        let (stdout, stderr, code) = match case.get("name").and_then(Json::as_str) {
            None => (guide(level), String::new(), 0),
            Some(name) => match lookup_entry(name) {
                Some(found) => (found, String::new(), 0),
                None => (String::new(), format!("{}\n", not_found_message(name, &candidates)), 1),
            },
        };
        let expected = (text(case, "stdout"), text(case, "stderr"), case.get("code").and_then(Json::as_i64));
        if (stdout.as_str(), stderr.as_str(), Some(code)) != expected {
            failures.push(format!("{case:?}\n--- rust\n{stdout}{stderr}"));
        }
    }
    assert!(failures.is_empty(), "{} of {} differ:\n{}", failures.len(), results.len(), failures.join("\n"));
}

#[test]
fn the_tables_and_example_data_match_python() {
    let reference = python_reference();
    let expected = reference.get("entries").and_then(Json::as_array).expect("entries");
    let actual = entries();
    assert_eq!(actual.len(), expected.len());
    for (rust, python) in actual.iter().zip(expected) {
        assert_eq!(rust.name, text(python, "name"));
        assert_eq!(rust.signature, text(python, "signature"));
        assert_eq!(rust.description, text(python, "description"));
        assert_eq!(rust.example, text(python, "example"));
        assert_eq!(rust.result, text(python, "result"));
    }
    assert!(python_equal(&example_declarations(), reference.get("declarations").expect("declarations")));
    assert!(python_equal(&example_responses(), reference.get("responses").expect("responses")));
}
