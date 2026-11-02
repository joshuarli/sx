use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use sx_kernel::json::{Json, Layout, dumps, parse};

use crate::process::Group;

pub const PROTOCOL_VERSION: &str = "2025-06-18";
pub const ACCEPTED_VERSIONS: [&str; 3] = [PROTOCOL_VERSION, "2025-03-26", "2024-11-05"];
pub const LINE_LIMIT: usize = 16 << 20;
pub const STANDARD_TIMEOUT: Duration = Duration::from_secs(60);

const SHUTDOWN_GRACE: Duration = Duration::from_millis(300);
const PAGE_LIMIT: usize = 10_000;
const METHOD_NOT_FOUND: i64 = -32601;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RpcError {
    Remote { code: i64, message: String },
    Timeout,
    Closed(String),
}

impl fmt::Display for RpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RpcError::Remote { code, message } => write!(f, "the server answered with error {code}: {message}"),
            RpcError::Timeout => f.write_str("the server did not answer in time"),
            RpcError::Closed(reason) => write!(f, "the server connection closed: {reason}"),
        }
    }
}

impl std::error::Error for RpcError {}

type Reply = Result<Json, RpcError>;

#[derive(Default)]
struct State {
    pending: HashMap<u64, Sender<Reply>>,
    closed: Option<String>,
}

struct Shared {
    state: Mutex<State>,
    writer: Mutex<Option<ChildStdin>>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    fn send(&self, message: &Json) -> Result<(), String> {
        let mut line = dumps(message, Layout::Spaced).map_err(|e| e.0)?;
        line.push('\n');
        let mut writer = locked(&self.writer);
        let Some(pipe) = writer.as_mut() else {
            return Err("the server's input is closed".to_string());
        };
        pipe.write_all(line.as_bytes()).and_then(|()| pipe.flush()).map_err(|e| format!("cannot write to the server: {e}"))
    }

    fn close(&self, reason: String) {
        let mut state = locked(&self.state);
        if state.closed.is_none() {
            state.closed = Some(reason);
        }
        state.pending.clear();
    }

    fn reason(&self) -> String {
        locked(&self.state).closed.clone().unwrap_or_else(|| "the server stopped answering".to_string())
    }
}

fn read_line(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(if line.is_empty() { None } else { Some(line) });
        }
        let end = available.iter().position(|byte| *byte == b'\n');
        let (taken, consumed) = match end {
            Some(at) => (available.get(..at).unwrap_or(&[]), at + 1),
            None => (available, available.len()),
        };
        if line.len() + taken.len() > LINE_LIMIT {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("a line is longer than {LINE_LIMIT} bytes")));
        }
        line.extend_from_slice(taken);
        reader.consume(consumed);
        if end.is_some() {
            return Ok(Some(line));
        }
    }
}

fn error_of(message: &Json) -> Option<RpcError> {
    let error = message.get("error")?;
    let code = error.get("code").and_then(Json::as_i64).unwrap_or(0);
    let text = error.get("message").and_then(Json::as_str).unwrap_or("no message");
    Some(RpcError::Remote { code, message: text.to_string() })
}

fn answer_server_request(shared: &Shared, id: &Json, method: &str) {
    let mut reply = vec![("jsonrpc", Json::string("2.0")), ("id", id.clone())];
    if method == "ping" {
        reply.push(("result", Json::object(Vec::<(String, Json)>::new())));
    } else {
        reply.push((
            "error",
            Json::object([
                ("code", Json::Int(METHOD_NOT_FOUND)),
                ("message", Json::string(format!("sx does not implement {method}"))),
            ]),
        ));
    }
    let _ = shared.send(&Json::object(reply));
}

fn dispatch(shared: &Shared, message: &Json) {
    if let Some(method) = message.get("method").and_then(Json::as_str) {
        if let Some(id) = message.get("id") {
            answer_server_request(shared, id, method);
        }
        return;
    }
    let Some(id) = message.get("id").and_then(Json::as_i64).and_then(|id| u64::try_from(id).ok()) else {
        return;
    };
    let reply = match (error_of(message), message.get("result")) {
        (Some(error), _) => Err(error),
        (None, Some(result)) => Ok(result.clone()),
        (None, None) => return,
    };
    if let Some(waiting) = locked(&shared.state).pending.remove(&id) {
        let _ = waiting.send(reply);
    }
}

fn pump(shared: &Shared, stdout: ChildStdout) {
    let mut reader = BufReader::new(stdout);
    loop {
        let line = match read_line(&mut reader) {
            Ok(Some(line)) => line,
            Ok(None) => return shared.close("the server closed its output or exited".to_string()),
            Err(error) => return shared.close(format!("cannot read the server's output: {error}")),
        };
        let Ok(text) = String::from_utf8(line) else {
            return shared.close("the server wrote a line that is not UTF-8".to_string());
        };
        if text.trim().is_empty() {
            continue;
        }
        match parse(&text) {
            Ok(message) => dispatch(shared, &message),
            Err(error) => return shared.close(format!("the server wrote a line that is not JSON: {error}")),
        }
    }
}

pub struct Connection {
    shared: Arc<Shared>,
    next_id: AtomicU64,
    child: Mutex<Child>,
    group: Group,
    version: String,
}

