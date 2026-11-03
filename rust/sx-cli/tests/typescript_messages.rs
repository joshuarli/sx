use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sx_kernel::json::{Json, parse};

const DECLARATIONS: &str = r#"{
  "cores": ["js"],
  "types": {"Item": "Record {id: String, name: String, qty: Int, price: Float, note: Option<String>}"},
  "enums": {"store.Ci": {"NotCached": "String", "Cached": "Record {age: Int}"}},
  "capabilities": [
    {"id": "items.get", "effect": "read", "input": "Record {id: String}", "output": "Item", "description": "Reads one item."},
    {"id": "items.list", "effect": "read", "input": "Record {}", "output": "List<Item>", "description": "Lists items."},
    {"id": "items.count", "effect": "read", "input": "Record {}", "output": "Int", "description": "Counts items."},
    {"id": "store.ci", "effect": "read", "input": "Record {}", "output": "store.Ci", "description": "Cached CI."}
  ]
}"#;

const ALLOWED: [&str; 8] = ["--allow", "items.get", "--allow", "items.list", "--allow", "items.count", "--allow", "store.ci"];

const SX_WORDS: [&str; 22] = [
    "(match", "(some", "(get ", "(let [", "(fn [", "(recur", "(when", "(call", "(loop", "(sort-by", "(float", "(round", "(floor", "(keep",
    "js.", "recur", "sort-by", "Sx ", " get ", "[:", "{:", "None",
];

fn scratch(name: &str, contents: &str) -> String {
    let directory: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("typescript-messages-{}", std::process::id()));
    fs::create_dir_all(&directory).expect("scratch directory");
    let path = directory.join(name);
    fs::write(&path, contents).expect("scratch file");
    path.to_string_lossy().into_owned()
}

fn diagnostics(source: &str, tag: &str) -> Vec<Json> {
    let program = scratch(&format!("{tag}.ts"), source);
    let declarations = scratch(&format!("{tag}-decl.json"), DECLARATIONS);
    let output = Command::new(env!("CARGO_BIN_EXE_sx"))
        .args(["check", &program, "--format", "json", "--declarations", &declarations])
        .args(ALLOWED)
        .output()
        .expect("the sx binary runs");
    let found = parse(&String::from_utf8(output.stdout).expect("utf-8")).expect("json");
    match found.get("diagnostics") {
        Some(Json::Array(items)) => items.to_vec(),
        _ => Vec::new(),
    }
}

fn texts(diagnostic: &Json) -> Vec<String> {
    ["message", "detail", "fix"]
        .iter()
        .filter_map(|key| match diagnostic.get(key) {
            Some(Json::String(text)) => Some(text.to_string()),
            _ => None,
        })
        .collect()
}

const ITEM: &str = "const it = await tools.items.get({ id: \"a\" });\n";

#[test]
fn no_checker_message_speaks_sx_to_a_typescript_program() {
    let programs = [
        "return it.note.length;",
        "return it.note + 1;",
        "return it.note.foo;",
        "return it.nope;",
        "return it.qty.x;",
        "return it.id < it.qty;",
        "return it.note < 3;",
        "return [it, it].map((x) => x.note.length);",
        "const c = it.qty > 1;\nreturn c ? it.note : it.qty;",
        "return await loop({ max: 3 }, { n: 0 }, ({ n }) => next({ n: n + 1 }));",
        "const xs = await tools.items.list({});\nreturn xs.sort((a, b) => a.note - b.note);",
        "const xs = await tools.items.list({});\nreturn xs.sort((a, b) => a.qty - b.price);",
        "const t = [1, \"a\"];\nconst i = await tools.items.count({});\nreturn t[i];",
        "const m = {};\nfor (const k of [\"a\"]) { m[k] = 1; }\nreturn m.a.b;",
        "return [1, 2] === [1, 2];",
        "const n = await tools.items.count({});\nreturn n.length;",
        "const n = await tools.items.count({});\nreturn n.x;",
        "return `a${[1]}`;",
        "const s = (await tools.store.ci({}));\nreturn s.value;",
        "const s = (await tools.store.ci({}));\nreturn s.tag === \"NotCached\" ? 0 : s.value.nope;",
        "const x = it.note ? it.note.length : it.qty.f;\nreturn x;",
    ];
    for (index, program) in programs.iter().enumerate() {
        let source = if program.contains("const it") || !program.contains("it.") { program.to_string() } else { format!("{ITEM}{program}") };
        let found = diagnostics(&source, &format!("p{index}"));
        assert!(!found.is_empty(), "program {index} should be rejected:\n{source}");
        for diagnostic in &found {
            for text in texts(diagnostic) {
                for word in SX_WORDS {
                    assert!(!text.contains(word), "{word:?} in {text:?} for:\n{source}");
                }
            }
        }
    }
}

#[test]
fn reading_a_field_of_an_optional_says_how_to_test_it_in_typescript() {
    let found = diagnostics(&format!("{ITEM}return it.note.foo;"), "optional");
    let text: Vec<String> = found.iter().flat_map(texts).collect();
    let joined = text.join("\n");
    assert!(joined.contains("r.f != null") && joined.contains("r.f ?? d"), "{joined}");
}

#[test]
fn reading_the_payload_of_an_untested_enum_says_to_test_the_tag() {
    let found = diagnostics("const s = await tools.store.ci({});\nreturn s.value;", "variants");
    let joined = found.iter().flat_map(texts).collect::<Vec<_>>().join("\n");
    assert!(joined.contains("x.tag === \"NotCached\""), "{joined}");
}
