mod support;

use std::sync::Arc;
use std::time::Duration;

use support::{Reply, Server, closed_port};
use sx_kernel::authority::{Grant, Grants, Screen, Verdict};
use sx_kernel::json::Json;
use sx_kernel::registry::Registry;
use sx_kernel::types::CallErrorKind;
use sx_kernel::values::{MapValue, RecordValue, Value};
use sx_packs::http::{self, GET, Limits, REQUEST};
use sx_packs::{Failure, pack};

const FAST: Limits = Limits {
    timeout: Duration::from_millis(600),
    connect_timeout: Duration::from_millis(600),
    max_body: 1024,
    max_redirects: 5,
};

fn scope_json(origins: &[&str], methods: &[&str]) -> Json {
    Json::object([
        ("origins", Json::Array(origins.iter().map(|o| Json::string(*o)).collect())),
        ("methods", Json::Array(methods.iter().map(|m| Json::string(*m)).collect())),
    ])
}

fn registry() -> Registry {
    let mut registry = Registry::new();
    registry.add_pack(pack("http").expect("the http pack")).expect("the http pack registers");
    registry
}

fn try_granted(origins: &[&str], methods: &[&str]) -> Result<Grants, String> {
    let scope = scope_json(origins, methods);
    let all = Json::object([(REQUEST, scope.clone()), (GET, scope)]);
    Grants::load(&registry(), &all).map_err(|e| e.to_string())
}

fn granted(origins: &[&str], methods: &[&str]) -> Grants {
    try_granted(origins, methods).expect("the grants load")
}

fn headers(pairs: &[(&str, &str)]) -> Value {
    Value::Map(MapValue::from_pairs(pairs.iter().map(|(n, v)| (Arc::from(*n), Value::string(v)))))
}

fn request_input(method: &str, url: &str, pairs: &[(&str, &str)], body: Option<&str>) -> Value {
    Value::Record(RecordValue::new(vec![
        (Arc::from("method"), Value::string(method)),
        (Arc::from("url"), Value::string(url)),
        (Arc::from("headers"), headers(pairs)),
        (Arc::from("body"), body.map_or(Value::None, |b| Value::some(Value::string(b)))),
    ]))
}

fn get_input(url: &str, pairs: &[(&str, &str)]) -> Value {
    Value::Record(RecordValue::new(vec![
        (Arc::from("url"), Value::string(url)),
        (Arc::from("headers"), headers(pairs)),
    ]))
}

fn scope_of(grants: &Grants, id: &str) -> Value {
    match grants.get(id) {
        Some(Grant::Scoped { scope, .. }) => scope.clone(),
        other => panic!("{id} is not granted with a scope: {other:?}"),
    }
}

fn verdict(grants: &Grants, id: &str, input: &Value) -> Verdict {
    match grants.screen(id, input) {
        Screen::Verdict(verdict) => verdict,
        other => panic!("{id} has no scope check: {other:?}"),
    }
}

fn reason(verdict: Verdict) -> String {
    match verdict {
        Verdict::Denied(reason) => reason,
        Verdict::Allowed => panic!("the request was allowed"),
    }
}

fn get(grants: &Grants, limits: &Limits, url: &str) -> Result<Value, Failure> {
    http::get_with(limits, &scope_of(grants, GET), &get_input(url, &[]))
}

fn request(
    grants: &Grants,
    limits: &Limits,
    method: &str,
    url: &str,
    pairs: &[(&str, &str)],
    body: Option<&str>,
) -> Result<Value, Failure> {
    http::request_with(limits, &scope_of(grants, REQUEST), &request_input(method, url, pairs, body))
}

fn field<'v>(value: &'v Value, name: &str) -> &'v Value {
    match value {
        Value::Record(record) => record.get(name).expect("the field exists"),
        other => panic!("not a record: {other:?}"),
    }
}

fn status(value: &Value) -> i64 {
    match field(value, "status") {
        Value::Int(n) => *n,
        other => panic!("not an Int: {other:?}"),
    }
}

fn body(value: &Value) -> String {
    match field(value, "body") {
        Value::String(s) => s.to_string(),
        other => panic!("not a String: {other:?}"),
    }
}

fn header(value: &Value, name: &str) -> Option<String> {
    match field(value, "headers") {
        Value::Map(map) => map.get(name).map(|v| match v {
            Value::String(s) => s.to_string(),
            other => panic!("not a String: {other:?}"),
        }),
        other => panic!("not a Map: {other:?}"),
    }
}

