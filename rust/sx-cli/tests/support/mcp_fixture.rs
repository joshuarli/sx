use std::io::{BufRead, Write};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use sx_kernel::json::{Json, Layout, dumps, parse};

struct Fixture {
    out: Mutex<std::io::Stdout>,
    order: AtomicU64,
    inflight: AtomicU64,
    peak: AtomicU64,
    cancelled: AtomicU64,
    initialized: AtomicBool,
}

enum Outcome {
    Result(Json),
    Error(i64, &'static str),
    Exit,
}

impl Fixture {
    fn send(&self, message: Json) {
        let mut line = dumps(&message, Layout::Spaced).unwrap_or_default();
        line.push('\n');
        let mut out = self.out.lock().unwrap_or_else(|e| e.into_inner());
        let _ = out.write_all(line.as_bytes());
        let _ = out.flush();
    }

    fn reply(&self, id: &Json, result: Json) {
        self.send(Json::object([("jsonrpc", Json::string("2.0")), ("id", id.clone()), ("result", result)]));
    }

    fn fail(&self, id: &Json, code: i64, message: &str) {
        let error = Json::object([("code", Json::Int(code)), ("message", Json::string(message))]);
        self.send(Json::object([("jsonrpc", Json::string("2.0")), ("id", id.clone()), ("error", error)]));
    }
}

fn text_block(text: &str) -> Json {
    Json::object([("type", Json::string("text")), ("text", Json::string(text))])
}

fn structured(value: Json) -> Outcome {
    let text = dumps(&value, Layout::Spaced).unwrap_or_default();
    Outcome::Result(Json::object([("content", Json::Array(vec![text_block(&text)])), ("structuredContent", value)]))
}

fn text_result(parts: &[&str]) -> Outcome {
    Outcome::Result(Json::object([("content", Json::Array(parts.iter().map(|part| text_block(part)).collect()))]))
}

fn int(arguments: &Json, key: &str) -> i64 {
    arguments.get(key).and_then(Json::as_i64).unwrap_or(0)
}

fn string(arguments: &Json, key: &str) -> String {
    arguments.get(key).and_then(Json::as_str).unwrap_or("").to_string()
}

fn call(fixture: &Fixture, name: &str, arguments: &Json) -> Outcome {
    fixture.send(Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("method", Json::string("notifications/message")),
        ("params", Json::object([("level", Json::string("info")), ("data", Json::string("calling"))])),
    ]));
    fixture.send(Json::object([("jsonrpc", Json::string("2.0")), ("id", Json::string("server-ping")), ("method", Json::string("ping"))]));
    match name {
        "add" => structured(Json::object([("sum", Json::Int(int(arguments, "a") + int(arguments, "b")))])),
        "greet" => {
            let greeting = arguments.get("greeting").and_then(Json::as_str).unwrap_or("Hello").to_string();
            text_result(&[&format!("{greeting}, {}!", string(arguments, "name"))])
        }
        "multi_text" => text_result(&["a", "b"]),
        "lookup" => {
            let id = string(arguments, "id");
            structured(Json::object([
                ("id", Json::string(id.as_str())),
                ("title", Json::string(format!("Item {id}"))),
                ("internal", Json::Int(7)),
            ]))
        }
        "profile" => structured(Json::object([("name", Json::string("ada")), ("bio", Json::Null)])),
        "echo_args" => {
            let seen = dumps(arguments, Layout::Spaced).unwrap_or_default();
            structured(Json::object([("seen", Json::string(seen))]))
        }
        "shape_echo" => structured(arguments.clone()),
        "bad_read" | "bad_write" => structured(Json::object([("sum", Json::string("nope"))])),
        "fail_read" | "fail_write" => Outcome::Result(Json::object([
            ("isError", Json::Bool(true)),
            ("content", Json::Array(vec![text_block("disk on fire")])),
        ])),
        "rpc_invalid_read" | "rpc_invalid_write" => Outcome::Error(-32602, "bad params"),
        "rpc_other" => Outcome::Error(-32000, "server trouble"),
        "sleep_read" | "sleep_write" => {
            thread::sleep(Duration::from_millis(u64::try_from(int(arguments, "ms")).unwrap_or(0)));
            text_result(&["slept"])
        }
        "die_read" | "die_write" => Outcome::Exit,
        "echo_delay" => {
            thread::sleep(Duration::from_millis(u64::try_from(int(arguments, "ms")).unwrap_or(0)));
            let order = fixture.order.fetch_add(1, Ordering::SeqCst) + 1;
            structured(Json::object([("tag", Json::string(string(arguments, "tag"))), ("order", Json::Int(order as i64))]))
        }
        "peak" => {
            let now = fixture.inflight.fetch_add(1, Ordering::SeqCst) + 1;
            fixture.peak.fetch_max(now, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(u64::try_from(int(arguments, "ms")).unwrap_or(0)));
            fixture.inflight.fetch_sub(1, Ordering::SeqCst);
            structured(Json::object([("peak", Json::Int(fixture.peak.load(Ordering::SeqCst) as i64))]))
        }
        "cancellations" => {
            thread::sleep(Duration::from_millis(100));
            structured(Json::object([("count", Json::Int(fixture.cancelled.load(Ordering::SeqCst) as i64))]))
        }
        "image_read" | "image_write" => Outcome::Result(Json::object([(
            "content",
            Json::Array(vec![Json::object([
                ("type", Json::string("image")),
                ("data", Json::string("AAAA")),
                ("mimeType", Json::string("image/png")),
            ])]),
        )])),
        "unstructured_read" | "unstructured_write" => text_result(&["only text"]),
        _ => Outcome::Error(-32602, "unknown tool"),
    }
}

