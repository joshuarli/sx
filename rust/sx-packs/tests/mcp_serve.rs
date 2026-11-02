mod support;

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use support::{Reply, Server as Origin, closed_port};
use sx_kernel::authority::Grants;
use sx_kernel::json::{Json, Layout, dumps, parse, python_equal};
use sx_kernel::registry::{Registry, load_declarations_with};
use sx_kernel::types::CallErrorKind;
use sx_packs::http::EndpointLoader;
use sx_packs::mcp::{Answer, Server, error_result, serve};

fn json(text: &str) -> Json {
    parse(text).unwrap_or_else(|e| panic!("{e}: {text}"))
}

fn shown(value: &Json) -> String {
    dumps(value, Layout::Spaced).expect("serializable")
}

fn server(declarations: &str, grants: &str) -> Server {
    let mut registry = Registry::new();
    let mut loader = EndpointLoader::new();
    let declarations = json(declarations);
    for pack in ["fs", "http"] {
        if declarations.get("packs").and_then(Json::as_array).is_some_and(|packs| packs.iter().any(|p| p.as_str() == Some(pack))) {
            registry.add_pack(sx_packs::pack(pack).expect("a built-in pack")).expect("pack added");
        }
    }
    load_declarations_with(&mut registry, &declarations, &mut loader).expect("declarations load");
    let grants = Grants::load(&registry, &json(grants)).expect("grants load");
    Server::new(&registry, grants, loader.finish()).expect("server builds")
}

fn tool(server: &Server, name: &str) -> Json {
    server
        .tools()
        .into_iter()
        .find(|tool| tool.get("name").and_then(Json::as_str) == Some(name))
        .unwrap_or_else(|| panic!("{name} is not served"))
}

fn same(found: &Json, expected: &str) {
    let expected = json(expected);
    assert!(python_equal(found, &expected), "found {}\nexpected {}", shown(found), shown(&expected));
}

const ORIGIN: &str = "http://127.0.0.1:1";

fn endpoint_server(input: &str, output: &str, method: &str, effect: &str, extra: &str) -> Server {
    let declarations = format!(
        r#"{{{extra} "capabilities": [{{"id": "svc.call", "http": {{"method": "{method}", "url": "{ORIGIN}/x"}},
        "input": "{input}", "output": "{output}", "effect": "{effect}", "description": "Calls the service."}}]}}"#
    );
    let grants = format!(r#"{{"svc.call": {{"origins": ["{ORIGIN}"], "methods": ["{method}"]}}}}"#);
    server(&declarations, &grants)
}

#[test]
fn a_record_maps_with_optional_input_fields_unrequired_and_optional_output_fields_nullable() {
    let served = endpoint_server(
        "Record {name: String, note: Option<String>, tags: List<Option<Int>>}",
        "Record open {id: Int, note: Option<String>}",
        "POST",
        "write",
        "",
    );
    let found = tool(&served, "svc.call");
    same(
        found.get("inputSchema").expect("inputSchema"),
        r#"{"type": "object", "properties": {"name": {"type": "string"}, "note": {"type": "string"},
            "tags": {"type": "array", "items": {"anyOf": [{"type": "integer"}, {"type": "null"}]}}},
            "required": ["name", "tags"], "additionalProperties": false}"#,
    );
    same(
        found.get("outputSchema").expect("outputSchema"),
        r#"{"type": "object", "properties": {"id": {"type": "integer"}, "note": {"anyOf": [{"type": "string"}, {"type": "null"}]}},
            "required": ["id", "note"]}"#,
    );
    assert!(found.get("_meta").is_none());
}