fn kind(result: Result<Value, Failure>) -> CallErrorKind {
    result.expect_err("the request fails").kind
}

#[test]
fn grants_normalize_origins_and_methods() {
    let grants = granted(
        &["HTTPS://API.Example.COM", "http://example.com:80", "https://bücher.example", "http://[::1]:8080", "http://127.0.0.1:9"],
        &["GET", "GET", "POST"],
    );
    let Some(Grant::Scoped { scope, .. }) = grants.get(GET) else {
        panic!("granted with a scope");
    };
    let origins = match field(scope, "origins") {
        Value::List(items) => items.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>(),
        other => panic!("not a list: {other:?}"),
    };
    assert_eq!(
        origins,
        [
            "String(\"https://api.example.com:443\")",
            "String(\"http://example.com:80\")",
            "String(\"https://xn--bcher-kva.example:443\")",
            "String(\"http://[::1]:8080\")",
            "String(\"http://127.0.0.1:9\")",
        ]
    );
    assert_eq!(field(scope, "methods"), &Value::list(vec![Value::string("GET"), Value::string("POST")]));
}

#[test]
fn grants_reject_malformed_scopes() {
    let cases: [(&[&str], &[&str], &str); 12] = [
        (&[], &["GET"], "at least one origin"),
        (&["http://a.example"], &[], "at least one method"),
        (&["ftp://a.example"], &["GET"], "scheme must be http or https"),
        (&["a.example"], &["GET"], "not an absolute http or https URL"),
        (&["https://a.example/"], &["GET"], "has a path"),
        (&["https://a.example/v1"], &["GET"], "has a path"),
        (&["https://user@a.example"], &["GET"], "userinfo"),
        (&["https://a.example."], &["GET"], "ends in a dot"),
        (&["https://a.example:0"], &["GET"], "port 0"),
        (&["https://a.example"], &["get"], "uppercase HTTP method token"),
        (&["https://a.example"], &["GE T"], "uppercase HTTP method token"),
        (&["https://a.example:"], &["GET"], "not a number"),
    ];
    for (origins, methods, expected) in cases {
        let error = try_granted(origins, methods).expect_err("the grant is refused");
        assert!(error.contains(expected), "{origins:?} {methods:?}: {error}");
    }
}

#[test]
fn urls_are_checked_by_normalized_origin_and_method() {
    let grants = granted(&["https://api.example.com", "https://bücher.example", "http://[::1]:8080"], &["GET"]);
    let allowed = [
        "https://api.example.com/x",
        "https://API.EXAMPLE.COM/x",
        "HTTPS://api.example.com:443/x?q=1#frag",
        "https://api.example.com",
        "https://api.example.com?q=1",
        "https://xn--bcher-kva.example/",
        "https://BÜCHER.example/",
        "http://[0:0:0:0:0:0:0:1]:8080/x",
    ];
    for url in allowed {
        assert_eq!(verdict(&grants, GET, &get_input(url, &[])), Verdict::Allowed, "{url}");
    }
    let denied = [
        ("https://api.example.com:8443/x", "outside every granted origin"),
        ("http://api.example.com/x", "outside every granted origin"),
        ("https://api.example.com:80/x", "outside every granted origin"),
        ("https://other.example.com/x", "outside every granted origin"),
        ("https://api.example.com.evil.example/x", "outside every granted origin"),
        ("https://evil.example/api.example.com", "outside every granted origin"),
        ("https://api.example.com./x", "ends in a dot"),
        ("https://user@api.example.com/x", "userinfo"),
        ("https://user:pw@api.example.com/x", "userinfo"),
        ("https://api.example.com@evil.example/x", "userinfo"),
        ("https://evil.example\\@api.example.com/x", "never holds unescaped"),
        ("https://api.example.com:443@evil.example/", "userinfo"),
        ("https://api.example.com%2eevil.example/", "percent escape"),
        ("https://api.example.com:/x", "not a number"),
        ("https://api.example.com:99999/x", "not between 1 and 65535"),
        ("https://api.example.com:0/x", "not between 1 and 65535"),
        ("https://api.example.com/a b", "never holds unescaped"),
        ("https://api.example.com/\u{e9}", "must be ASCII"),
        ("https://api.example.com/%zz", "not followed by two hexadecimal digits"),
        ("http://0x7f.1/", "dotted quad"),
        ("http://2130706433/", "dotted quad"),
        ("http://127.1/", "dotted quad"),
        ("ftp://api.example.com/x", "scheme must be http or https"),
        ("//api.example.com/x", "not an absolute"),
        ("/x", "not an absolute"),
        ("", "not an absolute"),
        ("https:///x", "no host"),
    ];
    for (url, expected) in denied {
        let why = reason(verdict(&grants, GET, &get_input(url, &[])));
        assert!(why.contains(expected), "{url}: {why}");
    }
}

