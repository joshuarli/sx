use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sx_cores::kernel::cores;
use sx_kernel::help::entry_names;
use sx_kernel::json::{Json, Layout, dumps, parse};
use sx_kernel::registry::Registry;

mod common;
use common::{json_case_runs_in_python, source_runs_in_python};

const HELP_CHANGES: &[(&str, &str)] = &[
    (
        r#"usage: sx [-h] {help,cores,check,explain,run} ...

Check, explain, and run Sx programs.

positional arguments:
  {help,cores,check,explain,run}
    help                Print the guide, or the entry for one name.
    cores               List the registered cores and the evidence for each.
    check               Check a program and print its diagnostics.
    explain             Print the tools, effects, inputs, and result type of a
                        program.
    run                 Run a program against recorded tool responses.

options:
  -h, --help            show this help message and exit
"#,
        r#"usage: sx [-h] {help,cores,check,explain,run} ...

Check, explain, and run Sx programs.

positional arguments:
  {help,cores,check,explain,run}
    help                Print the guide, or the entry for one name.
    cores               List the registered cores and the evidence for each.
    check               Check a program and print its diagnostics.
    explain             Print the tools, effects, inputs, and result type of a
                        program.
    run                 Run a program against recorded responses or packs.

options:
  -h, --help            show this help message and exit
"#,
    ),
    (
        r#"usage: sx check [-h] [--declarations DECLARATIONS] [--allow ID]
                [--format {text,json}] [--emit {ir}]
                file

positional arguments:
  file

options:
  -h, --help            show this help message and exit
  --declarations DECLARATIONS
                        JSON declarations of tools, types, and inputs
  --allow ID            Authorize one tool
  --format {text,json}
  --emit {ir}           Print the checked semantic IR as JSON
"#,
        r#"usage: sx check [-h] [--declarations DECLARATIONS] [--allow ID]
                [--grants GRANTS] [--format {text,json}] [--emit {ir}]
                file

positional arguments:
  file

options:
  -h, --help            show this help message and exit
  --declarations DECLARATIONS
                        JSON declarations of tools, types, and inputs
  --allow ID            Authorize one tool that declares no scope
  --grants GRANTS       JSON object granting tools, with their scopes
  --format {text,json}
  --emit {ir}           Print the checked semantic IR as JSON
"#,
    ),
    (
        r#"usage: sx explain [-h] [--declarations DECLARATIONS] [--allow ID]
                  [--format {text,json}]
                  file

positional arguments:
  file

options:
  -h, --help            show this help message and exit
  --declarations DECLARATIONS
                        JSON declarations of tools, types, and inputs
  --allow ID            Authorize one tool
  --format {text,json}
"#,
        r#"usage: sx explain [-h] [--declarations DECLARATIONS] [--allow ID]
                  [--grants GRANTS] [--format {text,json}]
                  file

positional arguments:
  file

options:
  -h, --help            show this help message and exit
  --declarations DECLARATIONS
                        JSON declarations of tools, types, and inputs
  --allow ID            Authorize one tool that declares no scope
  --grants GRANTS       JSON object granting tools, with their scopes
  --format {text,json}
"#,
    ),
    (
        r#"usage: sx run [-h] [--declarations DECLARATIONS] [--allow ID]
              [--format {text,json}] [--responses RESPONSES]
              [--input NAME=JSON]
              file

positional arguments:
  file

options:
  -h, --help            show this help message and exit
  --declarations DECLARATIONS
                        JSON declarations of tools, types, and inputs
  --allow ID            Authorize one tool
  --format {text,json}
  --responses RESPONSES
                        JSON list of recorded tool responses
  --input NAME=JSON     One input value
"#,
        r#"usage: sx run [-h] [--declarations DECLARATIONS] [--allow ID]
              [--grants GRANTS] [--format {text,json}]
              [--responses RESPONSES] [--input NAME=JSON]
              file

positional arguments:
  file

options:
  -h, --help            show this help message and exit
  --declarations DECLARATIONS
                        JSON declarations of tools, types, and inputs
  --allow ID            Authorize one tool that declares no scope
  --grants GRANTS       JSON object granting tools, with their scopes
  --format {text,json}
  --responses RESPONSES
                        JSON list of recorded tool responses; without it,
                        --grants runs pack tools for real
  --input NAME=JSON     One input value
"#,
    ),
];

