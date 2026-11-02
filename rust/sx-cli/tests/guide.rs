use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const SX: &str = env!("CARGO_BIN_EXE_sx");

fn here(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/guide").join(name).to_string_lossy().into_owned()
}

fn sx(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(SX).args(args).output().expect("sx starts");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn sections() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut found = Vec::new();
    for directory in ["sx-kernel/guide", "sx-packs/guide", "sx-cores/guide"] {
        let mut entries: Vec<PathBuf> = fs::read_dir(root.join(directory))
            .expect("guide directory")
            .map(|entry| entry.expect("guide entry").path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
            .collect();
        entries.sort();
        found.extend(entries);
    }
    found
}

fn sx_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        match current.as_mut() {
            None if trimmed == "```sx" => current = Some(Vec::new()),
            None => {}
            Some(lines) if trimmed == "```" => {
                blocks.push(lines.join("\n"));
                current = None;
            }
            Some(lines) => lines.push(line),
        }
    }
    assert!(current.is_none(), "an sx block is not closed");
    blocks
}

#[test]
fn every_guide_example_checks_against_the_tools_it_names() {
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("guide-{}", std::process::id()));
    fs::create_dir_all(&scratch).expect("scratch directory");
    let mut checked = 0;
    for section in sections() {
        let text = fs::read_to_string(&section).expect("guide section");
        for (index, block) in sx_blocks(&text).into_iter().enumerate() {
            let program = scratch.join(format!("{}-{index}.sx", section.file_stem().expect("stem").to_string_lossy()));
            fs::write(&program, &block).expect("program file");
            let program = program.to_string_lossy().into_owned();
            let (code, stdout, stderr) =
                sx(&["check", &program, "--declarations", &here("declarations.json"), "--grants", &here("grants.json")]);
            assert_eq!(code, 0, "{} example {index} does not check:\n{block}\n{stdout}{stderr}", section.display());
            checked += 1;
        }
    }
    assert!(checked >= 8, "only {checked} guide examples were found");
}

#[test]
fn the_guide_teaches_only_what_the_grants_allow() {
    let grants = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("guide-grants-{}.json", std::process::id()));
    fs::write(&grants, r#"{"fs.read-text": {"roots": ["/"]}, "ci.checks": {"origins": ["http://ci.test"], "methods": ["GET"]}}"#)
        .expect("grants file");
    let grants = grants.to_string_lossy().into_owned();
    let (code, guide, stderr) = sx(&["guide", "--declarations", &here("declarations.json"), "--grants", &grants]);
    assert_eq!(code, 0, "{stderr}");
    assert!(guide.contains("(call fs.read-text {:path \"...\"})"));
    assert!(guide.contains("(call ci.checks {:repo \"...\" :sha \"...\"})"));
    assert!(guide.contains("## Files: the fs pack"));
    assert!(guide.contains("## Web services"));
    for absent in ["### fs.write-text", "### process.run", "## Programs: the process pack", "## Databases", "### shop.item"] {
        assert!(!guide.contains(absent), "the guide shows {absent} though it is not granted");
    }
    let (code, notes, _) = sx(&["guide", "--declarations", &here("declarations.json"), "--grants", &grants, "--notes"]);
    assert_eq!(code, 0);
    assert!(notes.starts_with("### How the file tools behave"));
    assert!(notes.contains("### How the web service tools behave"));
    assert!(!notes.contains("```sx"), "the notes hold no Sx");
}

#[test]
fn the_guide_needs_declarations() {
    let (code, stdout, stderr) = sx(&["guide"]);
    assert_eq!((code, stdout.as_str()), (64, ""));
    assert_eq!(stderr, "sx guide: sx guide needs --declarations FILE\n");
}