#[test]
fn tuples_maps_bytes_and_enums_map_to_their_schemas() {
    let extra = r#""enums": {"demo.Color": {"Red": null, "Green": null},
                  "demo.Shape": {"Circle": "Float", "Dot": null, "Square": "Record {side: Int}"}},"#;
    let served = endpoint_server(
        "Record {pair: Tuple<Int, String>, labels: Map<String, Float>, blob: Bytes, ok: Bool, color: demo.Color}",
        "Record {shape: demo.Shape, outcome: Result<Int, String>, nothing: Unit}",
        "POST",
        "write",
        extra,
    );
    let found = tool(&served, "svc.call");
    same(
        found.get("inputSchema").expect("inputSchema"),
        r#"{"type": "object", "properties": {
            "pair": {"type": "array", "prefixItems": [{"type": "integer"}, {"type": "string"}], "items": false, "minItems": 2, "maxItems": 2},
            "labels": {"type": "object", "additionalProperties": {"type": "number"}},
            "blob": {"type": "string", "contentEncoding": "base64"},
            "ok": {"type": "boolean"},
            "color": {"type": "string", "enum": ["Red", "Green"]}},
            "required": ["pair", "labels", "blob", "ok", "color"], "additionalProperties": false}"#,
    );
    same(
        found.get("outputSchema").expect("outputSchema"),
        r#"{"type": "object", "properties": {
            "shape": {"oneOf": [
                {"type": "string", "enum": ["Dot"]},
                {"type": "object", "properties": {"tag": {"type": "string", "enum": ["Circle"]}, "value": {"type": "number"}},
                 "required": ["tag", "value"], "additionalProperties": false},
                {"type": "object", "properties": {"tag": {"type": "string", "enum": ["Square"]}, "value":
                    {"type": "object", "properties": {"side": {"type": "integer"}}, "required": ["side"], "additionalProperties": false}},
                 "required": ["tag", "value"], "additionalProperties": false}]},
            "outcome": {"oneOf": [
                {"type": "object", "properties": {"ok": {"type": "integer"}}, "required": ["ok"], "additionalProperties": false},
                {"type": "object", "properties": {"err": {"type": "string"}}, "required": ["err"], "additionalProperties": false}]},
            "nothing": {"type": "null"}},
            "required": ["shape", "outcome", "nothing"], "additionalProperties": false}"#,
    );
}

#[test]
fn a_string_output_has_no_schema_and_any_other_non_object_output_is_wrapped() {
    let text = endpoint_server("Record {}", "String", "GET", "read", "");
    let found = tool(&text, "svc.call");
    assert!(found.get("outputSchema").is_none() && found.get("_meta").is_none(), "{}", shown(&found));
    let list = endpoint_server("Record {}", "List<Int>", "GET", "read", "");
    let found = tool(&list, "svc.call");
    same(
        found.get("outputSchema").expect("outputSchema"),
        r#"{"type": "object", "properties": {"result": {"type": "array", "items": {"type": "integer"}}},
            "required": ["result"], "additionalProperties": false}"#,
    );
    same(found.get("_meta").expect("_meta"), r#"{"sx/result": "wrapped"}"#);
}