fn usage_of(help: &str) -> &str {
    help.find("\n\n").and_then(|end| help.get(..end + 1)).unwrap_or(help)
}

fn with_rust_help(text: &str) -> String {
    let mut text = text.to_string();
    for (python, rust) in HELP_CHANGES {
        text = text.replace(python, rust).replace(usage_of(python), usage_of(rust));
    }
    text
}

const CORE_IDS: [&str; 4] = ["text", "semver", "git", "github"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scratch() -> PathBuf {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join("cli-parity");
    fs::create_dir_all(&directory).expect("scratch directory");
    directory
}

fn write(name: &str, contents: &str) -> String {
    let path = scratch().join(name);
    fs::write(&path, contents).expect("scratch file");
    path.to_string_lossy().into_owned()
}

#[derive(Clone, Debug)]
struct Case {
    name: String,
    argv: Vec<String>,
    rust_argv: Option<Vec<String>>,
    cores: Vec<&'static str>,
}

impl Case {
    fn new(name: impl Into<String>, argv: &[&str]) -> Case {
        Case { name: name.into(), argv: argv.iter().map(|a| (*a).to_string()).collect(), rust_argv: None, cores: Vec::new() }
    }

    fn owned(name: impl Into<String>, argv: Vec<String>) -> Case {
        Case { name: name.into(), argv, rust_argv: None, cores: Vec::new() }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Seen {
    stdout: String,
    stderr: String,
    code: i64,
}

fn python_results(label: &str, cases: &[Case]) -> Vec<Option<Seen>> {
    let manifest: Vec<Json> = cases
        .iter()
        .map(|case| {
            Json::object([
                ("argv", Json::Array(case.argv.iter().map(|a| Json::string(a.as_str())).collect())),
                ("cores", Json::Array(case.cores.iter().map(|c| Json::string(*c)).collect())),
            ])
        })
        .collect();
    let manifest_path = scratch().join(format!("{label}-manifest.json"));
    let target = scratch().join(format!("{label}-python.json"));
    fs::write(&manifest_path, dumps(&Json::Array(manifest), Layout::Spaced).expect("manifest")).expect("write manifest");
    let status = Command::new("uv")
        .args(["run", "--quiet", "python", "rust/scripts/cli_reference.py"])
        .arg(&manifest_path)
        .arg(&target)
        .current_dir(root())
        .status()
        .expect("uv runs the Python CLI reference");
    assert!(status.success(), "rust/scripts/cli_reference.py failed");
    let results = parse(&fs::read_to_string(&target).expect("results")).expect("results json");
    let results = results.as_array().expect("result list");
    assert_eq!(results.len(), cases.len());
    results
        .iter()
        .map(|result| {
            if result.get("crash").is_some() {
                return None;
            }
            Some(Seen {
                stdout: result.get("stdout").and_then(Json::as_str).expect("stdout").to_string(),
                stderr: result.get("stderr").and_then(Json::as_str).expect("stderr").to_string(),
                code: result.get("code").and_then(Json::as_i64).expect("code"),
            })
        })
        .collect()
}

fn all_cores() -> Registry {
    let mut registry = Registry::new();
    for core in python_cores() {
        registry.add_core(core).expect("core registers");
    }
    registry
}

fn python_cores() -> Vec<sx_kernel::registry::Core> {
    cores().into_iter().filter(|core| CORE_IDS.contains(&core.id.as_str())).collect()
}

fn rust_result(case: &Case) -> Seen {
    let argv = case.rust_argv.as_ref().unwrap_or(&case.argv);
    if !case.cores.is_empty() && argv.first().is_some_and(|command| command == "cores") {
        let exit = sx_cli::run(argv, &all_cores);
        return Seen { stdout: exit.stdout, stderr: exit.stderr, code: i64::from(exit.code) };
    }
    let output = Command::new(env!("CARGO_BIN_EXE_sx"))
        .args(argv)
        .current_dir(root())
        .env_remove("COLUMNS")
        .output()
        .expect("the sx binary runs");
    Seen {
        stdout: String::from_utf8(output.stdout).expect("utf-8 stdout"),
        stderr: String::from_utf8(output.stderr).expect("utf-8 stderr"),
        code: i64::from(output.status.code().expect("exit status")),
    }
}

fn compare(label: &str, cases: &[Case]) {
    assert!(!cases.is_empty());
    let python = python_results(label, cases);
    let mut failures = Vec::new();
    let mut crashes = Vec::new();
    for (case, python) in cases.iter().zip(&python) {
        let Some(python) = python else {
            crashes.push(case.name.clone());
            continue;
        };
        let python = &Seen { stdout: with_rust_help(&python.stdout), stderr: with_rust_help(&python.stderr), code: python.code };
        let rust = rust_result(case);
        if &rust != python {
            failures.push(format!("{}: {:?}\n--- rust\n{rust:?}\n--- python\n{python:?}", case.name, case.argv));
        }
    }
    eprintln!("{label}: {} cases compared", cases.len());
    assert!(crashes.is_empty(), "Python crashed on {crashes:?}");
    assert!(failures.is_empty(), "{} of {} differ:\n{}", failures.len(), cases.len(), failures.join("\n"));
}

fn s(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| (*p).to_string()).collect()
}

fn fixture(name: &str) -> String {
    format!("fixtures/cli/{name}")
}

fn scenario_cases() -> Vec<Case> {
    let declarations = fixture("declarations.json");
    let uptimes = fixture("uptimes.sx");
    let responses = fixture("responses.json");
    vec![
        Case::owned(
            "run-prints-the-result",
            s(&["run", &uptimes, "--declarations", &declarations, "--responses", &responses, "--allow", "metrics.uptime", "--input", "hosts=[\"db1\",\"db2\"]", "--format", "json"]),
        ),
        Case::owned("check-rejects-unauthorized", s(&["check", &uptimes, "--declarations", &declarations, "--format", "json"])),
        Case::owned("explain-before-authorization", s(&["explain", &fixture("explain.sx"), "--declarations", &declarations, "--format", "json"])),
        Case::owned(
            "run-unexpected-call",
            s(&["run", &uptimes, "--declarations", &declarations, "--responses", &responses, "--allow", "metrics.uptime", "--input", "hosts=[\"db9\"]"]),
        ),
        Case::owned(
            "run-unexpected-call-json",
            s(&["run", &uptimes, "--declarations", &declarations, "--responses", &responses, "--allow", "metrics.uptime", "--input", "hosts=[\"db1\",\"db9\"]", "--format", "json"]),
        ),
        Case::owned("missing-input", s(&["run", &uptimes, "--declarations", &declarations, "--allow", "metrics.uptime"])),
        Case::new("help-suggests", &["help", "filtr"]),
        Case::owned("help-declared-tool", s(&["help", "pager.alert", "--declarations", &declarations])),
    ]
}

fn matrix_cases() -> Vec<Case> {
    let declarations = fixture("declarations.json");
    let responses = fixture("responses.json");
    let mut cases = Vec::new();
    let allows: [&[&str]; 4] = [&[], &["metrics.uptime"], &["metrics.uptime", "pager.alert"], &["pager.alert"]];
    let formats: [&[&str]; 3] = [&[], &["--format", "text"], &["--format", "json"]];
    let inputs: [&[&str]; 5] = [
        &[],
        &["--input", "hosts=[\"db1\",\"db2\"]"],
        &["--input", "hosts=[\"db9\"]"],
        &["--input", "hosts=[]"],
        &["--input", "hosts=[\"db1\"]", "--input", "hosts=[\"db2\"]"],
    ];
    for file in ["uptimes.sx", "explain.sx", "bad.sx"] {
        for command in ["check", "explain", "run"] {
            for with_declarations in [true, false] {
                for (a, allow) in allows.iter().enumerate() {
                    for (f, format) in formats.iter().enumerate() {
                        for (i, input) in inputs.iter().enumerate() {
                            if command != "run" && i > 0 {
                                continue;
                            }
                            let mut argv = s(&[command, &fixture(file)]);
                            if with_declarations {
                                argv.extend(s(&["--declarations", &declarations]));
                            }
                            for id in *allow {
                                argv.extend(s(&["--allow", id]));
                            }
                            argv.extend(s(format));
                            argv.extend(s(input));
                            if command == "run" && i % 2 == 0 {
                                argv.extend(s(&["--responses", &responses]));
                            }
                            cases.push(Case::owned(format!("{command}-{file}-{with_declarations}-{a}-{f}-{i}"), argv));
                        }
                    }
                }
            }
        }
        for format in formats {
            for with_declarations in [true, false] {
                let mut argv = s(&["check", &fixture(file), "--emit", "ir", "--allow", "metrics.uptime"]);
                if with_declarations {
                    argv.extend(s(&["--declarations", &declarations]));
                }
                argv.extend(s(format));
                cases.push(Case::owned(format!("emit-{file}-{with_declarations}"), argv));
            }
        }
    }
    cases
}

fn help_cases() -> Vec<Case> {
    let declarations = fixture("declarations.json");
    let mut cases = Vec::new();
    for level in ["minimal", "standard", "full"] {
        cases.push(Case::new(format!("guide-{level}"), &["help", "--level", level]));
        cases.push(Case::owned(
            format!("guide-{level}-declared"),
            s(&["help", "--level", level, "--declarations", &declarations]),
        ));
    }
    cases.push(Case::new("guide-default", &["help"]));
    for name in entry_names() {
        cases.push(Case::owned(format!("help-{name}"), s(&["help", &name])));
    }
    for name in ["metrics.uptime", "pager.alert", "Host", "nothing", "metrics.uptim", "metrics.uptime ", "Hots", "", "ho", "-", "-5"] {
        cases.push(Case::owned(format!("declared-help-{name}"), s(&["help", name, "--declarations", &declarations])));
        cases.push(Case::owned(format!("plain-help-{name}"), s(&["help", name])));
    }
    cases
}

fn core_cases() -> Vec<Case> {
    let declarations = fixture("declarations.json");
    let base = fs::read_to_string(root().join(&declarations)).expect("declarations");
    let base = parse(&base).expect("declarations json");
    let mut with_cores = base.as_object().expect("object").to_vec();
    with_cores.push(("cores".to_string(), Json::Array(CORE_IDS.iter().map(|c| Json::string(*c)).collect())));
    let rust_declarations = write("with-cores.json", &dumps(&Json::Object(with_cores), Layout::Spaced).expect("json"));
    let python_declarations = write("without-cores.json", &dumps(&base, Layout::Spaced).expect("json"));
    let mut names: Vec<String> = CORE_IDS.iter().map(|c| (*c).to_string()).collect();
    for core in python_cores() {
        names.extend(core.functions.iter().map(|f| f.name.clone()));
        names.extend(core.types.iter().map(|(name, _)| name.clone()));
    }
    names.extend(["semvr".to_string(), "text.splt".to_string(), "github.CiStat".to_string()]);
    let mut cases = Vec::new();
    for name in names {
        let mut case = Case::owned(format!("core-help-{name}"), s(&["help", &name, "--declarations", &python_declarations]));
        case.rust_argv = Some(s(&["help", &name, "--declarations", &rust_declarations]));
        case.cores = CORE_IDS.to_vec();
        cases.push(case);
    }
    for format in [&[][..], &["--format", "text"], &["--format", "json"]] {
        let mut argv = vec!["cores".to_string()];
        argv.extend(s(format));
        let mut case = Case::owned(format!("cores-{format:?}"), argv);
        case.cores = CORE_IDS.to_vec();
        cases.push(case);
        cases.push(Case::owned(format!("plain-cores-{format:?}"), {
            let mut argv = vec!["cores".to_string()];
            argv.extend(s(format));
            argv
        }));
    }
    cases
}

const ARGUMENT_ERRORS: &[&[&str]] = &[
    &[],
    &["bogus"],
    &["chek", "f"],
    &["explan"],
    &["hel"],
    &["--bogus"],
    &["--bogus", "help"],
    &["-h"],
    &["--help"],
    &["--he"],
    &["help", "-h"],
    &["help", "--help"],
    &["help", "--level", "x"],
    &["help", "--level", "mini"],
    &["help", "--level"],
    &["help", "--level="],
    &["help", "--level=minimal", "--", "filter"],
    &["help", "--level", "full", "filter"],
    &["help", "filter", "--level", "full"],
    &["help", "a", "b"],
    &["help", "--declarations"],
    &["help", "--decl", "x"],
    &["help", "--de=x"],
    &["help", "-x"],
    &["help", "-"],
    &["help", "--", "--level"],
    &["help", "two words"],
    &["cores", "x"],
    &["cores", "--format", "xml"],
    &["cores", "--form", "json"],
    &["cores", "-h"],
    &["check"],
    &["check", "-h"],
    &["check", "f", "-h"],
    &["check", "f", "-hx"],
    &["check", "--bogus", "f"],
    &["check", "f", "--format"],
    &["check", "f", "--emit"],
    &["check", "f", "--emit", "xml"],
    &["check", "f", "--emit", "ri"],
    &["check", "f", "--format=xml"],
    &["check", "f", "--format", "jsn"],
    &["check", "--format", "json"],
    &["check", "f", "g"],
    &["check", "f", "--allow"],
    &["check", "f", "--bogus", "--bogus2"],
    &["check", "f", "g", "--bogus"],
    &["check", "--", "-x"],
    &["check", "--e", "ir", "f"],
    &["explain"],
    &["explain", "-h"],
    &["explain", "f", "--emit", "ir"],
    &["run"],
    &["run", "-h"],
    &["run", "f", "--input"],
    &["run", "f", "--in"],
    &["run", "f", "--responses"],
    &["run", "f", "--emit", "ir"],
    &["run", "f", "-5"],
];

fn argument_cases() -> Vec<Case> {
    ARGUMENT_ERRORS.iter().enumerate().map(|(i, argv)| Case::new(format!("argument-{i}-{argv:?}"), argv)).collect()
}

const JSON_TEXTS: &[&str] = &[
    "", " ", "[", "[1", "[1,", "[1,]", "{", "{\"a\"", "{\"a\":", "{\"a\":1", "{\"a\":1,", "{\"a\":1,}", "{,}", "\"abc", "\"a\\x\"",
    "\"a\\u12\"", "\"a\\u12zz\"", "\"a\nb\"", "\"a\\", "-", "-x", "01", "1.", "1e", "nul", "tru", "Infinit", "[1 2]", "{\"a\" 1}",
    "{\"a\":1 \"b\":2}", "1 2", "\u{feff}1", "[1,,2]", "{\"a\":,}", "{1:2}", "['a']", "\t\n[\n1\n,\n]", "[\"é\" x]", "{\"é\":}",
    "[1]]", "nulll", "{\"a\":1}}", "[\"a\\u00zz\"]", "\"\\ud800\\u12\"", "tab\there", "[1e5, 2.5E-3, -0]", "[\n\n  1,\n  oops\n]",
    "\"unterminated\\", "{\"a\":\"b\" \"c\"}", "[[[[1]]]", "{\"k\":[1,2,}", "+1", ".5", "1.5.5", "0x10", "[true false]", "[\"a\"\"b\"]",
];

fn file_cases() -> Vec<Case> {
    let declarations = fixture("declarations.json");
    let uptimes = fixture("uptimes.sx");
    let mut cases = Vec::new();
    let nowhere = scratch().join("nowhere.json").to_string_lossy().into_owned();
    let directory = scratch().to_string_lossy().into_owned();
    for command in ["check", "explain", "run"] {
        cases.push(Case::owned(format!("missing-file-{command}"), s(&[command, &nowhere, "--declarations", &declarations])));
        cases.push(Case::owned(format!("missing-declarations-{command}"), s(&[command, &uptimes, "--declarations", &nowhere])));
        cases.push(Case::owned(format!("directory-declarations-{command}"), s(&[command, &uptimes, "--declarations", &directory])));
        cases.push(Case::owned(format!("directory-file-{command}"), s(&[command, &directory])));
        cases.push(Case::owned(format!("dotted-path-{command}"), s(&[command, "./fixtures//cli/./nope.sx"])));
    }
    cases.push(Case::owned("missing-responses", s(&["run", &uptimes, "--declarations", &declarations, "--allow", "metrics.uptime", "--input", "hosts=[]", "--responses", &nowhere])));
    cases.push(Case::owned("missing-help-declarations", s(&["help", "--declarations", &nowhere])));
    for (i, text) in JSON_TEXTS.iter().enumerate() {
        let path = write(&format!("bad-{i}.json"), text);
        cases.push(Case::owned(format!("bad-declarations-{i}"), s(&["check", &uptimes, "--declarations", &path])));
        cases.push(Case::owned(format!("bad-help-declarations-{i}"), s(&["help", "--declarations", &path])));
        if parse(text).is_err() {
            cases.push(Case::owned(
                format!("bad-responses-{i}"),
                s(&["run", &uptimes, "--declarations", &declarations, "--allow", "metrics.uptime", "--input", "hosts=[]", "--responses", &path]),
            ));
        }
        cases.push(Case::owned(
            format!("bad-input-{i}"),
            s(&["run", &uptimes, "--declarations", &declarations, "--allow", "metrics.uptime", "--input", &format!("hosts={text}")]),
        ));
    }
    let object_cases: &[(&str, &str)] = &[
        ("list-declarations", "[]"),
        ("number-declarations", "3"),
        ("null-declarations", "null"),
        ("empty-declarations", "{}"),
        ("bad-effect", "{\"capabilities\": [{\"id\": \"a.b\", \"effect\": \"destroy\", \"input\": \"Record {}\", \"output\": \"Unit\", \"description\": \"d\"}]}"),
        ("missing-key", "{\"capabilities\": [{\"id\": \"a.b\", \"effect\": \"read\", \"input\": \"Record {}\", \"description\": \"d\"}]}"),
        ("bad-type", "{\"types\": {\"A\": \"Recrd {}\"}}"),
        ("bad-capability-type", "{\"capabilities\": [{\"id\": \"a.b\", \"effect\": \"read\", \"input\": \"Wat\", \"output\": \"Unit\", \"description\": \"d\"}]}"),
        ("bad-capability-id", "{\"capabilities\": [{\"id\": \"ab\", \"effect\": \"read\", \"input\": \"Record {}\", \"output\": \"Unit\", \"description\": \"d\"}]}"),
        ("duplicate-capability", "{\"capabilities\": [{\"id\": \"a.b\", \"effect\": \"read\", \"input\": \"Record {}\", \"output\": \"Unit\", \"description\": \"d\"}, {\"id\": \"a.b\", \"effect\": \"read\", \"input\": \"Record {}\", \"output\": \"Unit\", \"description\": \"d\"}]}"),
        ("inputs-list", "{\"inputs\": []}"),
        ("inputs-bad-type", "{\"inputs\": {\"x\": \"Wat\"}}"),
        ("inputs-number-type", "{\"inputs\": {\"x\": 5}}"),
        ("inputs-int", "{\"inputs\": {\"n\": \"Int\"}}"),
        ("inputs-record", "{\"inputs\": {\"r\": \"Record {a: Int, b: Option<String>}\"}}"),
    ];
    for (name, text) in object_cases {
        let path = write(&format!("{name}.json"), text);
        for command in ["check", "explain"] {
            cases.push(Case::owned(format!("{command}-{name}"), s(&[command, &fixture("bad.sx"), "--declarations", &path])));
        }
        cases.push(Case::owned(format!("help-{name}"), s(&["help", "Host", "--declarations", &path])));
    }
    let program = write("int-input.sx", "(+ n 1)");
    let int_inputs = write("int-inputs.json", "{\"inputs\": {\"n\": \"Int\"}}");
    for input in ["n=1", "n=1.5", "n=\"s\"", "n=", "n", "m=1", "=1", "n=9223372036854775808", "n=-9223372036854775808", "n=1e2", "n=true", "n=null", "n=NaN", "n=01", "n=1 2"] {
        cases.push(Case::owned(format!("int-input-{input}"), s(&["run", &program, "--declarations", &int_inputs, "--input", input])));
        cases.push(Case::owned(format!("int-input-json-{input}"), s(&["run", &program, "--declarations", &int_inputs, "--input", input, "--format", "json"])));
    }
    cases.push(Case::owned("int-input-twice", s(&["run", &program, "--declarations", &int_inputs, "--input", "n=1", "--input", "n=5"])));
    cases.push(Case::owned("int-input-explain", s(&["explain", &program, "--declarations", &int_inputs])));
    cases.push(Case::owned("int-input-check-json", s(&["check", &program, "--declarations", &int_inputs, "--format", "json"])));
    cases.push(Case::owned("int-input-emit", s(&["check", &program, "--declarations", &int_inputs, "--emit", "ir"])));
    cases.push(Case::owned("allow-unknown", s(&["check", &uptimes, "--declarations", &declarations, "--allow", "nope.nothing"])));
    cases.push(Case::owned("allow-unknown-sorted", s(&["check", &uptimes, "--declarations", &declarations, "--allow", "zzz.b", "--allow", "aaa.b"])));
    cases.push(Case::owned("allow-without-declarations", s(&["check", &uptimes, "--allow", "metrics.uptime"])));
    let crlf = write("crlf.sx", "(let [a 1\r\n      b 2]\r\n  (+ a \"x\"))\r\n");
    let lone_cr = write("lone-cr.sx", "(let [a 1\r      b 2]\r  (+ a \"x\"))");
    for format in ["text", "json"] {
        cases.push(Case::owned(format!("crlf-{format}"), s(&["check", &crlf, "--format", format])));
        cases.push(Case::owned(format!("lone-cr-{format}"), s(&["check", &lone_cr, "--format", format])));
    }
    let responses_not_list = write("responses-object.json", "{}");
    cases.push(Case::owned("responses-not-list", s(&["run", &uptimes, "--declarations", &declarations, "--allow", "metrics.uptime", "--input", "hosts=[]", "--responses", &responses_not_list])));
    let unicode = write("unicode.sx", "(str \"é\" \"\u{1F600}\" 1)");
    let unicode_bad = write("unicode-bad.sx", "(+ \"é\u{1F600}\" 1)");
    for format in ["text", "json"] {
        cases.push(Case::owned(format!("unicode-run-{format}"), s(&["run", &unicode, "--format", format])));
        cases.push(Case::owned(format!("unicode-bad-{format}"), s(&["check", &unicode_bad, "--format", format])));
    }
    cases
}

fn sorted_files(directory: &str, extension: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(root().join(directory))
        .expect("fixture directory")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == extension))
        .collect();
    found.sort();
    found
}

