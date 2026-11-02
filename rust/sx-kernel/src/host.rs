use std::collections::BTreeMap;
use std::fmt;

use crate::budgets::Budgets;
use crate::ir::{CapabilitySignature, Program};
use crate::json::{Json, python_equal, python_repr};
use crate::machine::{CoreImplementations, Machine, Outcome, Request, Response};
use crate::registry::Effect;
use crate::values::Value;

#[derive(Clone, Debug)]
pub enum FixtureResult {
    Output(Json),
    Error { kind: String, message: String },
}

#[derive(Clone, Debug)]
pub struct FixtureEntry {
    pub capability: String,
    pub input: Json,
    pub results: Vec<FixtureResult>,
    pub served: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixtureFormatError(pub String);

impl fmt::Display for FixtureFormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FixtureFormatError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnexpectedReason {
    NoFixture,
    Repeated,
    NoResults,
    UnknownCapability,
}

#[derive(Clone, Debug)]
pub struct UnexpectedCall {
    pub capability: String,
    pub input: Json,
    pub reason: UnexpectedReason,
}

impl fmt::Display for UnexpectedCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.reason {
            UnexpectedReason::NoFixture => "has no fixture for this input",
            UnexpectedReason::Repeated => "was called more often than the fixture allows",
            UnexpectedReason::NoResults => "has a fixture with no results",
            UnexpectedReason::UnknownCapability => "has no signature in the program",
        };
        write!(f, "{} {what}: {}", self.capability, python_repr(&self.input))
    }
}

impl std::error::Error for UnexpectedCall {}

fn format_error(what: &str) -> FixtureFormatError {
    FixtureFormatError(format!("fixture responses: {what}"))
}

pub fn parse_result(json: &Json) -> Result<FixtureResult, FixtureFormatError> {
    if let Some(error) = json.get("error") {
        let kind = error.get("kind").and_then(Json::as_str).ok_or_else(|| format_error("an error needs a kind"))?;
        let message =
            error.get("message").and_then(Json::as_str).ok_or_else(|| format_error("an error needs a message"))?;
        return Ok(FixtureResult::Error { kind: kind.to_string(), message: message.to_string() });
    }
    json.get("output").cloned().map(FixtureResult::Output).ok_or_else(|| format_error("a result needs an output or an error"))
}

impl FixtureResult {
    pub fn response(&self) -> Response {
        match self {
            FixtureResult::Output(json) => Response::Output(json.clone()),
            FixtureResult::Error { kind, message } => Response::Refusal { kind: kind.clone(), message: message.clone() },
        }
    }
}

pub struct FixtureHost<'p> {
    capabilities: &'p BTreeMap<String, CapabilitySignature>,
    pub entries: Vec<FixtureEntry>,
    pub delays: Vec<i64>,
    pub calls: Vec<Json>,
}

impl<'p> FixtureHost<'p> {
    pub fn new(capabilities: &'p BTreeMap<String, CapabilitySignature>) -> FixtureHost<'p> {
        FixtureHost { capabilities, entries: Vec::new(), delays: Vec::new(), calls: Vec::new() }
    }

    pub fn from_responses(
        capabilities: &'p BTreeMap<String, CapabilitySignature>,
        responses: &Json,
    ) -> Result<FixtureHost<'p>, FixtureFormatError> {
        let mut host = FixtureHost::new(capabilities);
        let listed = responses.as_array().ok_or_else(|| format_error("must be a JSON list"))?;
        for response in listed {
            let capability =
                response.get("capability").and_then(Json::as_str).ok_or_else(|| format_error("an entry needs a capability"))?;
            let input = response.get("input").cloned().ok_or_else(|| format_error("an entry needs an input"))?;
            let results = response
                .get("results")
                .and_then(Json::as_array)
                .ok_or_else(|| format_error("an entry needs a results list"))?
                .iter()
                .map(parse_result)
                .collect::<Result<_, _>>()?;
            host.entries.push(FixtureEntry { capability: capability.to_string(), input, results, served: 0 });
        }
        Ok(host)
    }

    pub fn answer(&mut self, request: &Request) -> Result<Response, UnexpectedCall> {
        let (capability, input) = match request {
            Request::Delay { milliseconds, .. } => {
                self.delays.push(*milliseconds);
                return Ok(Response::Output(Json::Null));
            }
            Request::Call { capability, input, .. } => (capability, input),
        };
        self.calls.push(Json::object([("capability", Json::string(capability.as_str())), ("input", input.clone())]));
        let unexpected =
            |reason: UnexpectedReason| UnexpectedCall { capability: capability.clone(), input: input.clone(), reason };
        let Some(entry) =
            self.entries.iter_mut().find(|e| e.capability == *capability && python_equal(&e.input, input))
        else {
            return Err(unexpected(UnexpectedReason::NoFixture));
        };
        let Some(signature) = self.capabilities.get(capability) else {
            return Err(unexpected(UnexpectedReason::UnknownCapability));
        };
        if signature.effect == Effect::Write && entry.served >= entry.results.len() {
            return Err(unexpected(UnexpectedReason::Repeated));
        }
        let Some(result) = entry.results.get(entry.served.min(entry.results.len().saturating_sub(1))) else {
            return Err(unexpected(UnexpectedReason::NoResults));
        };
        let response = result.response();
        entry.served += 1;
        Ok(response)
    }

    pub fn unperformed_writes(&self) -> Vec<&FixtureEntry> {
        self.entries
            .iter()
            .filter(|entry| {
                self.capabilities.get(&entry.capability).is_some_and(|s| s.effect == Effect::Write)
                    && entry.served != entry.results.len()
            })
            .collect()
    }
}

pub struct Run<'p> {
    pub outcome: Outcome,
    pub machine: Machine<'p>,
}

pub fn run<'p>(
    program: &'p Program,
    host: &mut FixtureHost<'_>,
    inputs: &[(String, Value)],
    budgets: Budgets,
    cores: &'p dyn CoreImplementations,
) -> Result<Run<'p>, UnexpectedCall> {
    let mut machine = Machine::new(program, inputs, budgets, cores);
    let mut outcome = machine.start();
    while let Outcome::Suspended(requests) = &outcome {
        let mut responses = Vec::with_capacity(requests.len());
        for request in requests {
            responses.push((request.id().to_string(), host.answer(request)?));
        }
        outcome = machine.resume(responses);
    }
    Ok(Run { outcome, machine })
}
