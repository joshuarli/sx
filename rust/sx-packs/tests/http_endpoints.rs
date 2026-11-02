mod support;

use std::time::Duration;

use support::{Reply, Server, closed_port};
use sx_kernel::authority::{Grant, Grants, Screen, Verdict};
use sx_kernel::json::{Json, parse};
use sx_kernel::registry::{Registry, load_declarations_with};
use sx_kernel::types::CallErrorKind;
use sx_kernel::values::{Value, decode};
use sx_packs::Failure;
use sx_packs::http::{Endpoints, EndpointLoader, Limits};

const FAST: Limits = Limits {
    timeout: Duration::from_millis(600),
    connect_timeout: Duration::from_millis(600),
    max_body: 1024,
    max_redirects: 5,
};

struct World {
    registry: Registry,
    endpoints: Endpoints,
}

fn declare(declarations: &str) -> Result<World, String> {
    let json = parse(declarations).expect("the declarations are JSON");
    let mut registry = Registry::new();
    let mut loader = EndpointLoader::new();
    load_declarations_with(&mut registry, &json, &mut loader).map_err(|e| e.to_string())?;
    Ok(World { registry, endpoints: loader.finish() })
}

fn endpoint(origin: &str, method: &str, template: &str, query: &str, input: &str, output: &str, effect: &str) -> String {
    format!(
        r#"{{"capabilities": [{{"id": "svc.call", "http": {{"method": "{method}", "url": "{origin}{template}", "query": {query}, "headers": {{"accept": "application/json"}}}}, "input": "{input}", "output": "{output}", "effect": "{effect}"}}]}}"#
    )
}

fn scope(origins: &[&str], methods: &[&str]) -> Json {
    Json::object([
        ("origins", Json::Array(origins.iter().map(|o| Json::string(*o)).collect())),
        ("methods", Json::Array(methods.iter().map(|m| Json::string(*m)).collect())),
    ])
}

fn grant(world: &World, origins: &[&str], methods: &[&str]) -> Grants {
    Grants::load(&world.registry, &Json::object([("svc.call", scope(origins, methods))])).expect("the grant loads")
}

fn input(world: &World, json: &str) -> Value {
    let capability = world.registry.capability("svc.call").expect("declared");
    decode(&parse(json).expect("JSON"), &capability.input, "$").expect("the input decodes")
}

fn call(world: &World, grants: &Grants, json: &str) -> Result<Value, Failure> {
    let scope = match grants.get("svc.call") {
        Some(Grant::Scoped { scope, .. }) => scope.clone(),
        other => panic!("not granted with a scope: {other:?}"),
    };
    world.endpoints.get("svc.call").expect("bound").perform_with(&FAST, &scope, &input(world, json))
}

fn verdict(world: &World, grants: &Grants, json: &str) -> Verdict {
    match grants.screen("svc.call", &input(world, json)) {
        Screen::Verdict(verdict) => verdict,
        other => panic!("no scope check: {other:?}"),
    }
}

fn shown(value: &Value) -> String {
    sx_kernel::values::show_value(value)
}

const REPO_INPUT: &str = "Record {owner: String, name: String}";

fn repo(origin: &str) -> World {
    declare(&endpoint(
        origin,
        "GET",
        "/repos/{owner}/{name}",
        "[]",
        REPO_INPUT,
        "Record open {full_name: String, stargazers_count: Int}",
        "read",
    ))
    .expect("the endpoint declares")
}

#[test]
fn an_endpoint_registers_as_a_scoped_capability() {
    let world = repo("https://api.example.com");
    let capability = world.registry.capability("svc.call").expect("registered");
    assert!(capability.scope.is_some());
    assert_eq!(capability.effect, sx_kernel::registry::Effect::Read);
    assert!(world.registry.scope_check("svc.call").is_some());
    assert_eq!(world.endpoints.ids().collect::<Vec<_>>(), ["svc.call"]);
    assert_eq!(capability.description, "GET https://api.example.com/repos/{owner}/{name}");
}

