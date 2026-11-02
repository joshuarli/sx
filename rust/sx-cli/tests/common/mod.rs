#![allow(dead_code)]

use sx_kernel::json::Json;

const PYTHON_MARKER: &str = "; python: no";

pub fn json_case_runs_in_python(case: &Json) -> bool {
    case.get("python").and_then(Json::as_bool) != Some(false)
}

pub fn source_runs_in_python(source: &str) -> bool {
    !source.split('\n').take_while(|line| line.starts_with(';')).any(|line| line == PYTHON_MARKER)
}
