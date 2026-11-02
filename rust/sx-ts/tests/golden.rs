mod common;

use std::fs;
use std::path::PathBuf;

#[test]
fn golden_lowerings() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let bless = std::env::var("SX_TS_BLESS").is_ok();
    let mut failures = Vec::new();
    let mut names: Vec<PathBuf> = fs::read_dir(&dir).expect("golden dir").map(|e| e.expect("entry").path()).collect();
    names.sort();
    for path in names.iter().filter(|p| p.extension().is_some_and(|e| e == "ts")) {
        let source = fs::read_to_string(path).expect("read ts");
        let actual = match common::lowered(&source) {
            Ok(text) => text,
            Err(diagnostics) => {
                let shown: Vec<String> = diagnostics.iter().map(|d| format!("{} {} {}", d.code, d.span, d.message)).collect();
                format!("REJECTED\n{}\n", shown.join("\n"))
            }
        };
        let expected_path = path.with_extension("sx");
        if bless {
            fs::write(&expected_path, &actual).expect("write sx");
            continue;
        }
        let expected = fs::read_to_string(&expected_path).unwrap_or_default();
        if expected != actual {
            failures.push(format!("{}:\n--- expected\n{expected}\n--- actual\n{actual}", path.display()));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
