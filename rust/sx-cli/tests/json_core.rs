use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use sx_kernel::json::{Json, Layout, dumps, parse};

static SERIAL: AtomicUsize = AtomicUsize::new(0);

const PACKAGE: &str = "{\n  \"name\": \"demo\",\n  \"version\": \"1.4.0\",\n  \"author\": {\"name\": \"Ada\"},\n  \"license\": \"MIT\",\n  \"dependencies\": {\"left-pad\": \"^1.3.0\", \"lodash\": \"4.17.21\"},\n  \"files\": [],\n  \"private\": false\n}\n";

struct Sandbox {
    base: PathBuf,
    root: PathBuf,
}

impl Sandbox {
    fn new(label: &str) -> Sandbox {
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("json-core-{}-{label}-{serial}", std::process::id()));
        if base.exists() {
            fs::remove_dir_all(&base).expect("old sandbox removed");
        }
        fs::create_dir_all(base.join("root")).expect("root");
        let base = fs::canonicalize(base).expect("canonical sandbox");
        let root = base.join("root");
        fs::write(base.join("declarations.json"), r#"{"cores": ["json"], "packs": ["fs"]}"#).expect("declarations");
        let scope = Json::object([("roots", Json::Array(vec![Json::string(text(&root))]))]);
        let grants = Json::Object(["fs.read-text"].iter().map(|id| ((*id).to_string(), scope.clone())).collect());
        fs::write(base.join("grants.json"), dumps(&grants, Layout::Spaced).expect("grants")).expect("grants file");
        fs::write(root.join("package.json"), PACKAGE).expect("package");
        Sandbox { base, root }
    }

    fn package(&self) -> String {
        text(&self.root.join("package.json")).to_string()
    }

    fn sx(&self, command: &str, source: &str) -> Ran {
        let program = self.base.join(format!("program-{}.sx", SERIAL.fetch_add(1, Ordering::Relaxed)));
        fs::write(&program, source).expect("program");
        let declarations = self.base.join("declarations.json");
        let grants = self.base.join("grants.json");
        run_sx(&[command, text(&program), "--declarations", text(&declarations), "--grants", text(&grants), "--format", "json"])
    }
}

fn text(path: &Path) -> &str {
    path.to_str().expect("test paths are UTF-8")
}

#[derive(Debug)]
struct Ran {
    code: i32,
    stdout: String,
}

fn run_sx(args: &[&str]) -> Ran {
    let output = Command::new(env!("CARGO_BIN_EXE_sx")).args(args).output().expect("sx runs");
    Ran {
        code: output.status.code().expect("exit status"),
        stdout: String::from_utf8(output.stdout).expect("utf-8"),
    }
}

#[test]
fn a_program_reads_a_package_manifest_and_branches_on_the_shape_of_a_field() {
    let s = Sandbox::new("manifest");
    let path = s.package();
    let source = format!(
        "(let [doc (call fs.read-text {{:path \"{path}\"}}) \
               author (match (json.kind doc [\"author\"]) \
                        String (json.get doc [\"author\"]) \
                        Object (json.get doc [\"author\" \"name\"]) \
                        _ none)] \
           {{:name (json.get doc [\"name\"]) \
             :author author \
             :deps (json.keys doc [\"dependencies\"]) \
             :range (json.get doc [\"dependencies\" \"left-pad\"]) \
             :files (json.kind doc [\"files\"]) \
             :bugs (json.kind doc [\"bugs\"]) \
             :private (json.get doc [\"private\"])}})"
    );
    let ran = s.sx("run", &source);
    assert_eq!(ran.code, 0, "{ran:?}");
    let outcome = parse(&ran.stdout).expect("json");
    assert_eq!(
        dumps(outcome.get("result").expect("result"), Layout::Spaced).expect("json"),
        r#"{"name": "demo", "author": "Ada", "deps": ["left-pad", "lodash"], "range": "^1.3.0", "files": "Array", "bugs": "Absent", "private": "false"}"#
    );
}

#[test]
fn text_that_is_not_json_stops_the_program_with_the_line_and_column() {
    let s = Sandbox::new("refused");
    fs::write(s.root.join("package.json"), "{\n  \"name\": \"demo\",\n}\n").expect("broken manifest");
    let path = s.package();
    let source = format!("(json.get (call fs.read-text {{:path \"{path}\"}}) [\"name\"])");
    let ran = s.sx("run", &source);
    assert_eq!(ran.code, 2, "{ran:?}");
    let outcome = parse(&ran.stdout).expect("json");
    let failure = outcome.get("failure").expect("failure");
    assert_eq!(failure.get("kind").and_then(Json::as_str), Some("invalid-input"));
    assert_eq!(
        failure.get("message").and_then(Json::as_str),
        Some("json.get: line 2 column 17: a trailing comma is not allowed")
    );
}

#[test]
fn the_core_is_documented_by_help() {
    let s = Sandbox::new("help");
    let declarations = s.base.join("declarations.json");
    let help = run_sx(&["help", "json.kind", "--declarations", text(&declarations)]);
    assert_eq!(help.code, 0, "{help:?}");
    assert!(help.stdout.contains("Names the shape of the value at a path"), "{}", help.stdout);
    let core = run_sx(&["help", "json", "--declarations", text(&declarations)]);
    assert!(core.stdout.contains("json.get") && core.stdout.contains("json.keys") && core.stdout.contains("json.Kind"), "{core:?}");
}
