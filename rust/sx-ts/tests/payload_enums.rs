mod common;

use sx_kernel::authority::Authority;
use sx_kernel::json::parse;
use sx_kernel::registry::{Registry, load_declarations};
use sx_ts::declarations::declarations;

const CI: &str = r#"{
  "enums": {
    "mcp-repomod.CiState": {"Passed": null, "Failed": null, "Pending": null},
    "mcp-repomod.CachedCi": {
      "NotCached": "String",
      "Cached": "Record {sha: Option<String>, default-branch: Option<String>, state: mcp-repomod.CiState, age-seconds: Int}"
    },
    "mcp-repomod.Reading": {"Missing": null, "Count": "Int", "Pair": "Record {a: Int, b: Int}"}
  },
  "capabilities": [
    {"id": "mcp-repomod.github-cached-ci", "effect": "read", "input": "Record {repo: String}",
     "output": "Record {result: mcp-repomod.CachedCi}", "description": "The cached CI result."},
    {"id": "mcp-repomod.reading", "effect": "read", "input": "Record {}",
     "output": "mcp-repomod.Reading", "description": "A reading."}
  ]
}"#;

fn registry() -> Registry {
    let mut registry = Registry::new();
    load_declarations(&mut registry, &parse(CI).expect("json")).expect("declarations");
    registry
}

fn codes(source: &str) -> Vec<&'static str> {
    let registry = registry();
    let found = sx_ts::compile_source(source, &registry, Authority::Unrestricted, &Default::default(), Default::default());
    found.diagnostics.iter().map(|d| d.code).collect()
}

fn accepts(source: &str) {
    let found = codes(source);
    assert!(found.is_empty(), "expected acceptance for:\n{source}\nfound {found:?}");
}

fn rejects(code: &str, source: &str) {
    let found = codes(source);
    assert!(found.contains(&code), "expected {code} for:\n{source}\nfound {found:?}");
}

#[test]
fn the_declarations_quote_a_field_that_is_not_an_identifier() {
    let text = declarations(&registry(), &|_| true);
    assert!(text.contains("\"default-branch\": string | null; state: McpRepomodCiState; \"age-seconds\": Int"), "{text}");
    assert!(text.contains("{ tag: \"NotCached\"; value: string } | { tag: \"Cached\"; value:"), "{text}");
    assert!(text.contains("McpRepomodCachedCi"), "{text}");
}

#[test]
fn a_payload_enum_is_told_apart_by_its_tag() {
    accepts(
        "const r = (await tools.mcpRepomod.githubCachedCi({ repo: \"a\" })).result;\n\
         if (r.tag === \"NotCached\") { return r.value; }\n\
         return r.value[\"default-branch\"] ?? r.value.state;",
    );
    accepts(
        "const r = (await tools.mcpRepomod.githubCachedCi({ repo: \"a\" })).result;\n\
         return r.tag === \"Cached\" ? r.value[\"age-seconds\"] : 0;",
    );
    accepts(
        "const r = (await tools.mcpRepomod.githubCachedCi({ repo: \"a\" })).result;\n\
         if (r.tag === \"Cached\" && r.value.state === \"Failed\") { return r.tag; }\n\
         return null;",
    );
}

#[test]
fn a_nullary_variant_is_compared_as_a_string_and_leaves_the_rest_to_narrow() {
    accepts(
        "const s = await tools.mcpRepomod.reading({});\n\
         if (s === \"Missing\") { return 0; }\n\
         return s.tag === \"Count\" ? s.value : s.value.a;",
    );
    accepts("const s = await tools.mcpRepomod.reading({});\nreturn typeof s === \"string\" ? 0 : 1;");
}

#[test]
fn a_payload_is_not_read_before_a_tag_test_has_narrowed_it() {
    let head = "const r = (await tools.mcpRepomod.githubCachedCi({ repo: \"a\" })).result;\n";
    rejects("not-a-record", &format!("{head}return r.value;"));
    rejects("not-a-record", &format!("{head}if (r.tag === \"NotCached\") {{ return r.value.state; }}\nreturn 0;"));
    rejects("not-a-record", &format!("{head}if (r.tag === \"Cached\") {{ }}\nreturn r.value;"));
    rejects(
        "not-a-record",
        "let r = (await tools.mcpRepomod.githubCachedCi({ repo: \"a\" })).result;\nif (r.tag === \"Cached\") { r = (await tools.mcpRepomod.githubCachedCi({ repo: \"b\" })).result; return r.value.age; }\nreturn 0;",
    );
    let reading = "const s = await tools.mcpRepomod.reading({});\n";
    rejects("not-a-record", &format!("{reading}return s.tag;"));
    rejects("not-a-record", &format!("{reading}if (s.tag === \"Count\") {{ return 1; }}\nreturn s.value;"));
    rejects("js-equality", &format!("{reading}return s === \"Count\";"));
}
