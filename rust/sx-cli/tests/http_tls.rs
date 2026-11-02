use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::thread;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use sx_kernel::json::{Json, parse};

const CA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/tls/ca.pem");
const CERTIFICATE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/tls/server.pem");
const KEY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/tls/server.key");

fn tls_server() -> u16 {
    let certificates: Vec<CertificateDer<'static>> =
        CertificateDer::pem_file_iter(CERTIFICATE).expect("certificate file").collect::<Result<_, _>>().expect("certificates");
    let key = PrivateKeyDer::from_pem_file(KEY).expect("key");
    let provider = Arc::new(rustls_graviola::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .expect("versions")
        .with_no_client_auth()
        .with_single_cert(certificates, key)
        .expect("certificate and key");
    let config = Arc::new(config);
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("address").port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else {
                continue;
            };
            let config = config.clone();
            thread::spawn(move || {
                let Ok(connection) = ServerConnection::new(config) else {
                    return;
                };
                let mut tls = StreamOwned::new(connection, stream);
                let mut reader = BufReader::new(&mut tls);
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_err() {
                    return;
                }
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() || line.trim_end().is_empty() {
                        break;
                    }
                }
                let body = format!(r#"{{"line": {request_line:?}, "secure": true}}"#);
                let reply = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = tls.write_all(reply.as_bytes());
                tls.conn.send_close_notify();
                let _ = tls.flush();
                let mut rest = Vec::new();
                let _ = tls.read_to_end(&mut rest);
            });
        }
    });
    port
}

struct Workspace {
    base: PathBuf,
}

fn text(path: &Path) -> &str {
    path.to_str().expect("test paths are UTF-8")
}

impl Workspace {
    fn new(label: &str, port: u16) -> Workspace {
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("http-tls-{}-{label}", std::process::id()));
        if base.exists() {
            fs::remove_dir_all(&base).expect("old workspace removed");
        }
        fs::create_dir_all(&base).expect("workspace");
        let declarations = format!(
            r#"{{"packs": ["http"], "capabilities": [
  {{"id": "secure.lookup", "http": {{"method": "GET", "url": "https://localhost:{port}/things/{{id}}"}},
    "input": "Record {{id: Int}}", "output": "Record open {{secure: Bool}}", "effect": "read"}}]}}"#
        );
        let scope = format!(r#"{{"origins": ["https://localhost:{port}", "https://127.0.0.1:{port}"], "methods": ["GET"]}}"#);
        let grants = format!(r#"{{"secure.lookup": {scope}, "http.get": {scope}}}"#);
        fs::write(base.join("declarations.json"), declarations).expect("declarations");
        fs::write(base.join("grants.json"), grants).expect("grants");
        Workspace { base }
    }

    fn result(&self, source: &str, trusting: bool) -> Json {
        let program = self.base.join("program.sx");
        fs::write(&program, source).expect("program");
        let mut command = Command::new(env!("CARGO_BIN_EXE_sx"));
        command
            .args(["run", text(&program), "--format", "json", "--declarations"])
            .arg(self.base.join("declarations.json"))
            .arg("--grants")
            .arg(self.base.join("grants.json"));
        if trusting {
            command.env("SSL_CERT_FILE", CA);
        } else {
            command.env_remove("SSL_CERT_FILE").env_remove("SSL_CERT_DIR");
        }
        let output = command.output().expect("sx runs");
        let stdout = String::from_utf8(output.stdout).expect("utf-8");
        let outcome = parse(&stdout).unwrap_or_else(|e| panic!("{e}: {stdout} {}", String::from_utf8_lossy(&output.stderr)));
        assert_eq!(outcome.get("outcome").and_then(Json::as_str), Some("completed"), "{stdout}");
        outcome.get("result").cloned().expect("a result")
    }
}

#[test]
fn a_typed_endpoint_and_a_raw_get_reach_a_server_the_private_ca_signed() {
    let port = tls_server();
    let w = Workspace::new("trusted", port);
    let secure = w.result(r#"(call secure.lookup {:id 7})"#, true);
    assert_eq!(secure.get("secure").and_then(Json::as_bool), Some(true));
    let raw = w.result(
        &format!(r#"(let [r (call http.get {{:url "https://127.0.0.1:{port}/raw" :headers {{}}}})] (get r :status))"#),
        true,
    );
    assert_eq!(raw.as_i64(), Some(200));
}

#[test]
fn a_certificate_the_system_does_not_trust_is_a_failure() {
    let port = tls_server();
    let w = Workspace::new("untrusted", port);
    let outcome = w.result(r#"(attempt (call secure.lookup {:id 7}))"#, false);
    let error = outcome.get("err").expect("an error");
    assert_eq!(error.get("kind").and_then(Json::as_str), Some("Failed"), "{error:?}");
    let message = error.get("message").and_then(Json::as_str).expect("message");
    assert!(message.contains("TLS negotiation or certificate validation failed"), "{message}");
}