#[test]
fn methods_are_matched_exactly() {
    let grants = granted(&["https://api.example.com"], &["GET", "POST"]);
    let url = "https://api.example.com/x";
    assert_eq!(verdict(&grants, REQUEST, &request_input("POST", url, &[], None)), Verdict::Allowed);
    for method in ["DELETE", "get", "Get", "PUT", ""] {
        let why = reason(verdict(&grants, REQUEST, &request_input(method, url, &[], None)));
        assert!(why.contains("not granted"), "{method}: {why}");
    }
    let read_only = granted(&["https://api.example.com"], &["POST"]);
    assert!(reason(verdict(&read_only, GET, &get_input(url, &[]))).contains("method GET is not granted"));
}

#[test]
fn a_denied_request_never_opens_a_socket() {
    let server = Server::start(|_| Reply::ok("hello"));
    let other = Server::start(|_| Reply::ok("other"));
    let grants = granted(&[&server.origin()], &["GET"]);
    let denied = [
        format!("{}/x", other.origin()),
        format!("http://localhost:{}/x", server.port()),
        format!("http://127.0.0.1:{}/x", server.port() + 1),
        format!("https://127.0.0.1:{}/x", server.port()),
        format!("http://user@127.0.0.1:{}/x", server.port()),
        format!("http://127.0.0.1.:{}/x", server.port()),
        format!("http://127.0.0.1:{}\\@evil.example/x", server.port()),
    ];
    for url in &denied {
        assert_eq!(kind(get(&grants, &FAST, url)), CallErrorKind::PermissionDenied, "{url}");
    }
    let delete = request(&grants, &FAST, "POST", &format!("{}/x", server.origin()), &[], None);
    assert_eq!(kind(delete), CallErrorKind::PermissionDenied);
    assert_eq!(server.connections(), 0);
    assert_eq!(other.connections(), 0);
    assert!(server.requests().is_empty() && other.requests().is_empty());
    assert_eq!(status(&get(&grants, &FAST, &format!("{}/x", server.origin())).expect("allowed")), 200);
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn get_returns_status_headers_and_body() {
    let server = Server::start(|seen| {
        Reply::with(
            200,
            &[("X-Thing", "a"), ("x-thing", "b"), ("Content-Type", "text/plain")],
            &format!("{} {}", seen.method, seen.target),
        )
    });
    let grants = granted(&[&server.origin()], &["GET", "POST"]);
    let reply = get(&grants, &FAST, &format!("{}/a/b?x=1&y=%20#frag", server.origin())).expect("the request succeeds");
    assert_eq!(status(&reply), 200);
    assert_eq!(body(&reply), "GET /a/b?x=1&y=%20");
    assert_eq!(header(&reply, "x-thing").as_deref(), Some("a, b"));
    assert_eq!(header(&reply, "content-type").as_deref(), Some("text/plain"));
    assert_eq!(header(&reply, "X-Thing"), None);
    let seen = &server.requests()[0];
    assert!(seen.header("user-agent").is_some_and(|ua| ua.starts_with("sx-packs/")));
    assert_eq!(seen.header("host"), Some(format!("127.0.0.1:{}", server.port()).as_str()));
}

#[test]
fn request_sends_the_method_headers_and_body_it_is_given() {
    let server = Server::start(|seen| Reply::ok(&format!("{}|{}", seen.method, seen.text())));
    let grants = granted(&[&server.origin()], &["POST", "PUT", "PATCH", "DELETE", "HEAD"]);
    let url = format!("{}/r", server.origin());
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        let reply = request(&grants, &FAST, method, &url, &[("X-Token", "t"), ("Content-Type", "text/x")], Some("payload"))
            .expect("the request succeeds");
        assert_eq!(body(&reply), format!("{method}|payload"));
    }
    let seen = server.requests();
    assert_eq!(seen[0].header("x-token"), Some("t"));
    assert_eq!(seen[0].header("content-type"), Some("text/x"));
    assert_eq!(seen[0].header("content-length"), Some("7"));
    let head = request(&grants, &FAST, "HEAD", &url, &[], None).expect("head");
    assert_eq!(body(&head), "");
    assert_eq!(status(&head), 200);
    let user_agent = request(&grants, &FAST, "POST", &url, &[("User-Agent", "mine")], None).expect("ua");
    assert_eq!(status(&user_agent), 200);
    assert_eq!(server.requests().last().expect("seen").header("user-agent"), Some("mine"));
}

