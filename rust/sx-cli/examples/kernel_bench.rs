use std::fs;
use std::hint::black_box;
use std::process::ExitCode;

use sx_cores::kernel::{Cores, core};
use sx_kernel::budgets::Budgets;
use sx_kernel::checker::{check, compile_source};
use sx_kernel::host::{FixtureHost, run};
use sx_kernel::ir::Program;
use sx_kernel::json::{Json, parse};
use sx_kernel::machine::{CoreImplementations, Machine, Outcome, Response};
use sx_kernel::protocol::{CoreLibrary, declare};
use sx_kernel::reader::read;
use sx_kernel::authority::{Authority, Grants};
use sx_kernel::registry::{Core, Pack, Registry};
use sx_kernel::types::Type;
use sx_kernel::values::{Value, decode};

use std::collections::BTreeMap;

struct Library;

impl CoreLibrary for Library {
    fn core(&self, id: &str) -> Option<Core> {
        core(id)
    }

    fn pack(&self, id: &str) -> Option<Pack> {
        sx_packs::pack(id)
    }

    fn implementations(&self) -> &dyn CoreImplementations {
        &Cores
    }
}

struct Case {
    source: String,
    registry: Registry,
    declared: BTreeMap<String, Type>,
    grants: Grants,
    budgets: Budgets,
    inputs: Vec<(String, Value)>,
    responses: Json,
}

fn field<'a>(json: &'a Json, key: &str) -> &'a Json {
    json.get(key).unwrap_or_else(|| panic!("the bundle lacks {key}"))
}

fn case_of(json: &Json) -> Case {
    let declarations = field(json, "declarations");
    let declared = declare(declarations, &Library).expect("declarations load");
    let mut budgets = Budgets::default();
    for (name, value) in field(json, "budgets").as_object().expect("budgets object") {
        budgets.set(name, u64::try_from(value.as_i64().expect("budget count")).expect("budget count")).expect("budget");
    }
    let allow: Vec<&str> = field(json, "allow").as_array().expect("allow").iter().filter_map(Json::as_str).collect();
    let grants = Grants::unscoped(&declared.registry, &allow).expect("the allow list grants");
    let inputs = field(json, "inputs")
        .as_object()
        .expect("inputs object")
        .iter()
        .map(|(name, value)| {
            let t = declared.inputs.get(name).expect("declared input");
            (name.clone(), decode(value, t, "$").expect("input decodes"))
        })
        .collect();
    Case {
        source: field(json, "source").as_str().expect("source").to_string(),
        registry: declared.registry,
        declared: declared.inputs,
        grants,
        budgets,
        inputs,
        responses: field(json, "responses").clone(),
    }
}

fn cases(bundle: &Json, group: &str) -> Vec<Case> {
    let listed = match group {
        "corpus" | "status" => field(bundle, group).as_array().expect("case list").iter().map(case_of).collect(),
        other => vec![case_of(field(field(bundle, "synthetic"), other))],
    };
    assert!(!listed.is_empty(), "group {group} has no cases");
    listed
}

fn compiled(case: &Case) -> Program {
    let result = compile_source(&case.source, &case.registry, Authority::Granted(&case.grants), &case.declared, case.budgets);
    result.program.unwrap_or_else(|| panic!("a benchmark program was rejected: {}", case.source))
}

fn answer_all(program: &Program, case: &Case, cores: &dyn CoreImplementations) -> usize {
    let mut machine = Machine::new(program, &case.inputs, case.budgets, cores);
    let constant = Json::object([("id", Json::string("a")), ("size", Json::Int(1))]);
    let mut outcome = machine.start();
    let mut answered = 0;
    while let Outcome::Suspended(requests) = &outcome {
        let responses: Vec<(String, Response)> =
            requests.iter().map(|r| (r.id().to_string(), Response::Output(constant.clone()))).collect();
        answered += responses.len();
        outcome = machine.resume(responses);
    }
    assert!(matches!(outcome, Outcome::Completed { .. }), "a benchmark program did not complete");
    answered
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let [_, bundle_path, operation, count] = args.as_slice() else {
        eprintln!("usage: kernel_bench BUNDLE STAGE-GROUP ITERATIONS");
        return ExitCode::from(2);
    };
    let iterations: u64 = count.parse().expect("iteration count");
    let bundle = parse(&fs::read_to_string(bundle_path).expect("bundle readable")).expect("bundle is JSON");
    let Some((stage, group)) = operation.split_once('-') else {
        eprintln!("the operation is STAGE-GROUP, for example parse-corpus");
        return ExitCode::from(2);
    };
    let cases = cases(&bundle, group);
    match stage {
        "parse" => {
            for _ in 0..iterations {
                for case in &cases {
                    black_box(read(black_box(&case.source)));
                }
            }
        }
        "check" => {
            let parsed: Vec<_> = cases.iter().map(|c| read(&c.source).program.expect("parses")).collect();
            for _ in 0..iterations {
                for (case, node) in cases.iter().zip(&parsed) {
                    black_box(check(black_box(node), &case.registry, Authority::Granted(&case.grants), &case.declared, case.budgets));
                }
            }
        }
        "run" => {
            let programs: Vec<Program> = cases.iter().map(compiled).collect();
            if matches!(group, "corpus" | "status") {
                let mut hosts: Vec<FixtureHost<'_>> = programs
                    .iter()
                    .zip(&cases)
                    .map(|(p, c)| FixtureHost::from_responses(&p.signatures.capabilities, &c.responses).expect("responses"))
                    .collect();
                for _ in 0..iterations {
                    for ((program, case), host) in programs.iter().zip(&cases).zip(hosts.iter_mut()) {
                        for entry in &mut host.entries {
                            entry.served = 0;
                        }
                        host.delays.clear();
                        host.calls.clear();
                        black_box(run(program, &case.grants, host, &case.inputs, case.budgets, &Cores).expect("fixture answers"));
                    }
                }
            } else {
                for _ in 0..iterations {
                    for (program, case) in programs.iter().zip(&cases) {
                        black_box(answer_all(program, case, &Cores));
                    }
                }
            }
        }
        other => {
            eprintln!("unknown stage {other}");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}
