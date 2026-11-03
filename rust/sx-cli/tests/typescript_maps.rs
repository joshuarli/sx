use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sx_kernel::json::{Layout, dumps, parse};

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

#[test]
fn two_empty_arrays_filled_in_one_loop_keep_their_own_element_types() {
    let source = "const rows = [{ name: \"a\", n: 3 }];\nconst names = [];\nconst counts = [];\nfor (const row of rows) { names.push(row.name); counts.push(row.n); }\nreturn [names, counts];";
    assert_eq!(run("two-arrays", source), "[[\"a\"],[3]]");
}
