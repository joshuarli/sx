mod common;

use sx_ts::declarations::declarations;
use sx_ts::terms::rewrite;

#[test]
fn types_and_tools_are_written_in_typescript_terms() {
    let registry = common::registry();
    let cases = [
        ("Expected List<Option<Int>>, found Map<String, Float>", "Expected (Int | null)[], found Record<string, Float>"),
        ("Found Record {a: Int, b: Option<String>}", "Found { a: Int; b: string | null }"),
        ("Expected Tuple<Bool, Unit>, found Result<Int, String>", "Expected [boolean, null], found Settled<Int, string>"),
        ("This program may not call fs.read-text. The host allows: mcp-github.list-issues", "This program may not call tools.fs.readText. The host allows: tools.mcpGithub.listIssues"),
        ("a String, but an Int is needed", "a String, but an Int is needed"),
    ];
    for (found, expected) in cases {
        assert_eq!(rewrite(found, &registry), expected);
    }
}

#[test]
fn the_declarations_list_granted_tools_with_their_effects_and_types() {
    let registry = common::registry();
    let text = declarations(&registry, &|id| id != "fs.write-text");
    for expected in [
        "type Int = number;",
        "    /** A tool. [read] */\n    readText(input: { path: string }): Promise<string>;",
        "list(input: { path: string }): Promise<{ name: string; size: Int }[]>;",
        "  mcpGithub: {",
        "listIssues(input: { repo: string; cursor?: string | null }): Promise<{ items: { id: Int; title: string }[]; next: string | null }>;",
        "  jobs: {",
        "declare const tools: Tools;",
    ] {
        assert!(text.contains(expected), "{expected} missing from\n{text}");
    }
    assert!(!text.contains("writeText") && !text.contains("Settled"), "{text}");
}