#[test]
fn annotations_follow_the_effect() {
    let hints = |method: &str, effect: &str, idempotent: &str| {
        let extra = if idempotent.is_empty() { String::new() } else { format!(r#""idempotent": {idempotent},"#) };
        let declarations = format!(
            r#"{{"capabilities": [{{"id": "svc.call", {extra} "http": {{"method": "{method}", "url": "{ORIGIN}/x"}},
            "input": "Record {{}}", "output": "String", "effect": "{effect}"}}]}}"#
        );
        let grants = format!(r#"{{"svc.call": {{"origins": ["{ORIGIN}"], "methods": ["{method}"]}}}}"#);
        tool(&server(&declarations, &grants), "svc.call").get("annotations").cloned().expect("annotations")
    };
    same(&hints("GET", "read", ""), r#"{"readOnlyHint": true, "destructiveHint": false, "idempotentHint": true}"#);
    same(&hints("PUT", "write", "true"), r#"{"readOnlyHint": false, "destructiveHint": false, "idempotentHint": true}"#);
    same(&hints("POST", "write", ""), r#"{"readOnlyHint": false, "destructiveHint": true, "idempotentHint": false}"#);
}

#[test]
fn only_granted_capabilities_are_served_and_ungranted_names_are_unknown() {
    let declarations = r#"{"packs": ["fs", "http"]}"#;
    let granted = server(declarations, r#"{"fs.read-text": {"roots": ["/"]}, "http.get": {"origins": ["http://127.0.0.1:1"], "methods": ["GET"]}}"#);
    assert_eq!(granted.names().collect::<Vec<_>>(), ["fs.read-text", "http.get"]);
    let Answer::Rejected { code, message } = granted.call(&json(r#"{"name": "fs.list", "arguments": {"path": "/"}}"#)) else {
        panic!("an ungranted tool must not be callable");
    };
    assert_eq!((code, message.as_str()), (-32602, "unknown tool fs.list"));
}

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(label: &str) -> Sandbox {
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("mcp-serve-{}-{label}", std::process::id()));
        if base.exists() {
            fs::remove_dir_all(&base).expect("old sandbox removed");
        }
        fs::create_dir_all(&base).expect("sandbox");
        let root = fs::canonicalize(&base).expect("canonical sandbox");
        fs::write(root.join("note.txt"), "inside").expect("note");
        Sandbox { root }
    }

    fn server(&self) -> Server {
        let scope = format!(r#"{{"roots": ["{}"]}}"#, self.root.display());
        let grants = format!(r#"{{"fs.read-text": {scope}, "fs.list": {scope}, "fs.stat": {scope}, "fs.write-text": {scope}}}"#);
        server(r#"{"packs": ["fs"]}"#, &grants)
    }
}

fn called(server: &Server, name: &str, arguments: &str) -> Json {
    let params = Json::object([("name", Json::string(name)), ("arguments", json(arguments))]);
    match server.call(&params) {
        Answer::Result(result) => result,
        Answer::Rejected { code, message } => panic!("rejected {code}: {message}"),
    }
}

fn failure_kind(result: &Json) -> (String, String) {
    assert_eq!(result.get("isError").and_then(Json::as_bool), Some(true), "{}", shown(result));
    let structured = result.get("structuredContent").expect("structuredContent");
    let kind = structured.get("kind").and_then(Json::as_str).expect("kind").to_string();
    let message = structured.get("message").and_then(Json::as_str).expect("message").to_string();
    let text = result.get("content").and_then(Json::as_array).and_then(|c| c.first()).and_then(|b| b.get("text")).and_then(Json::as_str);
    assert_eq!(text, Some(format!("{kind}: {message}").as_str()));
    (kind, message)
}

#[test]
fn a_call_performs_like_sx_run_and_returns_the_encoded_output() {
    let sandbox = Sandbox::new("calls");
    let served = sandbox.server();
    let path = |name: &str| format!(r#"{{"path": "{}"}}"#, sandbox.root.join(name).display());
    let read = called(&served, "fs.read-text", &path("note.txt"));
    same(&read, r#"{"content": [{"type": "text", "text": "inside"}], "isError": false}"#);
    let stat = called(&served, "fs.stat", &path("note.txt"));
    same(stat.get("structuredContent").expect("structured"), r#"{"result": {"kind": "File", "size": 6}}"#);
    let missing = called(&served, "fs.stat", &path("absent.txt"));
    same(missing.get("structuredContent").expect("structured"), r#"{"result": null}"#);
    let written = called(
        &served,
        "fs.write-text",
        &format!(r#"{{"path": "{}", "text": "made"}}"#, sandbox.root.join("made.txt").display()),
    );
    same(written.get("structuredContent").expect("structured"), r#"{"result": null}"#);
    assert_eq!(fs::read_to_string(sandbox.root.join("made.txt")).expect("written"), "made");
}

#[test]
fn a_call_outside_the_scope_is_permission_denied_and_does_nothing() {
    let sandbox = Sandbox::new("scope");
    let served = sandbox.server();
    let outside = sandbox.root.parent().expect("parent").join("outside.txt");
    let (kind, message) = failure_kind(&called(
        &served,
        "fs.write-text",
        &format!(r#"{{"path": "{}", "text": "x"}}"#, outside.display()),
    ));
    assert_eq!(kind, "PermissionDenied");
    assert!(message.contains("outside every granted root"), "{message}");
    assert!(!outside.exists());
}

#[test]
fn errors_of_the_performers_keep_their_kind() {
    let sandbox = Sandbox::new("kinds");
    let served = sandbox.server();
    let missing = failure_kind(&called(&served, "fs.read-text", &format!(r#"{{"path": "{}"}}"#, sandbox.root.join("absent").display())));
    assert_eq!(missing.0, "NotFound");
    let wrong_type = failure_kind(&called(&served, "fs.read-text", r#"{"path": 5}"#));
    assert_eq!(wrong_type.0, "Invalid");
    let missing_field = failure_kind(&called(&served, "fs.write-text", r#"{"path": "/x"}"#));
    assert_eq!(missing_field.0, "Invalid");
    let extra_field = failure_kind(&called(&served, "fs.read-text", r#"{"path": "/x", "more": 1}"#));
    assert_eq!(extra_field.0, "Invalid");
}

#[test]
fn http_statuses_and_connection_failures_map_to_the_documented_kinds() {
    let origin = Origin::start(|seen| match seen.target.as_str() {
        "/ok" => Reply::json(r#"{"n": 7}"#),
        "/bad" => Reply::json(r#"{"n": "seven"}"#),
        "/400" => Reply::status(400, "no"),
        "/401" => Reply::status(401, "no"),
        "/404" => Reply::status(404, "no"),
        "/409" => Reply::status(409, "no"),
        "/429" => Reply::status(429, "no"),
        "/501" => Reply::status(501, "no"),
        _ => Reply::status(503, "no"),
    });
    let base = origin.origin();
    let declarations = format!(
        r#"{{"capabilities": [{}]}}"#,
        ["ok", "bad", "400", "401", "404", "409", "429", "501", "503"]
            .iter()
            .map(|path| format!(
                r#"{{"id": "svc.p{path}", "http": {{"method": "GET", "url": "{base}/{path}"}}, "input": "Record {{}}", "output": "Record {{n: Int}}", "effect": "read"}}"#
            ))
            .collect::<Vec<_>>()
            .join(",")
    );
    let refused = closed_port();
    let declarations = declarations.replacen(
        "]}",
        &format!(
            r#", {{"id": "svc.refused", "http": {{"method": "GET", "url": "http://127.0.0.1:{refused}/x"}}, "input": "Record {{}}", "output": "Record {{n: Int}}", "effect": "read"}}]}}"#
        ),
        1,
    );
    let grant = |id: &str, origin: &str| format!(r#""{id}": {{"origins": ["{origin}"], "methods": ["GET"]}}"#);
    let mut grants: Vec<String> =
        ["ok", "bad", "400", "401", "404", "409", "429", "501", "503"].iter().map(|p| grant(&format!("svc.p{p}"), &base)).collect();
    grants.push(grant("svc.refused", &format!("http://127.0.0.1:{refused}")));
    let served = server(&declarations, &format!("{{{}}}", grants.join(",")));
    same(called(&served, "svc.pok", "{}").get("structuredContent").expect("structured"), r#"{"n": 7}"#);
    let expect = |name: &str, kind: &str| assert_eq!(failure_kind(&called(&served, name, "{}")).0, kind, "{name}");
    expect("svc.pbad", "Invalid");
    expect("svc.p400", "Invalid");
    expect("svc.p401", "PermissionDenied");
    expect("svc.p404", "NotFound");
    expect("svc.p409", "Conflict");
    expect("svc.p429", "RateLimited");
    expect("svc.p501", "Failed");
    expect("svc.p503", "Unavailable");
    expect("svc.refused", "Unavailable");
}

#[test]
fn every_call_error_kind_becomes_an_error_result_naming_it() {
    for kind in CallErrorKind::ALL {
        let result = error_result(kind, "why");
        let (found, message) = failure_kind(&result);
        assert_eq!((found.as_str(), message.as_str()), (kind.name(), "why"));
    }
}

fn exchange(server: &Server, lines: &[&str]) -> Vec<Json> {
    let input = lines.join("\n") + "\n";
    let mut output = Vec::new();
    serve(server, Cursor::new(input.into_bytes()), &mut output).expect("the connection ends cleanly");
    String::from_utf8(output).expect("UTF-8").lines().map(json).collect()
}

#[test]
fn the_protocol_subset_negotiates_lists_and_rejects_the_rest() {
    let sandbox = Sandbox::new("protocol");
    let served = sandbox.server();
    let replies = exchange(
        &served,
        &[
            r#"{"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2024-11-05"}}"#,
            r#"{"jsonrpc": "2.0", "id": 2, "method": "initialize", "params": {"protocolVersion": "1999-01-01"}}"#,
            r#"{"jsonrpc": "2.0", "method": "notifications/initialized"}"#,
            r#"{"jsonrpc": "2.0", "id": 3, "method": "ping"}"#,
            r#"{"jsonrpc": "2.0", "id": 4, "method": "resources/list"}"#,
            r#"{"jsonrpc": "2.0", "id": 5, "method": "tools/list", "params": {"cursor": "x"}}"#,
            r#"{"jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": {"name": "nope"}}"#,
            r#"{"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": "fs.stat", "arguments": 5}}"#,
            r#"{"jsonrpc": "2.0", "id": 8, "method": "tools/call", "params": {}}"#,
            "not json",
            "[1]",
        ],
    );
    let by_id = |id: i64| replies.iter().find(|r| r.get("id").and_then(Json::as_i64) == Some(id)).unwrap_or_else(|| panic!("no reply {id}"));
    let version = |id: i64| by_id(id).get("result").and_then(|r| r.get("protocolVersion")).and_then(Json::as_str).map(str::to_string);
    assert_eq!(version(1).as_deref(), Some("2024-11-05"));
    assert_eq!(version(2).as_deref(), Some("2025-06-18"));
    same(by_id(3), r#"{"jsonrpc": "2.0", "id": 3, "result": {}}"#);
    let code = |id: i64| by_id(id).get("error").and_then(|e| e.get("code")).and_then(Json::as_i64);
    assert_eq!(code(4), Some(-32601));
    assert_eq!(code(5), Some(-32602));
    assert_eq!(code(6), Some(-32602));
    assert_eq!(code(7), Some(-32602));
    assert_eq!(code(8), Some(-32602));
    let nulls: Vec<Option<i64>> = replies
        .iter()
        .filter(|r| r.get("id").is_some_and(Json::is_null))
        .map(|r| r.get("error").and_then(|e| e.get("code")).and_then(Json::as_i64))
        .collect();
    assert_eq!(nulls, [Some(-32700), Some(-32600)]);
}
