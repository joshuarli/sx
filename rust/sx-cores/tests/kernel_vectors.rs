use std::fs;
use std::path::Path;

use sx_cores::kernel::{Cores, cores};
use sx_kernel::json::{Json, Layout, dumps, parse, python_equal};
use sx_kernel::machine::{CoreError, CoreImplementations};
use sx_kernel::registry::Registry;
use sx_kernel::values::{decode, encode};

fn registry() -> Registry {
    let mut registry = Registry::new();
    for core in cores() {
        registry.add_core(core).expect("each core registers");
    }
    registry
}

#[test]
fn every_core_vector_holds_through_the_kernel_values_and_signatures() {
    let registry = registry();
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/cores");
    let mut checked = 0;
    let mut failures = Vec::new();
    for id in ["text", "semver", "git", "github"] {
        let text = fs::read_to_string(directory.join(format!("{id}.json"))).expect("vector file");
        let file = parse(&text).expect("vector JSON");
        assert_eq!(file.get("core").and_then(Json::as_str), Some(id));
        let core = registry.core(id).expect("registered core");
        assert_eq!(file.get("version").and_then(Json::as_str), Some(core.version.as_str()));
        for vector in file.get("vectors").and_then(Json::as_array).expect("vectors") {
            checked += 1;
            let name = vector.get("function").and_then(Json::as_str).expect("function name");
            let function = registry.function(name).unwrap_or_else(|| panic!("{name} is registered"));
            let args = vector.get("args").and_then(Json::as_array).expect("args");
            assert_eq!(args.len(), function.params.len(), "{name} arity");
            let values: Vec<_> = args
                .iter()
                .zip(&function.params)
                .map(|(arg, t)| decode(arg, t, "$").unwrap_or_else(|e| panic!("{name}: {e}")))
                .collect();
            let outcome = Cores.apply(name, &values);
            let shown = || dumps(vector, Layout::Spaced).unwrap_or_default();
            match (vector.get("result"), outcome) {
                (Some(expected), Ok(value)) => {
                    let actual = encode(&value, &function.output);
                    if !python_equal(&actual, expected) {
                        failures.push(format!("{}: got {}", shown(), dumps(&actual, Layout::Spaced).unwrap_or_default()));
                    }
                }
                (None, Err(CoreError::Failed(_))) if vector.get("fails").is_some() => {}
                (_, other) => failures.push(format!("{}: got {other:?}", shown())),
            }
        }
    }
    eprintln!("{checked} core vectors hold through the kernel");
    assert!(checked > 1000, "only {checked} vectors");
    assert!(failures.is_empty(), "{} of {checked} vectors differ:\n{}", failures.len(), failures.join("\n"));
}
