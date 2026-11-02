use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use sx_kernel::json::{Json, Layout, dumps, parse};

struct Kernel {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Kernel {
    fn spawn() -> Kernel {
        let mut child = Command::new(env!("CARGO_BIN_EXE_sx"))
            .arg("serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("sx serve starts");
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
        Kernel { child, stdin, stdout }
    }

    fn exchange(&mut self, message: Json) -> Json {
        let text = dumps(&message, Layout::Spaced).expect("serializable");
        self.stdin.write_all(text.as_bytes()).and_then(|()| self.stdin.write_all(b"\n")).expect("kernel reads");
        self.stdin.flush().expect("kernel reads");
        let mut line = String::new();
        self.stdout.read_line(&mut line).expect("kernel answers");
        parse(&line).unwrap_or_else(|e| panic!("{e}: {line}"))
    }
}

impl Drop for Kernel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn text(path: &Path) -> &str {
    path.to_str().expect("test paths are UTF-8")
}

fn str_of<'a>(json: &'a Json, key: &str) -> &'a str {
    json.get(key).and_then(Json::as_str).unwrap_or_else(|| panic!("no {key} in {}", shown(json)))
}

fn shown(json: &Json) -> String {
    dumps(json, Layout::Spaced).expect("serializable")
}

fn sandbox(label: &str) -> (PathBuf, PathBuf) {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("serve-scope-{}-{label}", std::process::id()));
    if base.exists() {
        fs::remove_dir_all(&base).expect("old sandbox removed");
    }
    fs::create_dir_all(base.join("root")).expect("root");
    fs::create_dir_all(base.join("outside")).expect("outside");
    let base = fs::canonicalize(base).expect("canonical");
    fs::write(base.join("root/note.txt"), "inside").expect("note");
    fs::write(base.join("outside/secret.txt"), "secret").expect("secret");
    (base.join("root"), base.join("outside"))
}

fn grants(root: &Path) -> Json {
    Json::object([("fs.read-text", Json::object([("roots", Json::Array(vec![Json::string(text(root))]))]))])
}

fn declarations() -> Json {
    Json::object([("packs", Json::Array(vec![Json::string("fs")]))])
}

#[test]
fn a_protocol_host_asks_the_kernel_for_each_requests_verdict() {
    let (root, outside) = sandbox("verdicts");
    let mut kernel = Kernel::spawn();
    let program = format!(
        "(par (attempt (call fs.read-text {{:path (str \"{}\" \"/note.txt\")}})) (attempt (call fs.read-text {{:path (str \"{}\" \"/secret.txt\")}})))",
        text(&root),
        text(&outside)
    );
    let requests = kernel.exchange(Json::object([
        ("version", Json::Int(1)),
        ("type", Json::string("start")),
        ("program", Json::string(program)),
        ("declarations", declarations()),
        ("grants", grants(&root)),
    ]));
    assert_eq!(str_of(&requests, "type"), "requests", "{}", shown(&requests));
    let listed = requests.get("requests").and_then(Json::as_array).expect("requests").to_vec();
    assert_eq!(listed.len(), 2);
    let mut responses = Vec::new();
    for request in listed.iter() {
        let id = str_of(request, "id");
        let verdict = kernel.exchange(Json::object([("type", Json::string("check-scope")), ("id", Json::string(id))]));
        assert_eq!(str_of(&verdict, "type"), "scope-verdict");
        assert_eq!(str_of(&verdict, "id"), id);
        let path = str_of(request.get("input").expect("input"), "path");
        let response = match str_of(&verdict, "verdict") {
            "allowed" => Json::object([
                ("id", Json::string(id)),
                ("output", Json::string(fs::read_to_string(path).expect("the host reads"))),
            ]),
            "denied" => {
                assert!(str_of(&verdict, "reason").contains("outside every granted root"), "{}", shown(&verdict));
                Json::object([
                    ("id", Json::string(id)),
                    (
                        "error",
                        Json::object([
                            ("kind", Json::string("PermissionDenied")),
                            ("message", Json::string(str_of(&verdict, "reason"))),
                        ]),
                    ),
                ])
            }
            other => panic!("unexpected verdict {other}"),
        };
        responses.push(response);
    }
    let done = kernel.exchange(Json::object([("type", Json::string("respond")), ("responses", Json::Array(responses))]));
    assert_eq!(str_of(&done, "type"), "completed", "{}", shown(&done));
    let value = shown(done.get("value").expect("value"));
    assert!(value.starts_with(r#"[{"ok": "inside"}, {"err": {"kind": "PermissionDenied", "message": "#), "{value}");
}

#[test]
fn grants_reach_the_checker_and_explain_and_misuse_is_an_error() {
    let (root, outside) = sandbox("misuse");
    let mut kernel = Kernel::spawn();
    let literal = format!("(call fs.read-text {{:path \"{}/secret.txt\"}})", text(&outside));
    let rejected = kernel.exchange(Json::object([
        ("version", Json::Int(1)),
        ("type", Json::string("start")),
        ("program", Json::string(literal.as_str())),
        ("declarations", declarations()),
        ("grants", grants(&root)),
    ]));
    assert_eq!(str_of(&rejected, "type"), "rejected", "{}", shown(&rejected));
    let diagnostic = rejected.get("diagnostics").and_then(Json::as_array).and_then(|d| d.first()).expect("diagnostic");
    assert_eq!(str_of(diagnostic, "code"), "out-of-scope");
    let explained = kernel.exchange(Json::object([
        ("type", Json::string("explain")),
        ("program", Json::string(literal.as_str())),
        ("declarations", declarations()),
        ("grants", grants(&root)),
    ]));
    let tool = explained.get("tools").and_then(Json::as_array).and_then(|t| t.first()).expect("tool");
    let verdict = tool.get("verdicts").and_then(Json::as_array).and_then(|v| v.first()).expect("verdict");
    assert_eq!(str_of(verdict, "verdict"), "denied");
    let unscoped = kernel.exchange(Json::object([
        ("type", Json::string("start")),
        ("program", Json::string(literal.as_str())),
        ("declarations", declarations()),
        ("allow", Json::Array(vec![Json::string("fs.read-text")])),
    ]));
    assert_eq!((str_of(&unscoped, "type"), str_of(&unscoped, "code")), ("error", "invalid-grants"));
    let idle = kernel.exchange(Json::object([("type", Json::string("check-scope")), ("id", Json::string("r:1"))]));
    assert_eq!((str_of(&idle, "type"), str_of(&idle, "code")), ("error", "unexpected-message"));
}
