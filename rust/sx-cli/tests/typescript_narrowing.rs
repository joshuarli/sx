use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const DECLARATIONS: &str = r#"{
  "cores": ["js"],
  "types": {"Item": "Record {id: String, qty: Int, note: Option<String>, parent: Option<Record {label: Option<String>}>}"},
  "capabilities": [
    {"id": "items.get", "effect": "read", "input": "Record {id: String}", "output": "Item", "description": "Reads one item."},
    {"id": "items.list", "effect": "read", "input": "Record {}", "output": "List<Item>", "description": "Lists items."}
  ]
}"#;

fn scratch(name: &str, contents: &str) -> String {
    let directory: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("typescript-narrowing-{}", std::process::id()));
    fs::create_dir_all(&directory).expect("scratch directory");
    let path = directory.join(name);
    fs::write(&path, contents).expect("scratch file");
    path.to_string_lossy().into_owned()
}

fn checks(source: &str, tag: &str) -> bool {
    let program = scratch(&format!("{tag}.ts"), source);
    let declarations = scratch("decl.json", DECLARATIONS);
    let status = Command::new(env!("CARGO_BIN_EXE_sx"))
        .args(["check", &program, "--declarations", &declarations, "--allow", "items.get", "--allow", "items.list"])
        .output()
        .expect("the sx binary runs")
        .status;
    status.success()
}

const ITEM: &str = "const it = await tools.items.get({ id: \"a\" });\nconst xs = await tools.items.list({});\n";

#[test]
fn an_optional_is_narrowed_wherever_javascript_has_proved_it_present() {
    let accepted = [
        "if (it.note == null) { return 0; }\nreturn it.note.length;",
        "if (!it.note) { return 0; }\nreturn it.note.length;",
        "if (it.note == null) { throw new Error(\"none\"); }\nreturn it.note.length;",
        "let n = 0;\nfor (const x of xs) {\n  if (x.note == null) { continue; }\n  n += x.note.length;\n}\nreturn n;",
        "let n = 0;\nfor (const x of xs) {\n  if (x.note == null) { break; }\n  n += x.note.length;\n}\nreturn n;",
        "const n = it.note;\nif (n == null || it.qty > 1) { return 1; }\nreturn n.length;",
        "const n = it.note;\nreturn n != null && n.length > 0;",
        "const n = it.note;\nreturn n == null || n.length > 0;",
        "return it.note != null && it.note.length > 0;",
        "return xs.filter((x) => x.note != null && x.note.length > 1).length;",
        "return xs.map((x) => (x.note ? x.note.length : -1));",
        "return it.note == null ? 0 : it.note.length;",
        "if (it.note == null) { return 0; } else { return it.note.length; }",
        "if (it.note) { return it.note.length; } else { return 0; }",
        "return it.note && it.note.length > 1 ? 1 : 0;",
        "const n = it.note;\nreturn n != null ? (n != null ? n.length : 1) : 2;",
        "const n = it.note;\nif (n != null) { if (n != null) { return n.length; } }\nreturn 0;",
        "const n = it.note;\nconst m = it.id;\nif (n != null && m != null) { return n.length + m.length; }\nreturn 0;",
        "if (it.parent != null && it.parent.label != null) { return it.parent.label.length; }\nreturn 0;",
        "if (it.parent == null || it.parent.label == null) { return 0; }\nreturn it.parent.label.length;",
        "const f = (s: string | null) => {\n  if (s == null) { return -1; }\n  return s.length;\n};\nreturn f(it.note);",
    ];
    for (index, body) in accepted.iter().enumerate() {
        let source = format!("{ITEM}{body}");
        assert!(checks(&source, &format!("accepted{index}")), "should check:\n{source}");
    }
}

#[test]
fn an_optional_stays_optional_where_javascript_has_not_proved_it_present() {
    let rejected = [
        "const n = it.note;\nif (n != null) { }\nreturn n.length;",
        "const n = it.note;\nreturn n ? 1 : n.length;",
        "const n = it.note;\nreturn n == null ? n.length : 0;",
        "const n = it.note;\nreturn n != null || n.length > 0;",
        "const n = it.note;\nif (n == null) { const z = 1; }\nreturn n.length;",
        "const n = it.note;\nif (n != null && it.qty > 1) { return 1; }\nreturn n.length;",
        "for (const x of xs) {\n  if (x.note == null) { x.id; }\n  return x.note.length;\n}\nreturn 0;",
        "let n = it.note;\nif (n != null) { n = null; return n.length; }\nreturn 0;",
        "let r = { note: it.note };\nif (r.note != null) { r.note = null; return r.note.length; }\nreturn 0;",
        "const other = xs[0];\nreturn it.note != null ? other.note.length : 0;",
        "if (it.note != null) { }\nreturn it.note.length;",
        "return it.parent != null ? it.parent.label.length : 0;",
    ];
    for (index, body) in rejected.iter().enumerate() {
        let source = format!("{ITEM}{body}");
        assert!(!checks(&source, &format!("rejected{index}")), "should not check:\n{source}");
    }
}