#[test]
fn any_status_is_a_value_for_raw_requests() {
    let server = Server::start(|seen| match seen.target.as_str() {
        "/missing" => Reply::status(404, "nope"),
        "/limited" => Reply::status(429, "slow down"),
        "/broken" => Reply::status(503, "down"),
        _ => Reply::status(418, "teapot"),
    });
    let grants = granted(&[&server.origin()], &["GET"]);
    for (path, expected) in [("missing", 404), ("limited", 429), ("broken", 503), ("tea", 418)] {
        let reply = get(&grants, &FAST, &format!("{}/{path}", server.origin())).expect("a status is a value");
        assert_eq!(status(&reply), expected);
    }
}

#[test]
fn headers_that_frame_the_message_are_refused_before_connecting() {
    let server = Server::start(|_| Reply::ok("x"));
    let grants = granted(&[&server.origin()], &["GET"]);
    let url = format!("{}/x", server.origin());
    let scope = scope_of(&grants, GET);
    for pairs in [
        vec![("Host", "evil.example")],
        vec![("content-length", "5")],
        vec![("Transfer-Encoding", "chunked")],
        vec![("Connection", "upgrade")],
        vec![("bad name", "v")],
        vec![("x", "line\nbreak")],
        vec![("X-A", "1"), ("x-a", "2")],
    ] {
        let failure = http::get_with(&FAST, &scope, &get_input(&url, &pairs)).expect_err("refused");
        assert_eq!(failure.kind, CallErrorKind::Invalid, "{pairs:?}: {failure}");
    }
    assert_eq!(server.connections(), 0);
}

#[test]
fn a_body_that_is_not_utf8_is_invalid_for_a_safe_method_and_failed_for_the_rest() {
    let server = Server::start(|_| Reply::Http {
        status: 200,
        headers: Vec::new(),
        body: vec![0xff, 0xfe, 0x41],
        delay: Duration::ZERO,
    });
    let grants = granted(&[&server.origin()], &["GET", "POST"]);
    let url = format!("{}/bin", server.origin());
    assert_eq!(kind(get(&grants, &FAST, &url)), CallErrorKind::Invalid);
    assert_eq!(kind(request(&grants, &FAST, "POST", &url, &[], None)), CallErrorKind::Failed);
}

#[test]
fn redirects_in_scope_are_followed() {
    let server = Server::start(|seen| match seen.target.as_str() {
        "/start" => Reply::redirect(302, "/dir/one"),
        "/dir/one" => Reply::redirect(301, "two?a=1"),
        "/dir/two?a=1" => Reply::redirect(308, "../end#frag"),
        "/end" => Reply::ok("arrived"),
        other => Reply::status(404, other),
    });
    let grants = granted(&[&server.origin()], &["GET"]);
    let reply = get(&grants, &FAST, &format!("{}/start", server.origin())).expect("followed");
    assert_eq!(body(&reply), "arrived");
    let targets: Vec<String> = server.requests().into_iter().map(|s| s.target).collect();
    assert_eq!(targets, ["/start", "/dir/one", "/dir/two?a=1", "/end"]);
}

#[test]
fn redirects_stop_after_five_and_return_the_last_response() {
    let server = Server::start(|seen| {
        let hop: usize = seen.target.trim_start_matches("/hop").parse().unwrap_or(0);
        Reply::redirect(302, &format!("/hop{}", hop + 1))
    });
    let grants = granted(&[&server.origin()], &["GET"]);
    let reply = get(&grants, &FAST, &format!("{}/hop0", server.origin())).expect("returned as is");
    assert_eq!(status(&reply), 302);
    assert_eq!(header(&reply, "location").as_deref(), Some("/hop6"));
    assert_eq!(server.requests().len(), 6);
}

