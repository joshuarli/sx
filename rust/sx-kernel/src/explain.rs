use crate::authority::{Grants, Screen, Verdict};
use crate::help::effect_label;
use crate::ir::{Literal, Program, call_sites};
use crate::json::Json;
use crate::registry::{Effect, Registry};
use crate::syntax::Span;

pub const RUNTIME_INPUT: &str = "computed at run time";
pub const PER_REQUEST: &str = "checked per request";
pub const NOT_GRANTED: &str = "not granted";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallScope {
    Allowed,
    Denied(String),
    PerRequest,
    NotGranted,
}

impl CallScope {
    pub fn text(&self) -> String {
        match self {
            CallScope::Allowed => "allowed".to_string(),
            CallScope::Denied(reason) => format!("denied: {reason}"),
            CallScope::PerRequest => PER_REQUEST.to_string(),
            CallScope::NotGranted => NOT_GRANTED.to_string(),
        }
    }

    pub fn json(&self) -> Json {
        match self {
            CallScope::Denied(reason) => {
                Json::object([("verdict", Json::string("denied")), ("reason", Json::string(reason.as_str()))])
            }
            CallScope::Allowed => Json::object([("verdict", Json::string("allowed"))]),
            CallScope::PerRequest => Json::object([("verdict", Json::string(PER_REQUEST))]),
            CallScope::NotGranted => Json::object([("verdict", Json::string(NOT_GRANTED))]),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExplainedCall {
    pub span: Span,
    pub input: Option<Literal>,
    pub scope: Option<CallScope>,
}

impl ExplainedCall {
    pub fn input_json(&self) -> Json {
        self.input.as_ref().map_or_else(|| Json::string(RUNTIME_INPUT), Literal::json)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExplainedTool {
    pub id: String,
    pub effect: &'static str,
    pub writes: bool,
    pub allowed: bool,
    pub scoped: bool,
    pub calls: Vec<ExplainedCall>,
}

impl ExplainedTool {
    pub fn json(&self) -> Json {
        let mut fields = vec![
            ("id".to_string(), Json::string(self.id.as_str())),
            ("effect".to_string(), Json::string(self.effect)),
            ("allowed".to_string(), Json::Bool(self.allowed)),
            ("inputs".to_string(), Json::Array(self.calls.iter().map(ExplainedCall::input_json).collect())),
        ];
        if self.scoped {
            let verdicts = self.calls.iter().filter_map(|call| call.scope.as_ref().map(CallScope::json)).collect();
            fields.push(("verdicts".to_string(), Json::Array(verdicts)));
        }
        Json::Object(fields)
    }
}

fn call_scope(grants: &Grants, id: &str, input: Option<&Literal>) -> CallScope {
    if !grants.contains(id) {
        return CallScope::NotGranted;
    }
    let Some(input) = input else {
        return CallScope::PerRequest;
    };
    match grants.screen(id, &input.value()) {
        Screen::Verdict(Verdict::Allowed) | Screen::Unscoped => CallScope::Allowed,
        Screen::Verdict(Verdict::Denied(reason)) => CallScope::Denied(reason),
        Screen::Unchecked => CallScope::PerRequest,
        Screen::NotGranted => CallScope::NotGranted,
    }
}

pub fn explain_tools(program: &Program, registry: &Registry, grants: &Grants) -> Result<Vec<ExplainedTool>, String> {
    let sites = call_sites(&program.body);
    let mut ids = program.capabilities.clone();
    ids.sort();
    let mut tools = Vec::new();
    for id in ids {
        let signature =
            program.signatures.capabilities.get(&id).ok_or_else(|| format!("the program calls {id} without a signature"))?;
        let scoped = registry.capability(&id).is_some_and(|c| c.scope.is_some());
        let calls = sites
            .iter()
            .filter(|site| site.capability == id)
            .map(|site| ExplainedCall {
                span: site.span,
                scope: scoped.then(|| call_scope(grants, &id, site.input.as_ref())),
                input: site.input.clone(),
            })
            .collect();
        tools.push(ExplainedTool {
            effect: effect_label(signature.effect, signature.idempotent),
            writes: signature.effect == Effect::Write,
            allowed: grants.contains(&id),
            scoped,
            calls,
            id,
        });
    }
    Ok(tools)
}
