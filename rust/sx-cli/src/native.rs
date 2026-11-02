use std::thread;
use std::time::Duration;

use sx_cores::kernel::Cores;
use sx_kernel::authority::{Grant, Grants};
use sx_kernel::budgets::Budgets;
use sx_kernel::host::refusal;
use sx_kernel::ir::Program;
use sx_kernel::json::Json;
use sx_kernel::machine::{Machine, Outcome, Request, Response};
use sx_kernel::types::CallErrorKind;
use sx_kernel::values::{Value, decode, encode};
use sx_packs::performer;

pub fn unimplemented(program: &Program) -> Vec<&str> {
    program.capabilities.iter().map(String::as_str).filter(|id| performer(id).is_none()).collect()
}

fn failed(message: String) -> Response {
    Response::Refusal { kind: CallErrorKind::Failed.name().to_string(), message }
}

fn perform(program: &Program, grants: &Grants, capability: &str, input: &Json) -> Response {
    if let Some(refused) = refusal(program, grants, capability, input) {
        return refused;
    }
    let (Some(signature), Some(perform)) = (program.signatures.capabilities.get(capability), performer(capability)) else {
        return failed(format!("{capability} has no native implementation"));
    };
    let scope = match grants.get(capability) {
        Some(Grant::Scoped { scope, .. }) => scope.clone(),
        _ => Value::Unit,
    };
    let value = match decode(input, &signature.input, "$") {
        Ok(value) => value,
        Err(error) => return failed(format!("{capability} received an input that does not match its type: {error}")),
    };
    match perform(&scope, &value) {
        Ok(output) => Response::Output(encode(&output, &signature.output)),
        Err(failure) => Response::Refusal { kind: failure.kind.name().to_string(), message: failure.message },
    }
}

fn answer(program: &Program, grants: &Grants, request: &Request) -> Response {
    match request {
        Request::Delay { milliseconds, .. } => {
            thread::sleep(Duration::from_millis(u64::try_from(*milliseconds).unwrap_or(0)));
            Response::Output(Json::Null)
        }
        Request::Call { capability, input, .. } => perform(program, grants, capability, input),
    }
}

pub fn run<'p>(program: &'p Program, grants: &Grants, values: &[(String, Value)], budgets: Budgets) -> (Outcome, Machine<'p>) {
    let mut machine = Machine::new(program, values, budgets, &Cores);
    let mut outcome = machine.start();
    while let Outcome::Suspended(requests) = &outcome {
        let responses: Vec<(String, Response)> = thread::scope(|scope| {
            let running: Vec<_> = requests
                .iter()
                .map(|request| (request.id().to_string(), scope.spawn(|| answer(program, grants, request))))
                .collect();
            running
                .into_iter()
                .map(|(id, handle)| {
                    let response = handle
                        .join()
                        .unwrap_or_else(|_| failed(format!("performing {id} panicked")));
                    (id, response)
                })
                .collect()
        });
        outcome = machine.resume(responses);
    }
    (outcome, machine)
}