fn serve_call(fixture: &Fixture, id: &Json, params: &Json) {
    let name = params.get("name").and_then(Json::as_str).unwrap_or("");
    let arguments = params.get("arguments").cloned().unwrap_or(Json::Object(Vec::new()));
    match call(fixture, name, &arguments) {
        Outcome::Result(result) => fixture.reply(id, result),
        Outcome::Error(code, message) => fixture.fail(id, code, message),
        Outcome::Exit => std::process::exit(3),
    }
}

fn page(tools: &[Json], size: usize, cursor: Option<&str>) -> Json {
    let start = cursor.and_then(|text| text.strip_prefix("page-")).and_then(|n| n.parse::<usize>().ok()).unwrap_or(0);
    let end = (start + size).min(tools.len());
    let mut fields = vec![("tools", Json::Array(tools.get(start..end).unwrap_or(&[]).to_vec()))];
    if end < tools.len() {
        fields.push(("nextCursor", Json::string(format!("page-{end}"))));
    }
    Json::object(fields)
}

fn main() -> ExitCode {
    let mut tools_file = None;
    let mut page_size = usize::MAX;
    let mut version = "2025-06-18".to_string();
    let mut crash_on_start = false;
    let mut garbage = false;
    let mut pid_file = None;
    let mut child_pid_file = None;
    let mut linger = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--page-size" => page_size = args.next().and_then(|n| n.parse().ok()).unwrap_or(1),
            "--protocol-version" => version = args.next().unwrap_or_default(),
            "--crash-on-start" => crash_on_start = true,
            "--garbage-after-initialized" => garbage = true,
            "--pid-file" => pid_file = args.next(),
            "--child-pid-file" => child_pid_file = args.next(),
            "--linger" => linger = true,
            file => tools_file = Some(file.to_string()),
        }
    }
    if let Some(path) = pid_file {
        let _ = std::fs::write(path, std::process::id().to_string());
    }
    if let Some(path) = child_pid_file
        && let Ok(child) = std::process::Command::new("/bin/sleep").arg("120").spawn()
    {
        let _ = std::fs::write(path, child.id().to_string());
    }
    if crash_on_start {
        return ExitCode::from(5);
    }
    let Some(tools_file) = tools_file else {
        eprintln!("mcp-fixture: a tools file is required");
        return ExitCode::from(64);
    };
    let tools = std::fs::read_to_string(&tools_file)
        .ok()
        .and_then(|text| parse(&text).ok())
        .and_then(|saved| saved.get("tools").and_then(Json::as_array).map(<[Json]>::to_vec))
        .unwrap_or_default();
    let fixture = Arc::new(Fixture {
        out: Mutex::new(std::io::stdout()),
        order: AtomicU64::new(0),
        inflight: AtomicU64::new(0),
        peak: AtomicU64::new(0),
        cancelled: AtomicU64::new(0),
        initialized: AtomicBool::new(false),
    });
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(message) = parse(&line) else { continue };
        let method = message.get("method").and_then(Json::as_str).unwrap_or("").to_string();
        let Some(id) = message.get("id").cloned() else {
            match method.as_str() {
                "notifications/initialized" => {
                    fixture.initialized.store(true, Ordering::SeqCst);
                    if garbage {
                        let mut out = fixture.out.lock().unwrap_or_else(|e| e.into_inner());
                        let _ = out.write_all(b"this is not json\n");
                        let _ = out.flush();
                    }
                }
                "notifications/cancelled" => {
                    fixture.cancelled.fetch_add(1, Ordering::SeqCst);
                }
                _ => {}
            }
            continue;
        };
        if method.is_empty() {
            continue;
        }
        let params = message.get("params").cloned().unwrap_or(Json::Null);
        match method.as_str() {
            "initialize" => fixture.reply(
                &id,
                Json::object([
                    ("protocolVersion", Json::string(version.as_str())),
                    ("capabilities", Json::object([("tools", Json::object(Vec::<(String, Json)>::new()))])),
                    ("serverInfo", Json::object([("name", Json::string("sx-fixture")), ("version", Json::string("0"))])),
                ]),
            ),
            "ping" => fixture.reply(&id, Json::object(Vec::<(String, Json)>::new())),
            _ if !fixture.initialized.load(Ordering::SeqCst) => fixture.fail(&id, -32002, "not initialized"),
            "tools/list" => {
                let cursor = params.get("cursor").and_then(Json::as_str);
                fixture.reply(&id, page(&tools, page_size, cursor));
            }
            "tools/call" => {
                let fixture = fixture.clone();
                thread::spawn(move || serve_call(&fixture, &id, &params));
            }
            _ => fixture.fail(&id, -32601, "method not found"),
        }
    }
    if linger {
        thread::sleep(Duration::from_secs(120));
    }
    ExitCode::SUCCESS
}