#[test]
fn declaration_errors_are_reported_at_load() {
    let cases: [(String, &str); 14] = [
        (endpoint("https://a.example", "GET", "/r/{owner}/{missing}", "[]", REPO_INPUT, "String", "read"), "path variable 'missing' is not a field"),
        (endpoint("https://a.example", "GET", "/r/{owner}", "[\"nope\"]", REPO_INPUT, "String", "read"), "query parameter 'nope' is not a field"),
        (endpoint("https://a.example", "GET", "/r/{owner}", "[]", REPO_INPUT, "String", "read"), "input fields name are not used in the path or query, and GET sends no body"),
        (endpoint("https://{owner}.example", "GET", "/r/{name}", "[]", REPO_INPUT, "String", "read"), "origin of url"),
        (endpoint("https://a.example", "GET", "/r/{owner}/{name}?x=1", "[]", REPO_INPUT, "String", "read"), "no query or fragment"),
        (endpoint("https://a.example", "GET", "/r/{owner}/{owner}/{name}", "[]", REPO_INPUT, "String", "read"), "appears twice"),
        (endpoint("https://a.example", "GET", "/r/{owner}/{name", "[]", REPO_INPUT, "String", "read"), "malformed"),
        (endpoint("https://a.example", "GET", "/r/}{owner}/{name}", "[]", REPO_INPUT, "String", "read"), "without a {"),
        (endpoint("https://a.example", "GET", "/r/{n}", "[]", "Record {n: List<String>}", "String", "read"), "not optional or a list"),
        (endpoint("https://a.example", "GET", "/r/{n}", "[]", "Record {n: Float}", "String", "read"), "not a String, Int, or Bool"),
        (endpoint("ftp://a.example", "GET", "/r", "[]", "Record {}", "String", "read"), "scheme must be http or https"),
        (endpoint("https://u@a.example", "GET", "/r", "[]", "Record {}", "String", "read"), "userinfo"),
        (endpoint("https://a.example", "get", "/r", "[]", "Record {}", "String", "read"), "uppercase HTTP method token"),
        (endpoint("https://a.example", "GET", "/r", "[]", "String", "String", "read"), "input must be a record"),
    ];
    for (declarations, expected) in cases {
        let error = declare(&declarations).err().unwrap_or_else(|| panic!("{declarations} declares"));
        assert!(error.contains(expected), "{declarations}: {error}");
        assert!(error.contains("endpoint 'svc.call'"), "{error}");
    }
}

