mod support;

use std::sync::Arc;
use std::time::Duration;

use support::{Reply, Seen, Server, closed_port};
use sx_kernel::authority::{Grant, Grants, Screen, Verdict};
use sx_kernel::json::{Json, parse};
use sx_kernel::registry::{Registry, load_declarations_with};
use sx_kernel::types::CallErrorKind;
use sx_kernel::values::{MapValue, RecordValue, Value, decode, show_value};
use sx_packs::http::{self, EndpointLoader, Endpoints, GET, Limits, REQUEST};
use sx_packs::{Failure, pack};

const FAST: Limits = Limits {
    timeout: Duration::from_millis(600),
    connect_timeout: Duration::from_millis(600),
    max_body: 4096,
    max_redirects: 5,
};

const SECRET: &str = "Bearer s3cr3t-token-0123456789";
const TOKEN: &str = "s3cr3t-token-0123456789";

struct World {
    registry: Registry,
    endpoints: Endpoints,
}

fn world(declarations: &str) -> World {
    let mut registry = Registry::new();
    registry.add_pack(pack("http").expect("the http pack")).expect("registers");
    let mut loader = EndpointLoader::new();
    load_declarations_with(&mut registry, &parse(declarations).expect("JSON"), &mut loader).expect("declares");
    World { registry, endpoints: loader.finish() }
}

fn endpoint_declarations(origin: &str, extra_headers: &str) -> String {
    format!(
        r#"{{"capabilities": [
  {{"id": "svc.put", "http": {{"method": "PUT", "url": "{origin}/objects/{{key}}"{extra_headers}}}, "input": "Record {{key: String, body: String}}", "output": "Record open {{stored: Bool}}", "effect": "write"}}
]}}"#
    )
}

fn scope_json(origins: &[&str], headers: Json) -> Json {
    Json::object([
        ("origins", Json::Array(origins.iter().map(|o| Json::string(*o)).collect())),
        ("methods", Json::Array(vec![Json::string("GET"), Json::string("PUT"), Json::string("POST")])),
        ("headers", headers),
    ])
}

fn injection(origin: &str, pairs: &[(&str, &str)]) -> Json {
    Json::object([(origin, Json::object(pairs.iter().map(|(n, v)| (*n, Json::string(*v)))))])
}

fn grants_with(world: &World, scope: &Json) -> Result<Grants, String> {
    let mut all = vec![(REQUEST.to_string(), scope.clone()), (GET.to_string(), scope.clone())];
    all.extend(world.endpoints.ids().map(|id| (id.to_string(), scope.clone())));
    Grants::load(&world.registry, &Json::Object(all)).map_err(|e| e.to_string())
}

fn grants(world: &World, scope: &Json) -> Grants {
    grants_with(world, scope).expect("the grants load")
}

fn scope_of(grants: &Grants, id: &str) -> Value {
    match grants.get(id) {
        Some(Grant::Scoped { scope, .. }) => scope.clone(),
        other => panic!("{id} is not granted with a scope: {other:?}"),
    }
}

fn headers(pairs: &[(&str, &str)]) -> Value {
    Value::Map(MapValue::from_pairs(pairs.iter().map(|(n, v)| (Arc::from(*n), Value::string(v)))))
}

fn get_input(url: &str, pairs: &[(&str, &str)]) -> Value {
    Value::Record(RecordValue::new(vec![
        (Arc::from("url"), Value::string(url)),
        (Arc::from("headers"), headers(pairs)),
    ]))
}

fn request_input(method: &str, url: &str, pairs: &[(&str, &str)], body: &str) -> Value {
    Value::Record(RecordValue::new(vec![
        (Arc::from("method"), Value::string(method)),
        (Arc::from("url"), Value::string(url)),
        (Arc::from("headers"), headers(pairs)),
        (Arc::from("body"), Value::some(Value::string(body))),
    ]))
}

fn get(grants: &Grants, url: &str, pairs: &[(&str, &str)]) -> Result<Value, Failure> {
    http::get_with(&FAST, &scope_of(grants, GET), &get_input(url, pairs))
}