fn stem(path: &Path) -> String {
    path.file_stem().and_then(|name| name.to_str()).expect("file name").to_string()
}

fn run_fixture_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for path in sorted_files("fixtures/run", "json") {
        if path.ends_with("declarations.json") {
            continue;
        }
        let fixture = parse(&fs::read_to_string(&path).expect("fixture")).expect("fixture json");
        if fixture.get("steps").is_some() || fixture.get("budgets").is_some() || !json_case_runs_in_python(&fixture) {
            continue;
        }
        let name = stem(&path);
        let program = write(&format!("run-{name}.sx"), fixture.get("program").and_then(Json::as_str).expect("program"));
        let responses = write(
            &format!("run-{name}-responses.json"),
            &dumps(fixture.get("responses").unwrap_or(&Json::Array(Vec::new())), Layout::Spaced).expect("responses"),
        );
        let declarations = format!("fixtures/run/{}", fixture.get("declarations").and_then(Json::as_str).expect("declarations"));
        let mut common = s(&["--declarations", &declarations]);
        for id in fixture.get("allow").and_then(Json::as_array).expect("allow") {
            common.extend(s(&["--allow", id.as_str().expect("id")]));
        }
        let with = |command: &[&str], extra: &[&str]| {
            let mut argv = s(command);
            argv.push(program.clone());
            argv.extend(common.iter().cloned());
            argv.extend(s(extra));
            argv
        };
        cases.push(Case::owned(format!("run-{name}-json"), with(&["run"], &["--responses", &responses, "--format", "json"])));
        cases.push(Case::owned(format!("run-{name}-text"), with(&["run"], &["--responses", &responses])));
        cases.push(Case::owned(format!("emit-{name}"), with(&["check"], &["--emit", "ir"])));
        cases.push(Case::owned(format!("check-{name}"), with(&["check"], &["--format", "json"])));
        cases.push(Case::owned(format!("explain-{name}"), with(&["explain"], &["--format", "json"])));
        cases.push(Case::owned(format!("explain-text-{name}"), with(&["explain"], &[])));
    }
    cases
}