#[test]
fn declaration_keys_are_checked() {
    let base = |extra: &str| {
        format!(
            r#"{{"capabilities": [{{"id": "svc.call", {extra} "http": {{"method": "GET", "url": "https://a.example/r"}}, "input": "Record {{}}", "output": "String", "effect": "read"}}]}}"#
        )
    };
    assert!(declare(&base("")).is_ok());
    assert!(declare(&base(r#""scope": "Record {origins: List<String>, methods: List<String>}","#)).err().expect("refused").contains("scope is implied"));
    let unknown = base("").replace(r#""url""#, r#""timeout": 5, "url""#);
    assert!(declare(&unknown).err().expect("refused").contains("unknown key 'timeout'"));
    let framing = base("").replace(r#""url""#, r#""headers": {"Host": "x"}, "url""#);
    assert!(declare(&framing).err().expect("refused").contains("sets this header itself"));
    let twice = r#"{"capabilities": [
        {"id": "svc.call", "http": {"method": "GET", "url": "https://a.example/r"}, "input": "Record {}", "output": "String", "effect": "read"},
        {"id": "svc.call", "http": {"method": "GET", "url": "https://a.example/s"}, "input": "Record {}", "output": "String", "effect": "read"}]}"#;
    assert!(declare(twice).err().expect("refused").contains("already registered"));
    let read_idempotent = base("").replace(r#""effect": "read""#, r#""effect": "read", "idempotent": true"#);
    assert!(declare(&read_idempotent).err().expect("refused").contains("idempotent does not apply"));
}

#[test]
fn a_plain_loader_refuses_endpoints() {
    let json = parse(&endpoint("https://a.example", "GET", "/r", "[]", "Record {}", "String", "read")).expect("JSON");
    let error = sx_kernel::registry::load_declarations(&mut Registry::new(), &json).expect_err("refused");
    assert!(error.to_string().contains("declares an http endpoint"), "{error}");
}

#[test]
fn a_typed_get_decodes_the_response_and_drops_unknown_fields() {
    let server = Server::start(|_| {
        Reply::json(r#"{"full_name": "acme/widget", "stargazers_count": 7, "private": false, "owner": {"login": "acme"}}"#)
    });
    let world = repo(&server.origin());
    let grants = grant(&world, &[&server.origin()], &["GET"]);
    let reply = call(&world, &grants, r#"{"owner": "acme", "name": "widget"}"#).expect("the call succeeds");
    assert_eq!(shown(&reply), r#"{:full_name "acme/widget" :stargazers_count 7}"#);
    let seen = &server.requests()[0];
    assert_eq!(seen.method, "GET");
    assert_eq!(seen.target, "/repos/acme/widget");
    assert_eq!(seen.header("accept"), Some("application/json"));
    assert_eq!(seen.header("content-type"), None);
}

#[test]
fn path_variables_are_percent_encoded_one_segment_each() {
    let server = Server::start(|_| Reply::json(r#"{"full_name": "x", "stargazers_count": 1}"#));
    let world = repo(&server.origin());
    let grants = grant(&world, &[&server.origin()], &["GET"]);
    call(&world, &grants, r#"{"owner": "a b/c?d#e", "name": "x%2eé"}"#).expect("the call succeeds");
    assert_eq!(server.requests()[0].target, "/repos/a%20b%2Fc%3Fd%23e/x%252e%C3%A9");
    for bad in ["", ".", ".."] {
        let failure = call(&world, &grants, &format!(r#"{{"owner": "{bad}", "name": "x"}}"#)).expect_err("refused");
        assert_eq!(failure.kind, CallErrorKind::Invalid, "{bad:?}");
    }
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn query_parameters_come_from_listed_fields() {
    let server = Server::start(|_| Reply::json("[]"));
    let world = declare(&endpoint(
        &server.origin(),
        "GET",
        "/search/{kind}",
        r#"["q", "page", "draft", "label", "after"]"#,
        "Record {kind: String, q: String, page: Int, draft: Bool, label: List<String>, after: Option<String>}",
        "List<String>",
        "read",
    ))
    .expect("the endpoint declares");
    let grants = grant(&world, &[&server.origin()], &["GET"]);
    let base = r#"{"kind": "issues", "q": "is open&x=1 é", "page": 3, "draft": false, "label": ["a b", "c"], "after": null}"#;
    call(&world, &grants, base).expect("the call succeeds");
    assert_eq!(
        server.requests()[0].target,
        "/search/issues?q=is%20open%26x%3D1%20%C3%A9&page=3&draft=false&label=a%20b&label=c"
    );
    let with_after = base.replace(r#""after": null"#, r#""after": "z""#);
    call(&world, &grants, &with_after).expect("the call succeeds");
    assert!(server.requests()[1].target.ends_with("&after=z"));
    let empty = r#"{"kind": "k", "q": "", "page": 1, "draft": true, "label": [], "after": null}"#;
    call(&world, &grants, empty).expect("the call succeeds");
    assert_eq!(server.requests()[2].target, "/search/k?q=&page=1&draft=true");
}

#[test]
fn a_post_sends_the_unused_fields_as_a_json_body() {
    let server = Server::start(|seen| Reply::json(&format!(r#"{{"echo": {}}}"#, seen.text())));
    let world = declare(&endpoint(
        &server.origin(),
        "POST",
        "/orgs/{org}/issues",
        r#"["dry"]"#,
        "Record {org: String, dry: Bool, title: String, labels: List<String>, milestone: Option<Int>}",
        "Record open {echo: Record open {title: String}}",
        "write",
    ))
    .expect("the endpoint declares");
    let grants = grant(&world, &[&server.origin()], &["POST"]);
    call(
        &world,
        &grants,
        r#"{"org": "acme", "dry": true, "title": "café \"quoted\"", "labels": ["bug"], "milestone": null}"#,
    )
    .expect("the call succeeds");
    let seen = &server.requests()[0];
    assert_eq!(seen.method, "POST");
    assert_eq!(seen.target, "/orgs/acme/issues?dry=true");
    assert_eq!(seen.header("content-type"), Some("application/json"));
    assert_eq!(seen.header("accept"), Some("application/json"));
    let sent = parse(seen.text()).expect("the body is JSON");
    assert_eq!(sent.get("title").and_then(Json::as_str), Some("café \"quoted\""));
    assert_eq!(sent.get("labels").and_then(Json::as_array).map(<[Json]>::len), Some(1));
    assert!(matches!(sent.get("milestone"), Some(Json::Null)));
    assert!(sent.get("org").is_none() && sent.get("dry").is_none());
}

#[test]
fn put_and_patch_send_bodies_and_a_post_without_fields_sends_none() {
    let server = Server::start(|seen| Reply::json(&format!("\"{} {}\"", seen.method, seen.body.len())));
    for (method, fields, expected_length) in [("PUT", "name: String", Some("PUT 18")), ("PATCH", "name: String", Some("PATCH 18")), ("POST", "", Some("POST 0"))] {
        let world = declare(&endpoint(
            &server.origin(),
            method,
            "/things/{id}",
            "[]",
            &format!("Record {{id: Int{}{fields}}}", if fields.is_empty() { "" } else { ", " }),
            "String",
            "write",
        ))
        .expect("the endpoint declares");
        let grants = grant(&world, &[&server.origin()], &[method]);
        let json = if fields.is_empty() { r#"{"id": 5}"#.to_string() } else { r#"{"id": 5, "name": "widget"}"#.to_string() };
        let reply = call(&world, &grants, &json).expect("the call succeeds");
        assert_eq!(shown(&reply), format!("\"{}\"", expected_length.expect("expected")));
    }
    let sent = server.requests();
    assert_eq!(sent[2].header("content-type"), None);
    assert_eq!(sent[0].text(), r#"{"name": "widget"}"#);
}

#[test]
fn a_2xx_body_that_does_not_match_the_output_is_invalid_with_its_path() {
    let server = Server::start(|seen| match seen.target.as_str() {
        "/extra" => Reply::json(r#"{"a": 1, "b": 2}"#),
        "/missing" => Reply::json(r#"{}"#),
        "/mistyped" => Reply::json(r#"{"a": "one"}"#),
        "/nested" => Reply::json(r#"{"a": 1, "list": [{"n": 1}, {"n": "x"}]}"#),
        "/html" => Reply::with(200, &[("content-type", "text/html")], "<html>"),
        "/empty" => Reply::status(200, ""),
        _ => Reply::Http { status: 200, headers: Vec::new(), body: vec![0xff], delay: Duration::ZERO },
    });
    let closed = |path: &str, output: &str| {
        let world = declare(&endpoint(&server.origin(), "GET", path, "[]", "Record {}", output, "read")).expect("declares");
        let grants = grant(&world, &[&server.origin()], &["GET"]);
        call(&world, &grants, "{}")
    };
    let mismatch = |failure: Failure, needle: &str| {
        assert_eq!(failure.kind, CallErrorKind::Invalid, "{failure}");
        assert!(failure.message.contains(needle), "{}", failure.message);
    };
    mismatch(closed("/extra", "Record {a: Int}").expect_err("closed"), "$.b: expected no field");
    assert!(closed("/extra", "Record open {a: Int}").is_ok());
    mismatch(closed("/missing", "Record {a: Int}").expect_err("missing"), "$.a: expected Int, found no field");
    mismatch(closed("/mistyped", "Record {a: Int}").expect_err("mistyped"), "$.a: expected Int, found a string");
    mismatch(
        closed("/nested", "Record open {list: List<Record {n: Int}>}").expect_err("nested"),
        "$.list[1].n: expected Int, found a string",
    );
    mismatch(closed("/html", "String").expect_err("html"), "not JSON");
    mismatch(closed("/empty", "String").expect_err("empty"), "not JSON");
    mismatch(closed("/bytes", "String").expect_err("bytes"), "not valid UTF-8");
}

#[test]
fn a_write_whose_response_cannot_be_read_is_failed_not_invalid() {
    let server = Server::start(|_| Reply::json(r#"{"unexpected": true}"#));
    let world = declare(&endpoint(&server.origin(), "POST", "/make", "[]", "Record {}", "Record {id: Int}", "write"))
        .expect("declares");
    let grants = grant(&world, &[&server.origin()], &["POST"]);
    let failure = call(&world, &grants, "{}").expect_err("mismatch");
    assert_eq!(failure.kind, CallErrorKind::Failed);
    assert!(failure.message.contains("$.id"), "{}", failure.message);
}

#[test]
fn a_unit_output_ignores_the_body() {
    let server = Server::start(|_| Reply::status(204, ""));
    let world = declare(&endpoint(&server.origin(), "DELETE", "/things/{id}", "[]", "Record {id: Int}", "Unit", "write"))
        .expect("declares");
    let grants = grant(&world, &[&server.origin()], &["DELETE"]);
    assert_eq!(call(&world, &grants, r#"{"id": 1}"#).expect("succeeds"), Value::Unit);
    assert_eq!(server.requests()[0].method, "DELETE");
}

#[test]
fn every_status_maps_to_its_documented_kind() {
    let table: [(u16, CallErrorKind); 22] = [
        (400, CallErrorKind::Invalid),
        (401, CallErrorKind::PermissionDenied),
        (403, CallErrorKind::PermissionDenied),
        (404, CallErrorKind::NotFound),
        (409, CallErrorKind::Conflict),
        (412, CallErrorKind::Conflict),
        (422, CallErrorKind::Invalid),
        (429, CallErrorKind::RateLimited),
        (500, CallErrorKind::Unavailable),
        (501, CallErrorKind::Failed),
        (502, CallErrorKind::Unavailable),
        (503, CallErrorKind::Unavailable),
        (504, CallErrorKind::Unavailable),
        (599, CallErrorKind::Unavailable),
        (405, CallErrorKind::Failed),
        (406, CallErrorKind::Failed),
        (410, CallErrorKind::Failed),
        (413, CallErrorKind::Failed),
        (418, CallErrorKind::Failed),
        (451, CallErrorKind::Failed),
        (304, CallErrorKind::Failed),
        (300, CallErrorKind::Failed),
    ];
    let server = Server::start(|seen| {
        let status: u16 = seen.target.trim_start_matches('/').parse().expect("a status path");
        Reply::status(status, "explained")
    });
    for effect in ["read", "write"] {
        let method = if effect == "read" { "GET" } else { "POST" };
        let world = declare(&endpoint(&server.origin(), method, "/{status}", "[]", "Record {status: Int}", "Record open {}", effect))
            .expect("declares");
        let grants = grant(&world, &[&server.origin()], &[method]);
        for (status, expected) in table {
            let failure = call(&world, &grants, &format!(r#"{{"status": {status}}}"#)).expect_err("a failure");
            assert_eq!(failure.kind, expected, "{effect} {status}: {failure}");
            assert!(failure.message.contains(&format!("answered {status}")), "{}", failure.message);
        }
    }
}

#[test]
fn error_messages_leave_out_the_query() {
    let server = Server::start(|_| Reply::status(404, ""));
    let world = declare(&endpoint(&server.origin(), "GET", "/x", r#"["token"]"#, "Record {token: String}", "String", "read"))
        .expect("declares");
    let grants = grant(&world, &[&server.origin()], &["GET"]);
    let failure = call(&world, &grants, r#"{"token": "hunter2"}"#).expect_err("not found");
    assert_eq!(failure.kind, CallErrorKind::NotFound);
    assert!(!failure.message.contains("hunter2"), "{}", failure.message);
}

#[test]
fn an_unfollowed_redirect_is_a_failure_and_a_followed_one_decodes() {
    let outside = Server::start(|_| Reply::json("\"leaked\""));
    let target = format!("{}/x", outside.origin());
    let server = Server::start(move |seen| match seen.target.as_str() {
        "/in" => Reply::redirect(302, "/landed"),
        "/landed" => Reply::json("\"here\""),
        _ => Reply::redirect(302, &target),
    });
    let get_path = |path: &str| {
        let world = declare(&endpoint(&server.origin(), "GET", path, "[]", "Record {}", "String", "read")).expect("declares");
        let grants = grant(&world, &[&server.origin()], &["GET"]);
        call(&world, &grants, "{}")
    };
    assert_eq!(shown(&get_path("/in").expect("followed")), "\"here\"");
    let failure = get_path("/out").expect_err("not followed");
    assert_eq!(failure.kind, CallErrorKind::Failed);
    assert_eq!(outside.connections(), 0);
}

#[test]
fn transport_failures_map_like_raw_requests() {
    let port = closed_port();
    let refused = format!("http://127.0.0.1:{port}");
    let world = declare(&endpoint(&refused, "GET", "/x", "[]", "Record {}", "String", "read")).expect("declares");
    let grants = grant(&world, &[&refused], &["GET"]);
    assert_eq!(call(&world, &grants, "{}").expect_err("refused").kind, CallErrorKind::Unavailable);
    let slow = Server::start(|_| Reply::slow(Duration::from_secs(3)));
    let world = declare(&endpoint(&slow.origin(), "GET", "/x", "[]", "Record {}", "String", "read")).expect("declares");
    let grants = grant(&world, &[&slow.origin()], &["GET"]);
    assert_eq!(call(&world, &grants, "{}").expect_err("slow").kind, CallErrorKind::Timeout);
}

#[test]
fn the_response_body_cap_applies_to_endpoints() {
    let server = Server::start(|_| Reply::json(&format!("\"{}\"", "a".repeat(4096))));
    let world = declare(&endpoint(&server.origin(), "GET", "/x", "[]", "Record {}", "String", "read")).expect("declares");
    let grants = grant(&world, &[&server.origin()], &["GET"]);
    let failure = call(&world, &grants, "{}").expect_err("too large");
    assert_eq!(failure.kind, CallErrorKind::Failed);
    assert!(failure.message.contains("larger than 1024 bytes"), "{}", failure.message);
}

#[test]
fn an_endpoint_is_denied_outside_its_grant_without_a_request() {
    let server = Server::start(|_| Reply::json(r#"{"full_name": "x", "stargazers_count": 1}"#));
    let elsewhere = Server::start(|_| Reply::json("{}"));
    let world = repo(&server.origin());
    let input_json = r#"{"owner": "o", "name": "n"}"#;

    let allowed = grant(&world, &[&server.origin()], &["GET", "POST"]);
    assert_eq!(verdict(&world, &allowed, input_json), Verdict::Allowed);

    let wrong_origin = grant(&world, &[&elsewhere.origin()], &["GET"]);
    let wrong_scheme = grant(&world, &[&format!("https://127.0.0.1:{}", server.port())], &["GET"]);
    let wrong_port = grant(&world, &[&format!("http://127.0.0.1:{}", server.port() + 1)], &["GET"]);
    let wrong_host = grant(&world, &[&format!("http://localhost:{}", server.port())], &["GET"]);
    for denied in [&wrong_origin, &wrong_scheme, &wrong_port, &wrong_host] {
        let why = match verdict(&world, denied, input_json) {
            Verdict::Denied(why) => why,
            Verdict::Allowed => panic!("allowed"),
        };
        assert!(why.contains("outside every granted origin"), "{why}");
        assert_eq!(call(&world, denied, input_json).expect_err("denied").kind, CallErrorKind::PermissionDenied);
    }
    let wrong_method = grant(&world, &[&server.origin()], &["POST"]);
    match verdict(&world, &wrong_method, input_json) {
        Verdict::Denied(why) => assert!(why.contains("method GET is not granted"), "{why}"),
        Verdict::Allowed => panic!("allowed"),
    }
    assert_eq!(call(&world, &wrong_method, input_json).expect_err("denied").kind, CallErrorKind::PermissionDenied);
    assert_eq!(server.connections(), 0);
    assert_eq!(elsewhere.connections(), 0);
}

#[test]
fn a_granted_scope_is_validated_like_a_raw_one() {
    let world = repo("https://api.example.com");
    let bad = Json::object([("svc.call", scope(&["https://api.example.com/v1"], &["GET"]))]);
    let error = Grants::load(&world.registry, &bad).expect_err("refused");
    assert!(error.to_string().contains("has a path"), "{error}");
}

#[test]
fn an_endpoint_origin_with_a_default_port_is_normalized() {
    let world = repo("HTTPS://API.Example.com:443");
    let grants = grant(&world, &["https://api.example.com"], &["GET"]);
    assert_eq!(verdict(&world, &grants, r#"{"owner": "o", "name": "n"}"#), Verdict::Allowed);
    let other = Grants::load(
        &world.registry,
        &Json::object([("svc.call", scope(&["https://api.example.com:8443"], &["GET"]))]),
    )
    .expect("loads");
    assert!(matches!(verdict(&world, &other, r#"{"owner": "o", "name": "n"}"#), Verdict::Denied(_)));
}
