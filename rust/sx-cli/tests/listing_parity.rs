use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sx_cores::kernel::declared_registry;
use sx_kernel::help::tool_listing;
use sx_kernel::json::{Json, parse};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn every_tool_listing_matches_python_for_the_tasks_that_are_not_fresh() {
    let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("listings.json");
    let status = Command::new("uv")
        .args(["run", "--quiet", "python", "rust/scripts/listing_reference.py"])
        .arg(&target)
        .env("PYTHONPATH", root().join("bench/codemode"))
        .current_dir(root())
        .status()
        .expect("uv runs the listing reference");
    assert!(status.success());
    let python = parse(&fs::read_to_string(&target).expect("listings")).expect("listings json");
    let listings = python.as_object().expect("listing map");
    assert!(listings.len() > 20);
    let mut failures = Vec::new();
    for (id, expected) in listings {
        let task = parse(&fs::read_to_string(root().join("bench/codemode/tasks").join(id).join("task.json")).expect("task"))
            .expect("task json");
        let registry = declared_registry(&task).expect("task registry");
        let capabilities: Vec<&str> = task
            .get("capabilities")
            .and_then(Json::as_array)
            .expect("capabilities")
            .iter()
            .filter_map(|c| c.get("id").and_then(Json::as_str))
            .collect();
        let cores: Vec<&str> = task
            .get("cores")
            .and_then(Json::as_array)
            .map(|ids| ids.iter().filter_map(Json::as_str).collect())
            .unwrap_or_default();
        let rust = tool_listing(&registry, &capabilities, &cores).expect("listing");
        if Some(rust.as_str()) != expected.as_str() {
            failures.push(id.clone());
        }
    }
    eprintln!("{} tool listings compared", listings.len());
    assert!(failures.is_empty(), "listings differ for {failures:?}");
}