fn endpoint_call(world: &World, grants: &Grants) -> Result<Value, Failure> {
    let capability = world.registry.capability("svc.put").expect("declared");
    let input = decode(&parse(r#"{"key": "a", "body": "hello"}"#).expect("JSON"), &capability.input, "$").expect("decodes");
    world.endpoints.get("svc.put").expect("bound").perform_with(&FAST, &scope_of(grants, "svc.put"), &input)
}

fn verdict(grants: &Grants, id: &str, input: &Value) -> Verdict {
    match grants.screen(id, input) {
        Screen::Verdict(verdict) => verdict,
        other => panic!("{id} has no scope check: {other:?}"),
    }
}

fn plain_world() -> World {
    world(r#"{}"#)
}

fn echo(seen: &Seen) -> Reply {
    let mut lines = vec![format!("{} {}", seen.method, seen.target)];
    lines.extend(seen.headers.iter().map(|(n, v)| format!("{n}: {v}")));
    Reply::with(200, &[("x-echo-authorization", seen.header("authorization").unwrap_or("none"))], &lines.join("\n"))
}

#[test]
fn the_granted_header_reaches_the_server_on_every_capability() {
    let server = Server::start(|seen| {
        if seen.target.starts_with("/objects/") { Reply::json(r#"{"stored": true}"#) } else { Reply::ok("fine") }
    });
    let origin = server.origin();
    let w = world(&endpoint_declarations(&origin, ""));
    let g = grants(&w, &scope_json(&[&origin], injection(&origin, &[("Authorization", SECRET), ("x-team", "blue")])));
    let result = get(&g, &format!("{origin}/a"), &[("accept", "text/plain")]).expect("get");
    assert!(!show_value(&result).contains(TOKEN));
    let posted = http::request_with(&FAST, &scope_of(&g, REQUEST), &request_input("POST", &format!("{origin}/b"), &[], "{}")).expect("post");
    assert!(!show_value(&posted).contains(TOKEN));
    let stored = endpoint_call(&w, &g).expect("endpoint");
    assert_eq!(show_value(&stored), "{:stored true}");
    let seen = server.requests();
    assert_eq!(seen.len(), 3);
    for request in &seen {
        assert_eq!(request.header("authorization"), Some(SECRET), "{}", request.target);
        assert_eq!(request.header("x-team"), Some("blue"), "{}", request.target);
    }
    assert_eq!(seen[0].header("accept"), Some("text/plain"));
    assert_eq!(seen[2].header("content-type"), Some("application/json"));
}

#[test]
fn a_grant_without_headers_sends_none_and_an_empty_map_is_the_same() {
    let server = Server::start(echo);
    let origin = server.origin();
    let w = plain_world();
    let omitted = Json::object([
        ("origins", Json::Array(vec![Json::string(&origin)])),
        ("methods", Json::Array(vec![Json::string("GET")])),
    ]);
    for scope in [omitted, scope_json(&[&origin], Json::Object(Vec::new()))] {
        let g = grants(&w, &scope);
        get(&g, &format!("{origin}/"), &[]).expect("get");
    }
    assert!(server.requests().iter().all(|request| request.header("authorization").is_none()));
}

#[test]
fn each_origin_injects_only_its_own_headers_across_redirects() {
    let other = Server::start(echo);
    let other_origin = other.origin();
    let target = format!("{other_origin}/landing");
    let first = Server::start(move |seen| match seen.target.as_str() {
        "/to-other" => Reply::redirect(302, &target),
        "/within" => Reply::redirect(302, "/after"),
        _ => Reply::ok("home"),
    });
    let origin = first.origin();
    let w = plain_world();
    let headers = Json::object([
        (origin.as_str(), Json::object([("authorization", Json::string("Bearer first-origin-secret-value"))])),
        (other_origin.as_str(), Json::object([("authorization", Json::string("Bearer other-origin-secret-value"))])),
    ]);
    let g = grants(&w, &scope_json(&[&origin, &other_origin], headers));
    get(&g, &format!("{origin}/within"), &[]).expect("same-origin redirect");
    let within: Vec<_> = first.requests().iter().map(|r| (r.target.clone(), r.header("authorization").map(str::to_string))).collect();
    assert_eq!(
        within,
        [
            ("/within".to_string(), Some("Bearer first-origin-secret-value".to_string())),
            ("/after".to_string(), Some("Bearer first-origin-secret-value".to_string()))
        ]
    );
    let result = get(&g, &format!("{origin}/to-other"), &[("x-program", "kept-at-home")]).expect("cross-origin redirect");
    let landed = other.requests();
    assert_eq!(landed.len(), 1);
    assert_eq!(landed[0].header("authorization"), Some("Bearer other-origin-secret-value"));
    assert_eq!(landed[0].header("x-program"), None, "the program's own headers do not cross origins");
    let shown = show_value(&result);
    assert!(!shown.contains("first-origin-secret-value") && !shown.contains("other-origin-secret-value"), "{shown}");
}

#[test]
fn the_secret_is_redacted_from_whatever_the_server_echoes_back() {
    let server = Server::start(|seen| {
        if seen.target.starts_with("/objects/") {
            let echoed = seen.header("authorization").unwrap_or("none");
            return Reply::with(500, &[("x-token", TOKEN)], &format!("rejected {echoed}"));
        }
        echo(seen)
    });
    let origin = server.origin();
    let w = world(&endpoint_declarations(&origin, ""));
    let g = grants(&w, &scope_json(&[&origin], injection(&origin, &[("authorization", SECRET)])));
    let result = get(&g, &format!("{origin}/echo"), &[]).expect("get");
    let shown = show_value(&result);
    assert!(!shown.contains(TOKEN), "{shown}");
    assert!(shown.contains("authorization: [redacted]"), "{shown}");
    assert!(shown.contains(r#""x-echo-authorization" "[redacted]""#), "{shown}");
    let failure = endpoint_call(&w, &g).expect_err("a 500");
    assert_eq!(failure.kind, CallErrorKind::Unavailable);
    assert!(!failure.message.contains(TOKEN), "{failure}");
    assert!(failure.message.contains("rejected [redacted]"), "{failure}");
    let parsed = http::Scope::of(&scope_of(&g, GET)).expect("the prepared scope parses");
    assert!(!format!("{parsed:?}").contains(TOKEN), "a debug print of the scope hides the secret");
}

#[test]
fn a_program_cannot_set_override_or_probe_a_header_the_grant_injects() {
    let server = Server::start(echo);
    let origin = server.origin();
    let w = plain_world();
    let g = grants(&w, &scope_json(&[&origin], injection(&origin, &[("authorization", SECRET)])));
    for name in ["authorization", "Authorization", "AUTHORIZATION"] {
        let input = get_input(&format!("{origin}/"), &[(name, "mine")]);
        let Verdict::Denied(reason) = verdict(&g, GET, &input) else { panic!("{name} was allowed") };
        assert!(reason.contains("the grant sets the header authorization") && !reason.contains(TOKEN), "{reason}");
        let failure = get(&g, &format!("{origin}/"), &[(name, "mine")]).expect_err("denied at run time");
        assert_eq!(failure.kind, CallErrorKind::PermissionDenied, "{failure}");
        assert!(!failure.message.contains(TOKEN), "{failure}");
        let request = request_input("POST", &format!("{origin}/"), &[(name, "mine")], "{}");
        assert!(matches!(verdict(&g, REQUEST, &request), Verdict::Denied(_)));
        let failure = http::request_with(&FAST, &scope_of(&g, REQUEST), &request).expect_err("denied at run time");
        assert_eq!(failure.kind, CallErrorKind::PermissionDenied, "{failure}");
    }
    assert_eq!(server.connections(), 0, "a denied request opens no socket");
    let other_header = get_input(&format!("{origin}/"), &[("x-other", "fine")]);
    assert_eq!(verdict(&g, GET, &other_header), Verdict::Allowed);
    get(&g, &format!("{origin}/"), &[("x-other", "fine")]).expect("a different header is allowed");
}

#[test]
fn a_header_is_only_reserved_at_the_origin_that_injects_it() {
    let injected = Server::start(echo);
    let plain = Server::start(echo);
    let (injected_origin, plain_origin) = (injected.origin(), plain.origin());
    let w = plain_world();
    let g = grants(&w, &scope_json(&[&injected_origin, &plain_origin], injection(&injected_origin, &[("authorization", SECRET)])));
    get(&g, &format!("{plain_origin}/"), &[("authorization", "program token")]).expect("no injection at this origin");
    assert_eq!(plain.requests()[0].header("authorization"), Some("program token"));
}

#[test]
fn an_endpoint_that_declares_an_injected_header_is_out_of_scope_before_the_run() {
    let server = Server::start(echo);
    let origin = server.origin();
    let w = world(&endpoint_declarations(&origin, r#", "headers": {"Authorization": "static"}"#));
    let g = grants(&w, &scope_json(&[&origin], injection(&origin, &[("authorization", SECRET)])));
    let input = Value::Record(RecordValue::new(vec![
        (Arc::from("key"), Value::string("a")),
        (Arc::from("body"), Value::string("x")),
    ]));
    let Verdict::Denied(reason) = verdict(&g, "svc.put", &input) else { panic!("allowed") };
    assert!(reason.contains("the grant sets the header authorization"), "{reason}");
    let failure = endpoint_call(&w, &g).expect_err("denied at run time");
    assert_eq!(failure.kind, CallErrorKind::PermissionDenied, "{failure}");
    assert_eq!(server.connections(), 0);
    let relaxed = grants(&w, &scope_json(&[&origin], injection(&origin, &[("x-other", "value-long-enough")])));
    assert_eq!(verdict(&relaxed, "svc.put", &input), Verdict::Allowed);
}

#[test]
fn a_connection_failure_names_the_url_and_never_the_secret() {
    let port = closed_port();
    let origin = format!("http://127.0.0.1:{port}");
    let w = plain_world();
    let g = grants(&w, &scope_json(&[&origin], injection(&origin, &[("authorization", SECRET)])));
    let failure = get(&g, &format!("{origin}/x"), &[]).expect_err("nothing listens");
    assert_eq!(failure.kind, CallErrorKind::Unavailable);
    assert!(!failure.message.contains(TOKEN) && !failure.message.contains("Bearer"), "{failure}");
}

#[test]
fn an_env_reference_is_read_when_the_grant_loads() {
    let server = Server::start(echo);
    let origin = server.origin();
    let w = plain_world();
    let reference = |variable: &str| {
        scope_json(&[&origin], Json::object([(origin.as_str(), Json::object([("authorization", Json::object([("env", Json::string(variable))]))]))]))
    };
    let g = grants(&w, &reference("CARGO_MANIFEST_DIR"));
    get(&g, &format!("{origin}/"), &[]).expect("get");
    let expected = std::env::var("CARGO_MANIFEST_DIR").expect("set by cargo");
    assert_eq!(server.requests()[0].header("authorization"), Some(expected.as_str()));
    let missing = grants_with(&w, &reference("SX_NO_SUCH_VARIABLE_FOR_TESTS")).expect_err("a missing variable is refused");
    assert!(missing.contains("the environment variable SX_NO_SUCH_VARIABLE_FOR_TESTS is not set"), "{missing}");
    for bad in ["", "A=B", "A\0B"] {
        let error = grants_with(&w, &reference(bad)).expect_err("an invalid name is refused");
        assert!(error.contains("is not a valid environment variable name"), "{error}");
    }
    let wrong_key = Json::object([("other", Json::string("X"))]);
    let scope = scope_json(&[&origin], Json::object([(origin.as_str(), Json::object([("authorization", wrong_key)]))]));
    assert!(grants_with(&w, &scope).expect_err("only env is a reference").contains("expected String"));
    assert_eq!(server.connections(), 1);
}

#[test]
fn malformed_credential_grants_are_usage_errors_that_do_not_echo_values() {
    let w = plain_world();
    let origin = "http://127.0.0.1:8080";
    let other = "http://127.0.0.1:9090";
    let value = "do-not-echo-this-value-1234";
    let cases: Vec<(Json, &str)> = vec![
        (injection(other, &[("authorization", value)]), "which is not one of the granted origins"),
        (injection("not an origin", &[("authorization", value)]), "headers for origin"),
        (injection(origin, &[("host", value)]), "the client sets this header itself"),
        (injection(origin, &[("content-length", value)]), "the client sets this header itself"),
        (injection(origin, &[("bad name", value)]), "not a valid header name"),
        (injection(origin, &[("authorization", "")]), "the value is empty"),
        (injection(origin, &[("authorization", "line\nbreak")]), "not a valid header value"),
        (injection(origin, &[("Authorization", value), ("authorization", value)]), "the name appears twice"),
        (Json::object([(origin, Json::string(value))]), "expected"),
        (Json::object([(origin, Json::object([("authorization", Json::Int(5))]))]), "expected String"),
        (
            Json::object([
                ("http://127.0.0.1:8080", Json::object([("x-a", Json::string(value))])),
                ("http://127.0.0.1:08080", Json::object([("x-b", Json::string(value))])),
            ]),
            "given twice",
        ),
    ];
    for (headers, expected) in cases {
        let error = grants_with(&w, &scope_json(&[origin], headers)).expect_err(expected);
        assert!(error.contains(expected), "{error:?} lacks {expected:?}");
        assert!(!error.contains(value), "{error}");
    }
    let spelled = Json::object([("HTTP://127.0.0.1:8080", Json::object([("x-a", Json::string(value))]))]);
    grants_with(&w, &scope_json(&[origin], spelled)).expect("an origin spelled differently normalizes to the granted one");
}
