use std::fs;
use std::path::Path;
use std::process::Command;

use sx_kernel::help::EXTENSIONS;

const SX: &str = env!("CARGO_BIN_EXE_sx");

#[test]
fn every_help_extension_example_without_inputs_produces_its_documented_result() {
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("help-extensions-{}", std::process::id()));
    fs::create_dir_all(&scratch).expect("scratch directory");
    let mut ran = 0;
    for entry in EXTENSIONS.iter().filter(|entry| entry.name != "script") {
        let program = scratch.join(format!("{}.sx", entry.name));
        fs::write(&program, entry.example).expect("program file");
        let output = Command::new(SX).arg("run").arg(&program).output().expect("sx starts");
        let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
        assert_eq!(stdout.trim_end(), entry.result, "{}: {}", entry.name, String::from_utf8_lossy(&output.stderr));
        ran += 1;
    }
    assert!(ran >= 5, "only {ran} extension examples ran");
}
