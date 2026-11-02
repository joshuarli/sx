use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use sx_kernel::json::{Json, Layout, dumps, parse};

static SERIAL: AtomicUsize = AtomicUsize::new(0);

const MANIFEST: &str = "# deployment manifest\nservices:\n  web:\n    image: nginx:1.25   # pinned\n    replicas: 2\n  worker:\n    image: \"busybox:1.36\"\n    replicas: 1\n";

struct Sandbox {
    base: PathBuf,
    root: PathBuf,
}

impl Sandbox {
    fn new(label: &str) -> Sandbox {
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("yaml-core-{}-{label}-{serial}", std::process::id()));
        if base.exists() {
            fs::remove_dir_all(&base).expect("old sandbox removed");
        }
        fs::create_dir_all(base.join("root")).expect("root");
        let base = fs::canonicalize(base).expect("canonical sandbox");
        let root = base.join("root");
        fs::write(base.join("declarations.json"), r#"{"cores": ["yaml"], "packs": ["fs"]}"#).expect("declarations");
        let scope = Json::object([("roots", Json::Array(vec![Json::string(text(&root))]))]);
        let grants = Json::Object(["fs.read-text", "fs.write-text"].iter().map(|id| ((*id).to_string(), scope.clone())).collect());
        fs::write(base.join("grants.json"), dumps(&grants, Layout::Spaced).expect("grants")).expect("grants file");
        fs::write(root.join("manifest.yaml"), MANIFEST).expect("manifest");
        Sandbox { base, root }
    }

    fn manifest(&self) -> String {
        text(&self.root.join("manifest.yaml")).to_string()
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
fn a_program_bumps_manifest_values_and_writes_the_file_back_untouched_elsewhere() {
    let s = Sandbox::new("bump");
    let path = s.manifest();
    let source = format!(
        "(let [text (call fs.read-text {{:path \"{path}\"}}) \
               names (yaml.keys text [\"services\"]) \
               bumped (reduce (fn [acc name] (yaml.set acc [\"services\" name \"replicas\"] \"5\")) text names) \
               tagged (yaml.set bumped [\"services\" \"web\" \"image\"] \"nginx:1.26\") \
               tagged (yaml.set tagged [\"services\" \"worker\" \"image\"] \"busybox:1.37\") \
               _ (call fs.write-text {{:path \"{path}\" :text tagged}})] \
           {{:names names :before (yaml.get text [\"services\" \"web\" \"replicas\"]) :after (yaml.get tagged [\"services\" \"web\" \"replicas\"]) :missing (yaml.get tagged [\"services\" \"db\" \"replicas\"])}})"
    );
    let ran = s.sx("run", &source);
    assert_eq!(ran.code, 0, "{ran:?}");
    let outcome = parse(&ran.stdout).expect("json");
    assert_eq!(
        dumps(outcome.get("result").expect("result"), Layout::Spaced).expect("json"),
        r#"{"names": ["web", "worker"], "before": "2", "after": "5", "missing": null}"#
    );
    assert_eq!(
        fs::read_to_string(&path).expect("manifest"),
        "# deployment manifest\nservices:\n  web:\n    image: nginx:1.26   # pinned\n    replicas: 5\n  worker:\n    image: \"busybox:1.37\"\n    replicas: 5\n"
    );
}

#[test]
fn text_outside_the_supported_subset_stops_the_program_with_a_reason() {
    let s = Sandbox::new("refused");
    fs::write(s.root.join("manifest.yaml"), "services:\n  web: {image: nginx}\n").expect("flow manifest");
    let path = s.manifest();
    let source = format!("(yaml.set (call fs.read-text {{:path \"{path}\"}}) [\"services\" \"web\" \"image\"] \"x\")");
    let ran = s.sx("run", &source);
    assert_eq!(ran.code, 2, "{ran:?}");
    let outcome = parse(&ran.stdout).expect("json");
    assert_eq!(outcome.get("outcome").and_then(Json::as_str), Some("failed"));
    let failure = outcome.get("failure").expect("failure");
    assert_eq!(failure.get("kind").and_then(Json::as_str), Some("arithmetic"));
    let message = failure.get("message").and_then(Json::as_str).expect("message");
    assert!(message.starts_with("yaml.set: line 2: flow collections are outside the supported YAML subset"), "{message}");
    assert_eq!(fs::read_to_string(&path).expect("manifest"), "services:\n  web: {image: nginx}\n");
}

#[test]
fn the_core_is_documented_by_help() {
    let s = Sandbox::new("help");
    let declarations = s.base.join("declarations.json");
    let help = run_sx(&["help", "yaml.set", "--declarations", text(&declarations)]);
    assert_eq!(help.code, 0, "{help:?}");
    assert!(help.stdout.contains("Replaces the scalar at a path and keeps every other byte"), "{}", help.stdout);
    let keys = run_sx(&["help", "yaml.keys", "--declarations", text(&declarations)]);
    assert!(keys.stdout.contains("yaml.keys"), "{keys:?}");
}