#[test]
fn a_redirect_out_of_scope_is_returned_and_never_requested() {
    let outside = Server::start(|_| Reply::ok("secret"));
    let outside_url = format!("{}/stolen", outside.origin());
    let target = outside_url.clone();
    let server = Server::start(move |seen| match seen.target.as_str() {
        "/to-port" => Reply::redirect(302, &target),
        "/to-scheme" => Reply::redirect(302, "https://127.0.0.1/x"),
        "/to-userinfo" => Reply::redirect(302, "http://user@127.0.0.1/x"),
        "/to-garbage" => Reply::redirect(302, "http://[nope/x"),
        "/to-ftp" => Reply::redirect(302, "ftp://127.0.0.1/x"),
        "/two-locations" => Reply::with(302, &[("location", "/a"), ("location", "/b")], ""),
        "/no-location" => Reply::status(302, ""),
        _ => Reply::ok("inside"),
    });
    let grants = granted(&[&server.origin()], &["GET", "POST"]);
    for path in ["to-port", "to-scheme", "to-userinfo", "to-garbage", "to-ftp", "two-locations", "no-location"] {
        let reply = get(&grants, &FAST, &format!("{}/{path}", server.origin())).expect("returned as is");
        assert_eq!(status(&reply), 302, "{path}");
    }
    let reply = get(&grants, &FAST, &format!("{}/to-port", server.origin())).expect("returned as is");
    assert_eq!(header(&reply, "location").as_deref(), Some(outside_url.as_str()));
    assert_eq!(outside.connections(), 0);
}

