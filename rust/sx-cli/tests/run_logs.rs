use std::fs;
use std::path::Path;
use std::process::Command;

use sx_kernel::json::{Json, parse};

fn run(name: &str, source: &str, extra: &[&str]) -> (i32, String, String) {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("run-logs-{}", std::process::id()));
    fs::create_dir_all(&directory).expect("scratch directory");
    let program = directory.join(name);
    fs::write(&program, source).expect("program file");
    let output = Command::new(env!("CARGO_BIN_EXE_sx")).arg("run").arg(&program).args(extra).output().expect("sx runs");
    (
        output.status.code().expect("exit status"),
        String::from_utf8(output.stdout).expect("utf-8 stdout"),
        String::from_utf8(output.stderr).expect("utf-8 stderr"),
    )
}

#[test]
fn json_output_lists_the_logs_before_the_trace_and_omits_them_when_there_are_none() {
    let (code, stdout, _) = run("logs.sx", "(let [_ (log \"a\" 1) _ (log [1 2])] 7)", &["--format", "json"]);
    assert_eq!(code, 0);
    let report = parse(&stdout).expect("json");
    let names: Vec<&str> = report.as_object().expect("object").iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["outcome", "result", "logs", "trace"]);
    assert_eq!(report.get("logs").and_then(Json::as_array).map(<[Json]>::len), Some(2));

    let (_, stdout, _) = run("quiet.sx", "7", &["--format", "json"]);
    assert!(parse(&stdout).expect("json").get("logs").is_none());
}

#[test]
fn a_failed_run_keeps_the_logs_and_names_the_class_thrown() {
    let (code, stdout, _) = run("failed.sx", "(let [_ (log \"before\")] (fail \"stop\"))", &["--format", "json"]);
    assert_eq!(code, 2);
    let report = parse(&stdout).expect("json");
    assert_eq!(report.get("failure").and_then(|f| f.get("kind")).and_then(Json::as_str), Some("thrown"));
    assert_eq!(report.get("logs").and_then(Json::as_array).map(<[Json]>::len), Some(1));
}

#[test]
fn text_output_keeps_standard_output_to_the_result_and_writes_the_lines_to_standard_error() {
    let (code, stdout, stderr) = run("text.sx", "(let [_ (log \"hello\" 2)] 7)", &[]);
    assert_eq!(code, 0);
    assert_eq!(stdout, "7\n");
    assert_eq!(stderr, "log: hello 2\n");
}
