#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Seen {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    pub fn text(&self) -> &str {
        std::str::from_utf8(&self.body).expect("the body is UTF-8")
    }
}

pub enum Reply {
    Http { status: u16, headers: Vec<(String, String)>, body: Vec<u8>, delay: Duration },
    Raw(Vec<u8>),
    Stalled { sent: Vec<u8>, stall: Duration },
}

impl Reply {
    pub fn ok(body: &str) -> Reply {
        Reply::status(200, body)
    }

    pub fn json(body: &str) -> Reply {
        Reply::Http {
            status: 200,
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            body: body.as_bytes().to_vec(),
            delay: Duration::ZERO,
        }
    }

    pub fn status(status: u16, body: &str) -> Reply {
        Reply::Http { status, headers: Vec::new(), body: body.as_bytes().to_vec(), delay: Duration::ZERO }
    }

    pub fn with(status: u16, headers: &[(&str, &str)], body: &str) -> Reply {
        Reply::Http {
            status,
            headers: headers.iter().map(|(n, v)| ((*n).to_string(), (*v).to_string())).collect(),
            body: body.as_bytes().to_vec(),
            delay: Duration::ZERO,
        }
    }

    pub fn redirect(status: u16, location: &str) -> Reply {
        Reply::with(status, &[("location", location)], "moved")
    }

    pub fn slow(delay: Duration) -> Reply {
        Reply::Http { status: 200, headers: Vec::new(), body: b"late".to_vec(), delay }
    }
}

type Handler = dyn Fn(&Seen) -> Reply + Send + Sync;

pub struct Server {
    address: SocketAddr,
    seen: Arc<Mutex<Vec<Seen>>>,
    connections: Arc<AtomicUsize>,
    stopping: Arc<AtomicBool>,
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Status",
    }
}

fn read_request(stream: &TcpStream) -> Option<Seen> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).ok()?;
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        let (name, value) = header.split_once(':')?;
        headers.push((name.to_string(), value.trim().to_string()));
    }
    let length = headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).ok()?;
    Some(Seen { method, target, headers, body })
}

fn serve(mut stream: TcpStream, handler: &Handler, seen: &Mutex<Vec<Seen>>) {
    let Some(request) = read_request(&stream) else {
        return;
    };
    seen.lock().expect("seen").push(request.clone());
    let head_only = request.method == "HEAD";
    match handler(&request) {
        Reply::Raw(bytes) => {
            let _ = stream.write_all(&bytes);
        }
        Reply::Stalled { sent, stall } => {
            let _ = stream.write_all(&sent);
            let _ = stream.flush();
            thread::sleep(stall);
        }
        Reply::Http { status, headers, body, delay } => {
            thread::sleep(delay);
            let mut out = format!("HTTP/1.1 {status} {}\r\n", reason(status));
            for (name, value) in &headers {
                out.push_str(&format!("{name}: {value}\r\n"));
            }
            if !headers.iter().any(|(n, _)| n.eq_ignore_ascii_case("content-length")) {
                out.push_str(&format!("content-length: {}\r\n", body.len()));
            }
            out.push_str("connection: close\r\n\r\n");
            let mut bytes = out.into_bytes();
            if !head_only {
                bytes.extend_from_slice(&body);
            }
            let _ = stream.write_all(&bytes);
        }
    }
}

impl Server {
    pub fn start(handler: impl Fn(&Seen) -> Reply + Send + Sync + 'static) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let connections = Arc::new(AtomicUsize::new(0));
        let stopping = Arc::new(AtomicBool::new(false));
        let handler: Arc<Handler> = Arc::new(handler);
        let (seen_in, connections_in, stopping_in) = (seen.clone(), connections.clone(), stopping.clone());
        thread::spawn(move || {
            for stream in listener.incoming() {
                if stopping_in.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else {
                    continue;
                };
                connections_in.fetch_add(1, Ordering::SeqCst);
                let (handler, seen) = (handler.clone(), seen_in.clone());
                thread::spawn(move || serve(stream, handler.as_ref(), &seen));
            }
        });
        Server { address, seen, connections, stopping }
    }

    pub fn port(&self) -> u16 {
        self.address.port()
    }

    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.address.port())
    }

    pub fn requests(&self) -> Vec<Seen> {
        self.seen.lock().expect("seen").clone()
    }

    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
    }
}

pub fn closed_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.local_addr().expect("address").port()
}