#[test]
fn a_redirect_needs_its_method_in_scope() {
    let server = Server::start(|seen| match seen.target.as_str() {
        "/see-other" => Reply::redirect(303, "/landing"),
        _ => Reply::ok(&format!("{} {}", seen.method, seen.text())),
    });
    let grants = granted(&[&server.origin()], &["POST"]);
    let reply = request(&grants, &FAST, "POST", &format!("{}/see-other", server.origin()), &[], Some("data"))
        .expect("returned as is");
    assert_eq!(status(&reply), 303);
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn redirect_methods_and_bodies_follow_the_status() {
    let server = Server::start(|seen| match seen.target.as_str() {
        "/303" => Reply::redirect(303, "/landing"),
        "/307" => Reply::redirect(307, "/landing"),
        "/308" => Reply::redirect(308, "/landing"),
        "/302" => Reply::redirect(302, "/landing"),
        _ => Reply::ok(&format!("{} {}", seen.method, seen.text())),
    });
    let grants = granted(&[&server.origin()], &["GET", "POST"]);
    let post = |path: &str| {
        request(&grants, &FAST, "POST", &format!("{}/{path}", server.origin()), &[("content-type", "text/x")], Some("data"))
            .expect("the request succeeds")
    };
    assert_eq!(body(&post("303")), "GET ");
    assert_eq!(body(&post("307")), "POST data");
    assert_eq!(body(&post("308")), "POST data");
    let unfollowed = post("302");
    assert_eq!(status(&unfollowed), 302);
    let landing = server.requests().into_iter().filter(|s| s.target == "/landing").collect::<Vec<_>>();
    assert_eq!(landing.len(), 3);
    assert_eq!(landing[0].header("content-type"), None);
    assert_eq!(landing[1].header("content-type"), Some("text/x"));
}

#[test]
fn a_cross_origin_redirect_carries_no_caller_headers() {
    let target = Server::start(|seen| Reply::ok(seen.header("authorization").unwrap_or("none")));
    let location = format!("{}/landing", target.origin());
    let source = Server::start(move |_| Reply::redirect(302, &location));
    let grants = granted(&[&source.origin(), &target.origin()], &["GET"]);
    let reply = http::get_with(
        &FAST,
        &scope_of(&grants, GET),
        &get_input(&format!("{}/start", source.origin()), &[("Authorization", "Bearer secret")]),
    )
    .expect("followed");
    assert_eq!(body(&reply), "none");
    assert_eq!(source.requests()[0].header("authorization"), Some("Bearer secret"));
    assert_eq!(target.requests().len(), 1);
}

#[test]
fn a_same_origin_redirect_keeps_caller_headers() {
    let server = Server::start(|seen| match seen.target.as_str() {
        "/start" => Reply::redirect(302, "/landing"),
        _ => Reply::ok(seen.header("authorization").unwrap_or("none")),
    });
    let grants = granted(&[&server.origin()], &["GET"]);
    let reply = http::get_with(
        &FAST,
        &scope_of(&grants, GET),
        &get_input(&format!("{}/start", server.origin()), &[("Authorization", "Bearer secret")]),
    )
    .expect("followed");
    assert_eq!(body(&reply), "Bearer secret");
}

#[test]
fn an_oversized_body_fails() {
    let server = Server::start(|seen| match seen.target.as_str() {
        "/declared" => Reply::Http { status: 200, headers: Vec::new(), body: vec![b'a'; 4096], delay: Duration::ZERO },
        "/streamed" => {
            let mut raw = b"HTTP/1.1 200 OK\r\nconnection: close\r\n\r\n".to_vec();
            raw.extend(std::iter::repeat_n(b'a', 4096));
            Reply::Raw(raw)
        }
        "/exact" => Reply::Http { status: 200, headers: Vec::new(), body: vec![b'a'; 1024], delay: Duration::ZERO },
        _ => Reply::status(404, ""),
    });
    let grants = granted(&[&server.origin()], &["GET"]);
    for path in ["declared", "streamed"] {
        let failure = get(&grants, &FAST, &format!("{}/{path}", server.origin())).expect_err("too large");
        assert_eq!(failure.kind, CallErrorKind::Failed, "{path}");
        assert!(failure.message.contains("larger than 1024 bytes"), "{}", failure.message);
    }
    let exact = get(&grants, &FAST, &format!("{}/exact", server.origin())).expect("the cap itself fits");
    assert_eq!(body(&exact).len(), 1024);
}

#[test]
fn a_truncated_body_fails() {
    let server = Server::start(|_| {
        Reply::Raw(b"HTTP/1.1 200 OK\r\ncontent-length: 100\r\nconnection: close\r\n\r\nshort".to_vec())
    });
    let grants = granted(&[&server.origin()], &["GET"]);
    let failure = get(&grants, &FAST, &format!("{}/x", server.origin())).expect_err("truncated");
    assert_eq!(failure.kind, CallErrorKind::Failed);
}

#[test]
fn a_slow_response_head_times_out() {
    let server = Server::start(|_| Reply::slow(Duration::from_secs(3)));
    let grants = granted(&[&server.origin()], &["GET"]);
    let started = std::time::Instant::now();
    assert_eq!(kind(get(&grants, &FAST, &format!("{}/x", server.origin()))), CallErrorKind::Timeout);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn a_stalled_body_times_out() {
    let server = Server::start(|_| {
        Reply::Stalled {
            sent: b"HTTP/1.1 200 OK\r\ncontent-length: 100\r\nconnection: close\r\n\r\nsome".to_vec(),
            stall: Duration::from_secs(3),
        }
    });
    let grants = granted(&[&server.origin()], &["GET"]);
    let started = std::time::Instant::now();
    assert_eq!(kind(get(&grants, &FAST, &format!("{}/x", server.origin()))), CallErrorKind::Timeout);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn the_timeout_covers_every_redirect() {
    let server = Server::start(|seen| {
        let hop: usize = seen.target.trim_start_matches("/hop").parse().unwrap_or(0);
        std::thread::sleep(Duration::from_millis(250));
        Reply::redirect(302, &format!("/hop{}", hop + 1))
    });
    let grants = granted(&[&server.origin()], &["GET"]);
    let started = std::time::Instant::now();
    assert_eq!(kind(get(&grants, &FAST, &format!("{}/hop0", server.origin()))), CallErrorKind::Timeout);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn an_unreachable_server_is_unavailable() {
    let port = closed_port();
    let origin = format!("http://127.0.0.1:{port}");
    let grants = granted(&[&origin, "http://nonexistent.invalid"], &["GET"]);
    assert_eq!(kind(get(&grants, &FAST, &format!("{origin}/x"))), CallErrorKind::Unavailable);
    assert_eq!(kind(get(&grants, &FAST, "http://nonexistent.invalid/x")), CallErrorKind::Unavailable);
}

#[test]
fn a_server_that_does_not_speak_tls_is_a_failure() {
    let server = Server::start(|_| Reply::ok("plain"));
    let origin = format!("https://127.0.0.1:{}", server.port());
    let grants = granted(&[&origin], &["GET"]);
    assert_eq!(kind(get(&grants, &FAST, &format!("{origin}/x"))), CallErrorKind::Failed);
}

#[test]
fn a_garbled_response_is_a_failure() {
    let server = Server::start(|_| Reply::Raw(b"not http at all\r\n\r\n".to_vec()));
    let grants = granted(&[&server.origin()], &["GET"]);
    assert_eq!(kind(get(&grants, &FAST, &format!("{}/x", server.origin()))), CallErrorKind::Failed);
}