fn check_fixture_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for path in sorted_files("fixtures/check", "sx") {
        let source = fs::read_to_string(&path).expect("fixture source");
        if !source_runs_in_python(&source) {
            continue;
        }
        let mut lines = source.split('\n');
        let Some(declarations) = lines.next().and_then(|line| line.strip_prefix("; declarations: ")) else {
            continue;
        };
        let name = stem(&path);
        let file = write(&format!("check-{name}.sx"), &source);
        let mut common = s(&["--declarations", declarations]);
        if let Some(allowed) = lines.next().and_then(|line| line.strip_prefix("; allow: ")) {
            for id in allowed.split_whitespace() {
                common.extend(s(&["--allow", id]));
            }
        }
        for format in ["text", "json"] {
            let mut argv = s(&["check", &file]);
            argv.extend(common.iter().cloned());
            argv.extend(s(&["--format", format]));
            cases.push(Case::owned(format!("check-fixture-{name}-{format}"), argv));
        }
    }
    cases
}

#[test]
fn every_run_fixture_matches_python_through_the_command_line() {
    compare("run-fixtures", &run_fixture_cases());
}

#[test]
fn every_check_fixture_matches_python_through_the_command_line() {
    compare("check-fixtures", &check_fixture_cases());
}

#[test]
fn the_test_scenarios_match_python() {
    compare("scenarios", &scenario_cases());
}

#[test]
fn the_cli_fixture_matrix_matches_python() {
    compare("matrix", &matrix_cases());
}

#[test]
fn every_help_entry_and_declared_tool_matches_python() {
    compare("help", &help_cases());
}

#[test]
fn cores_and_core_help_match_python() {
    compare("cores", &core_cases());
}

#[test]
fn argument_errors_match_python() {
    compare("arguments", &argument_cases());
}

#[test]
fn file_and_json_errors_match_python() {
    compare("files", &file_cases());
}
