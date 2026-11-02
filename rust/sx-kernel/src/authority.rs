use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use crate::json::{Json, python_repr};
use crate::registry::Registry;
use crate::types::show_type;
use crate::values::{Value, decode};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    Denied(String),
}

type PrepareScope = dyn Fn(&Value) -> Result<Value, String> + Send + Sync;
type CheckScope = dyn Fn(&Value, &Value) -> Verdict + Send + Sync;
type ResolveScope = dyn Fn(&Json) -> Result<Json, String> + Send + Sync;
type ShowScope = dyn Fn(&Value) -> Value + Send + Sync;

#[derive(Clone)]
pub struct ScopeCheck {
    resolve: Option<Arc<ResolveScope>>,
    show: Option<Arc<ShowScope>>,
    prepare: Arc<PrepareScope>,
    check: Arc<CheckScope>,
}

impl ScopeCheck {
    pub fn new(
        prepare: impl Fn(&Value) -> Result<Value, String> + Send + Sync + 'static,
        check: impl Fn(&Value, &Value) -> Verdict + Send + Sync + 'static,
    ) -> ScopeCheck {
        ScopeCheck { resolve: None, show: None, prepare: Arc::new(prepare), check: Arc::new(check) }
    }

    pub fn resolving(
        mut self,
        resolve: impl Fn(&Json) -> Result<Json, String> + Send + Sync + 'static,
    ) -> ScopeCheck {
        self.resolve = Some(Arc::new(resolve));
        self
    }

    pub fn showing(mut self, show: impl Fn(&Value) -> Value + Send + Sync + 'static) -> ScopeCheck {
        self.show = Some(Arc::new(show));
        self
    }

    pub fn shown(&self, scope: &Value) -> Value {
        match &self.show {
            Some(show) => show(scope),
            None => scope.clone(),
        }
    }

    pub fn resolve(&self, written: &Json) -> Result<Json, String> {
        match &self.resolve {
            Some(resolve) => resolve(written),
            None => Ok(written.clone()),
        }
    }

    pub fn prepare(&self, scope: &Value) -> Result<Value, String> {
        (self.prepare)(scope)
    }

    pub fn check(&self, scope: &Value, input: &Value) -> Verdict {
        (self.check)(scope, input)
    }
}

impl fmt::Debug for ScopeCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ScopeCheck")
    }
}

#[derive(Clone, Debug)]
pub enum Grant {
    Unscoped,
    Scoped { scope: Value, check: Option<ScopeCheck> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    NotGranted,
    Unscoped,
    Verdict(Verdict),
    Unchecked,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GrantError {
    NotAnObject(String),
    UnknownCapability(String),
    MissingScope { capability: String, scope: String },
    UnexpectedScope(String),
    InvalidScope { capability: String, message: String },
}

impl fmt::Display for GrantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GrantError::NotAnObject(found) => {
                write!(f, "grants must be a JSON object mapping capability IDs to scopes, not {found}")
            }
            GrantError::UnknownCapability(id) => write!(f, "no capability is named {id}"),
            GrantError::MissingScope { capability, scope } => {
                write!(f, "{capability} declares a scope of type {scope}, so its grant needs a scope value")
            }
            GrantError::UnexpectedScope(id) => write!(f, "{id} declares no scope, so its grant must be null"),
            GrantError::InvalidScope { capability, message } => write!(f, "the scope granted to {capability}: {message}"),
        }
    }
}

impl std::error::Error for GrantError {}

#[derive(Clone, Debug, Default)]
pub struct Grants {
    entries: BTreeMap<String, Grant>,
}

impl Grants {
    pub fn new() -> Grants {
        Grants::default()
    }

    pub fn unscoped<S: AsRef<str>>(registry: &Registry, ids: &[S]) -> Result<Grants, GrantError> {
        let mut grants = Grants::new();
        for id in ids {
            grants.grant_unscoped(registry, id.as_ref())?;
        }
        Ok(grants)
    }

    pub fn load(registry: &Registry, json: &Json) -> Result<Grants, GrantError> {
        let Json::Object(entries) = json else {
            return Err(GrantError::NotAnObject(python_repr(json)));
        };
        let mut grants = Grants::new();
        for (id, scope) in entries {
            grants.grant(registry, id, scope)?;
        }
        Ok(grants)
    }

    pub fn grant_unscoped(&mut self, registry: &Registry, id: &str) -> Result<(), GrantError> {
        self.grant(registry, id, &Json::Null)
    }

    fn grant(&mut self, registry: &Registry, id: &str, scope: &Json) -> Result<(), GrantError> {
        let capability = registry.capability(id).ok_or_else(|| GrantError::UnknownCapability(id.to_string()))?;
        let grant = match (&capability.scope, scope) {
            (None, Json::Null) => Grant::Unscoped,
            (None, _) => return Err(GrantError::UnexpectedScope(id.to_string())),
            (Some(scope_type), Json::Null) => {
                return Err(GrantError::MissingScope { capability: id.to_string(), scope: show_type(scope_type) });
            }
            (Some(scope_type), given) => {
                let invalid = |message: String| GrantError::InvalidScope { capability: id.to_string(), message };
                let check = registry.scope_check(id);
                let resolved = match &check {
                    Some(check) => check.resolve(given).map_err(invalid)?,
                    None => given.clone(),
                };
                let decoded = decode(&resolved, scope_type, "$").map_err(|e| invalid(e.to_string()))?;
                let scope = match &check {
                    Some(check) => check.prepare(&decoded).map_err(invalid)?,
                    None => decoded,
                };
                Grant::Scoped { scope, check }
            }
        };
        self.entries.insert(id.to_string(), grant);
        Ok(())
    }

    pub fn contains(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    pub fn get(&self, id: &str) -> Option<&Grant> {
        self.entries.get(id)
    }

    pub fn screen(&self, id: &str, input: &Value) -> Screen {
        match self.entries.get(id) {
            None => Screen::NotGranted,
            Some(Grant::Unscoped) => Screen::Unscoped,
            Some(Grant::Scoped { check: None, .. }) => Screen::Unchecked,
            Some(Grant::Scoped { scope, check: Some(check) }) => Screen::Verdict(check.check(scope, input)),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Authority<'g> {
    Granted(&'g Grants),
    Unrestricted,
}

impl Authority<'_> {
    pub fn allows(&self, id: &str) -> bool {
        match self {
            Authority::Granted(grants) => grants.contains(id),
            Authority::Unrestricted => true,
        }
    }
}