fn request_message(id: u64, method: &str, params: Json) -> Json {
    Json::object([
        ("jsonrpc", Json::string("2.0")),
        ("id", Json::Int(i64::try_from(id).unwrap_or(i64::MAX))),
        ("method", Json::string(method)),
        ("params", params),
    ])
}

fn notification(method: &str, params: Json) -> Json {
    Json::object([("jsonrpc", Json::string("2.0")), ("method", Json::string(method)), ("params", params)])
}

impl Connection {
    pub fn open(command: &[String], timeout: Duration) -> Result<Connection, String> {
        let Some((program, args)) = command.split_first() else {
            return Err("no server command".to_string());
        };
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .process_group(0)
            .spawn()
            .map_err(|e| format!("cannot start {program}: {e}"))?;
        let group = Group::of(child.id());
        let (stdin, stdout) = (child.stdin.take(), child.stdout.take());
        let (Some(group), Some(stdin), Some(stdout)) = (group, stdin, stdout) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("cannot attach to the pipes of {program}"));
        };
        let shared = Arc::new(Shared { state: Mutex::new(State::default()), writer: Mutex::new(Some(stdin)) });
        let reading = shared.clone();
        let started = thread::Builder::new().name("mcp-reader".to_string()).spawn(move || pump(&reading, stdout));
        let mut connection =
            Connection { shared, next_id: AtomicU64::new(0), child: Mutex::new(child), group, version: String::new() };
        started.map_err(|e| format!("cannot start the reader thread: {e}"))?;
        connection.handshake(timeout)?;
        Ok(connection)
    }

    fn handshake(&mut self, timeout: Duration) -> Result<(), String> {
        let params = Json::object([
            ("protocolVersion", Json::string(PROTOCOL_VERSION)),
            ("capabilities", Json::object(Vec::<(String, Json)>::new())),
            (
                "clientInfo",
                Json::object([("name", Json::string("sx")), ("version", Json::string(env!("CARGO_PKG_VERSION")))]),
            ),
        ]);
        let result = self.request("initialize", params, timeout).map_err(|e| format!("initialize failed: {e}"))?;
        let version = result.get("protocolVersion").and_then(Json::as_str).unwrap_or("");
        if !ACCEPTED_VERSIONS.contains(&version) {
            return Err(format!(
                "the server speaks protocol version {version:?}, and sx speaks {}",
                ACCEPTED_VERSIONS.join(", ")
            ));
        }
        self.version = version.to_string();
        self.shared
            .send(&notification("notifications/initialized", Json::object(Vec::<(String, Json)>::new())))
            .map_err(|e| format!("initialized failed: {e}"))
    }

    pub fn protocol_version(&self) -> &str {
        &self.version
    }

    pub fn request(&self, method: &str, params: Json, timeout: Duration) -> Result<Json, RpcError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (sender, receiver) = mpsc::channel();
        {
            let mut state = locked(&self.shared.state);
            if let Some(reason) = &state.closed {
                return Err(RpcError::Closed(reason.clone()));
            }
            state.pending.insert(id, sender);
        }
        if let Err(reason) = self.shared.send(&request_message(id, method, params)) {
            locked(&self.shared.state).pending.remove(&id);
            return Err(RpcError::Closed(reason));
        }
        match receiver.recv_timeout(timeout) {
            Ok(reply) => reply,
            Err(RecvTimeoutError::Timeout) => {
                locked(&self.shared.state).pending.remove(&id);
                let cancel = Json::object([
                    ("requestId", Json::Int(i64::try_from(id).unwrap_or(i64::MAX))),
                    ("reason", Json::string("sx stopped waiting")),
                ]);
                let _ = self.shared.send(&notification("notifications/cancelled", cancel));
                Err(RpcError::Timeout)
            }
            Err(RecvTimeoutError::Disconnected) => Err(RpcError::Closed(self.shared.reason())),
        }
    }

    pub fn list_tools(&self, timeout: Duration) -> Result<Vec<Json>, String> {
        let mut tools = Vec::new();
        let mut cursors: BTreeSet<String> = BTreeSet::new();
        let mut cursor: Option<String> = None;
        for _ in 0..PAGE_LIMIT {
            let params = match &cursor {
                None => Json::object(Vec::<(String, Json)>::new()),
                Some(cursor) => Json::object([("cursor", Json::string(cursor.as_str()))]),
            };
            let page = self.request("tools/list", params, timeout).map_err(|e| format!("tools/list failed: {e}"))?;
            let Some(listed) = page.get("tools").and_then(Json::as_array) else {
                return Err("tools/list answered without a tools list".to_string());
            };
            tools.extend(listed.iter().cloned());
            match page.get("nextCursor") {
                None | Some(Json::Null) => return Ok(tools),
                Some(Json::String(next)) if cursors.insert(next.clone()) => cursor = Some(next.clone()),
                Some(Json::String(next)) => return Err(format!("tools/list repeated the cursor {next:?}")),
                Some(_) => return Err("tools/list answered with a nextCursor that is not a string".to_string()),
            }
        }
        Err(format!("tools/list went past {PAGE_LIMIT} pages"))
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        locked(&self.shared.writer).take();
        let mut child = locked(&self.child);
        let until = Instant::now() + SHUTDOWN_GRACE;
        while Instant::now() < until && matches!(child.try_wait(), Ok(None)) {
            thread::sleep(Duration::from_millis(5));
        }
        self.group.kill();
        let _ = child.wait();
        self.shared.close("the connection was shut down".to_string());
    }
}
