mod common;

use sx_ts::declarations::declarations;
use sx_ts::terms::rewrite;

#[test]
fn tool_ids_in_a_message_are_written_as_calls() {
    let registry = common::registry();
    let cases = [
        (
            "This program may not call fs.read-text. The host allows: mcp-github.list-issues, jobs.status.",
            "This program may not call tools.fs.readText. The host allows: tools.mcpGithub.listIssues, tools.jobs.status.",
        ),
        ("a String, but an Int is needed; see fs.nothing", "a String, but an Int is needed; see fs.nothing"),
    ];
    for (found, expected) in cases {
        assert_eq!(rewrite(found, &registry), expected);
    }
}

#[test]
fn types_in_a_checked_program_are_written_as_typescript() {
    let registry = common::registry();
    let source = "const xs = [1, 2];\nconst ys: string[] = xs;\nconst t = await tools.jobs.status({ id: \"a\" });\nreturn t.pct + t;";
    let found = sx_ts::compile_source(source, &registry, sx_kernel::authority::Authority::Unrestricted, &Default::default(), Default::default());
    let text = found.diagnostics.iter().map(|d| format!("{} {}", d.message, d.detail.clone().unwrap_or_default())).collect::<Vec<_>>().join("\n");
    assert!(text.contains("{ state: string; pct: Int }") && !text.contains("Record {"), "{text}");
    assert_eq!(rewrite("This record has no field :nope.\nFields: :state, :pct", &registry), "This record has no field nope.\nFields: state, pct");
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
