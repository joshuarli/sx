use std::fs;
use std::path::Path;

use sx_kernel::registry::Registry;

#[test]
fn a_capability_may_name_a_type_of_a_core_the_declarations_list() {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let declarations = directory.join("declared-cores.json");
    fs::write(
        &declarations,
        r#"{"cores":["git"],"capabilities":[{"id":"git.status","effect":"read","input":"Record {repo: String}","output":"git.Checkout","description":"Reads a checkout."}]}"#,
    )
    .expect("declarations written");
    let program = directory.join("declared-cores.sx");
    fs::write(&program, "(call git.status {:repo \"acme/tool\"})\n").expect("program written");
    let args: Vec<String> = ["check", program.to_str().expect("utf-8"), "--declarations", declarations.to_str().expect("utf-8"), "--allow", "git.status"]
        .iter()
        .map(|a| (*a).to_string())
        .collect();
    let exit = sx_cli::run(&args, &Registry::new);
    assert_eq!((exit.code, exit.stderr.as_str()), (0, ""), "{}", exit.stdout);
}
