use std::collections::BTreeSet;

use sx_kernel::explain::{CallScope, ExplainedTool};
use sx_kernel::json::{Json, python_repr, repr_str};
use sx_kernel::registry::Registry;
use sx_kernel::syntax::Span;

use crate::commands::span_json;

const POLICY_KEYS: [&str; 3] = ["capabilities", "computed_inputs", "max_write_sites"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Policy {
    capabilities: BTreeSet<String>,
    computed_inputs: BTreeSet<String>,
    max_write_sites: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Site {
    pub capability: String,
    pub span: Span,
}

impl Site {
    fn text(&self) -> String {
        format!("{} at {}", self.capability, self.span)
    }

    fn json(&self) -> Json {
        Json::object([("capability", Json::string(self.capability.as_str())), ("span", span_json(Some(self.span)))])
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    CapabilityNotAllowed(Site),
    ComputedInputNotAllowed(Site),
    OutsideGrant { site: Site, verdict: String },
    TooManyWriteSites { limit: usize, sites: Vec<Site> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    Approved,
    Refused(Vec<Refusal>),
}

impl Refusal {
    pub fn kind(&self) -> &'static str {
        match self {
            Refusal::CapabilityNotAllowed(_) => "capability-not-allowed",
            Refusal::ComputedInputNotAllowed(_) => "computed-input-not-allowed",
            Refusal::OutsideGrant { .. } => "outside-grant",
            Refusal::TooManyWriteSites { .. } => "too-many-write-sites",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Refusal::CapabilityNotAllowed(site) => {
                format!("{}: the policy does not allow the program to call {}", site.text(), site.capability)
            }
            Refusal::ComputedInputNotAllowed(site) => format!(
                "{}: the input is computed at run time and checked per request, and the policy does not list {} in computed_inputs",
                site.text(),
                site.capability
            ),
            Refusal::OutsideGrant { site, verdict } => format!("{}: {verdict}", site.text()),
            Refusal::TooManyWriteSites { limit, sites } => {
                let listed: Vec<String> = sites.iter().map(Site::text).collect();
                format!(
                    "the program has {} write call sites ({}), and the policy allows {limit}",
                    sites.len(),
                    listed.join("; ")
                )
            }
        }
    }

    pub fn json(&self) -> Json {
        let detail = match self {
            Refusal::CapabilityNotAllowed(site) | Refusal::ComputedInputNotAllowed(site) | Refusal::OutsideGrant { site, .. } => {
                site.json()
            }
            Refusal::TooManyWriteSites { limit, sites } => Json::object([
                ("limit", Json::Int(i64::try_from(*limit).unwrap_or(i64::MAX))),
                ("sites", Json::Array(sites.iter().map(Site::json).collect())),
            ]),
        };
        let mut fields = vec![("kind".to_string(), Json::string(self.kind())), ("message".to_string(), Json::string(self.message()))];
        if let Json::Object(more) = detail {
            fields.extend(more);
        }
        Json::Object(fields)
    }
}

impl Verdict {
    pub fn json(&self) -> Json {
        match self {
            Verdict::Approved => Json::object([("verdict", Json::string("approved"))]),
            Verdict::Refused(refusals) => Json::object([
                ("verdict", Json::string("refused")),
                ("refusals", Json::Array(refusals.iter().map(Refusal::json).collect())),
            ]),
        }
    }

    pub fn text(&self) -> String {
        match self {
            Verdict::Approved => "policy: approved".to_string(),
            Verdict::Refused(refusals) => {
                let mut lines = vec!["policy: refused".to_string()];
                lines.extend(refusals.iter().map(|refusal| format!("  {}", refusal.message())));
                lines.join("\n")
            }
        }
    }
}

fn id_list(policy: &Json, key: &str, registry: &Registry) -> Result<Option<BTreeSet<String>>, String> {
    let Some(listed) = policy.get(key) else {
        return Ok(None);
    };
    let items = listed.as_array().ok_or_else(|| format!("{key} must be a list of capability ids, not {}", python_repr(listed)))?;
    let mut ids = BTreeSet::new();
    for item in items {
        let id = item.as_str().ok_or_else(|| format!("{key} must hold capability ids as strings, not {}", python_repr(item)))?;
        if registry.capability(id).is_none() {
            return Err(format!("{key} names {}, which the declarations do not define", repr_str(id)));
        }
        ids.insert(id.to_string());
    }
    Ok(Some(ids))
}

impl Policy {
    pub fn load(policy: &Json, registry: &Registry) -> Result<Policy, String> {
        let Json::Object(fields) = policy else {
            return Err("the policy must be a JSON object".to_string());
        };
        if let Some((key, _)) = fields.iter().find(|(key, _)| !POLICY_KEYS.contains(&key.as_str())) {
            return Err(format!("the policy has an unknown key {}; the keys are {}", repr_str(key), POLICY_KEYS.join(", ")));
        }
        let capabilities = id_list(policy, "capabilities", registry)?
            .ok_or_else(|| "the policy needs capabilities: the list of capabilities a program may call".to_string())?;
        let computed_inputs = id_list(policy, "computed_inputs", registry)?.unwrap_or_default();
        for id in &computed_inputs {
            if !capabilities.contains(id) {
                return Err(format!("computed_inputs names {}, which capabilities does not allow", repr_str(id)));
            }
            if registry.capability(id).is_some_and(|capability| capability.scope.is_none()) {
                return Err(format!(
                    "computed_inputs names {}, which declares no scope, so no call to it is ever checked per request",
                    repr_str(id)
                ));
            }
        }
        let max_write_sites = match policy.get("max_write_sites") {
            None => None,
            Some(Json::Int(limit)) => Some(
                usize::try_from(*limit).map_err(|_| format!("max_write_sites must not be negative, not {limit}"))?,
            ),
            Some(other) => return Err(format!("max_write_sites must be a whole number, not {}", python_repr(other))),
        };
        Ok(Policy { capabilities, computed_inputs, max_write_sites })
    }

    pub fn judge(&self, tools: &[ExplainedTool]) -> Verdict {
        let mut refusals = Vec::new();
        let mut writes = Vec::new();
        for tool in tools {
            for call in &tool.calls {
                let site = Site { capability: tool.id.clone(), span: call.span };
                if tool.writes {
                    writes.push(site.clone());
                }
                if !self.capabilities.contains(&tool.id) {
                    refusals.push(Refusal::CapabilityNotAllowed(site));
                    continue;
                }
                match &call.scope {
                    Some(CallScope::PerRequest) if !self.computed_inputs.contains(&tool.id) => {
                        refusals.push(Refusal::ComputedInputNotAllowed(site));
                    }
                    Some(scope @ (CallScope::Denied(_) | CallScope::NotGranted)) => {
                        refusals.push(Refusal::OutsideGrant { site, verdict: scope.text() });
                    }
                    Some(CallScope::PerRequest | CallScope::Allowed) | None => {}
                }
            }
        }
        if let Some(limit) = self.max_write_sites
            && writes.len() > limit
        {
            writes.sort_by_key(|site| (site.span.start.line, site.span.start.column));
            refusals.push(Refusal::TooManyWriteSites { limit, sites: writes });
        }
        refusals.sort_by_key(|refusal| match refusal {
            Refusal::CapabilityNotAllowed(site) | Refusal::ComputedInputNotAllowed(site) | Refusal::OutsideGrant { site, .. } => {
                (site.span.start.line, site.span.start.column)
            }
            Refusal::TooManyWriteSites { .. } => (usize::MAX, usize::MAX),
        });
        if refusals.is_empty() { Verdict::Approved } else { Verdict::Refused(refusals) }
    }

    pub fn describe(&self) -> String {
        let listed = |ids: &BTreeSet<String>| if ids.is_empty() { "none".to_string() } else { ids.iter().cloned().collect::<Vec<_>>().join(", ") };
        let limit = self.max_write_sites.map_or("no limit".to_string(), |limit| limit.to_string());
        format!(
            "## The approval policy\n\nBefore a program runs, the host judges it against a policy, and a program it refuses \
             performs nothing.\n\n- Tools the program may call: {}.\n- Tools that may be called with an input computed at run \
             time, which the checker cannot see: {}.\n- Write call sites allowed in the program text: {limit}.",
            listed(&self.capabilities),
            listed(&self.computed_inputs)
        )
    }
}
