use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use crate::json::{Json, python_repr, repr_str};
use crate::types::{EnumType, TYPE_NESTING_LIMIT, Type, TypeSyntaxError, parse_type, type_depth};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Effect {
    Read,
    Write,
}

impl Effect {
    pub fn name(self) -> &'static str {
        match self {
            Effect::Read => "read",
            Effect::Write => "write",
        }
    }

    pub fn from_name(name: &str) -> Option<Effect> {
        match name {
            "read" => Some(Effect::Read),
            "write" => Some(Effect::Write),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Example {
    pub program: String,
    pub result: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    pub name: String,
    pub params: Vec<Type>,
    pub output: Type,
    pub description: String,
    pub example: Option<Example>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Capability {
    pub id: String,
    pub effect: Effect,
    pub input: Type,
    pub output: Type,
    pub description: String,
    pub idempotent: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Assurance {
    Unverified,
    ConformanceTested,
    Specified,
    ProvedProperties,
}

impl Assurance {
    pub const ALL: [Assurance; 4] =
        [Assurance::Unverified, Assurance::ConformanceTested, Assurance::Specified, Assurance::ProvedProperties];

    pub fn name(self) -> &'static str {
        match self {
            Assurance::Unverified => "unverified",
            Assurance::ConformanceTested => "conformance-tested",
            Assurance::Specified => "specified",
            Assurance::ProvedProperties => "proved-properties",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evidence {
    pub level: Assurance,
    pub references: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Core {
    pub id: String,
    pub version: String,
    pub functions: Vec<Function>,
    pub types: Vec<(String, Type)>,
    pub evidence: Vec<Evidence>,
}

impl Core {
    pub fn assurance(&self) -> Vec<(Assurance, Vec<String>)> {
        let mut levels: Vec<(Assurance, Vec<String>)> = vec![(Assurance::Unverified, Vec::new())];
        for evidence in &self.evidence {
            match levels.iter_mut().find(|(level, _)| *level == evidence.level) {
                Some(entry) => entry.1 = evidence.references.clone(),
                None => levels.push((evidence.level, evidence.references.clone())),
            }
        }
        levels
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryError(pub String);

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RegistryError {}

impl From<TypeSyntaxError> for RegistryError {
    fn from(error: TypeSyntaxError) -> RegistryError {
        RegistryError(error.0)
    }
}

#[derive(Clone, Debug)]
struct Ordered<T> {
    order: Vec<String>,
    entries: BTreeMap<String, T>,
}

impl<T> Default for Ordered<T> {
    fn default() -> Ordered<T> {
        Ordered { order: Vec::new(), entries: BTreeMap::new() }
    }
}

impl<T> Ordered<T> {
    fn get(&self, name: &str) -> Option<&T> {
        self.entries.get(name)
    }

    fn insert(&mut self, name: &str, value: T) {
        if self.entries.insert(name.to_string(), value).is_none() {
            self.order.push(name.to_string());
        }
    }

    fn iter(&self) -> impl Iterator<Item = (&str, &T)> {
        self.order.iter().filter_map(|name| self.entries.get(name).map(|value| (name.as_str(), value)))
    }
}

#[derive(Clone, Debug, Default)]
pub struct Registry {
    functions: Ordered<Function>,
    capabilities: Ordered<Capability>,
    types: Ordered<Type>,
    cores: Ordered<Core>,
}

fn is_word(text: &str) -> bool {
    let mut first = true;
    for part in text.split('-') {
        let mut chars = part.chars();
        match chars.next() {
            Some(c) if first && c.is_ascii_lowercase() => {}
            Some(c) if !first && (c.is_ascii_lowercase() || c.is_ascii_digit()) => {}
            _ => return false,
        }
        if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) {
            return false;
        }
        first = false;
    }
    true
}

fn is_core_id(text: &str) -> bool {
    is_word(text)
}

fn is_member(text: &str) -> bool {
    is_word(text.strip_suffix('?').unwrap_or(text))
}

fn partition(text: &str) -> (&str, &str) {
    text.split_once('.').unwrap_or((text, ""))
}

impl Registry {
    pub fn new() -> Registry {
        Registry::default()
    }

    pub fn add_core(&mut self, core: Core) -> Result<(), RegistryError> {
        let id = repr_str(&core.id);
        if !is_core_id(&core.id) {
            return Err(RegistryError(format!("core id {id} is not lowercase words joined by -")));
        }
        if self.cores.get(&core.id).is_some() {
            return Err(RegistryError(format!("core {id} is already registered")));
        }
        check_evidence(&core)?;
        for (name, t) in &core.types {
            self.add_owned_type(name, t.clone(), Some(&core.id))?;
        }
        for function in &core.functions {
            let (namespace, member) = partition(&function.name);
            let name = repr_str(&function.name);
            if namespace != core.id || !is_member(member) {
                return Err(RegistryError(format!("function {name} must be named {}.NAME", core.id)));
            }
            if self.functions.get(&function.name).is_some() {
                return Err(RegistryError(format!("function {name} is already registered")));
            }
            if python_strip(&function.description).is_empty() {
                return Err(RegistryError(format!("function {name} needs a one-line description")));
            }
            if function.example.is_none() {
                return Err(RegistryError(format!("function {name} needs an example with its result")));
            }
            self.functions.insert(&function.name, function.clone());
        }
        let core_id = core.id.clone();
        self.cores.insert(&core_id, core);
        Ok(())
    }

    pub fn add_capability(&mut self, capability: Capability) -> Result<(), RegistryError> {
        let (namespace, member) = partition(&capability.id);
        let id = repr_str(&capability.id);
        if !is_core_id(namespace) || !is_member(member) {
            return Err(RegistryError(format!("capability id {id} must be NAMESPACE.NAME")));
        }
        if self.capabilities.get(&capability.id).is_some() {
            return Err(RegistryError(format!("capability {id} is already registered")));
        }
        if python_strip(&capability.description).is_empty() {
            return Err(RegistryError(format!("capability {id} needs a one-line description")));
        }
        if capability.effect == Effect::Read && capability.idempotent {
            return Err(RegistryError(format!("capability {id} is a read, so idempotent does not apply")));
        }
        let key = capability.id.clone();
        self.capabilities.insert(&key, capability);
        Ok(())
    }

    pub fn add_type(&mut self, name: &str, t: Type) -> Result<(), RegistryError> {
        self.add_owned_type(name, t, None)
    }

    fn add_owned_type(&mut self, name: &str, t: Type, owner: Option<&str>) -> Result<(), RegistryError> {
        if let Some(owner) = owner
            && !name.starts_with(&format!("{owner}."))
        {
            return Err(RegistryError(format!(
                "type {} of core {} must be named {owner}.NAME",
                repr_str(name),
                repr_str(owner)
            )));
        }
        if type_depth(&t) > TYPE_NESTING_LIMIT {
            return Err(RegistryError(format!(
                "type {} nests more than {TYPE_NESTING_LIMIT} levels deep",
                repr_str(name)
            )));
        }
        if self.types.get(name).is_some_and(|existing| *existing != t) {
            return Err(RegistryError(format!(
                "type {} is already registered with a different definition",
                repr_str(name)
            )));
        }
        self.types.insert(name, t);
        Ok(())
    }

    pub fn function(&self, name: &str) -> Option<&Function> {
        self.functions.get(name)
    }

    pub fn functions(&self) -> impl Iterator<Item = &Function> {
        self.functions.iter().map(|(_, f)| f)
    }

    pub fn core(&self, id: &str) -> Option<&Core> {
        self.cores.get(id)
    }

    pub fn cores(&self) -> impl Iterator<Item = &Core> {
        self.cores.iter().map(|(_, c)| c)
    }

    pub fn capability(&self, id: &str) -> Option<&Capability> {
        self.capabilities.get(id)
    }

    pub fn capabilities(&self) -> impl Iterator<Item = &Capability> {
        self.capabilities.iter().map(|(_, c)| c)
    }

    pub fn types(&self) -> impl Iterator<Item = (&str, &Type)> {
        self.types.iter()
    }

    pub fn named_types(&self) -> &BTreeMap<String, Type> {
        &self.types.entries
    }

    pub fn parse(&self, text: &str) -> Result<Type, RegistryError> {
        Ok(parse_type(text, &self.types.entries)?)
    }
}

fn check_evidence(core: &Core) -> Result<(), RegistryError> {
    let id = repr_str(&core.id);
    let levels: Vec<Assurance> = core.evidence.iter().map(|e| e.level).collect();
    if levels.contains(&Assurance::Unverified) {
        return Err(RegistryError(format!(
            "core {id} declares unverified evidence; registering the interface is that"
        )));
    }
    let mut distinct = levels.clone();
    distinct.sort();
    distinct.dedup();
    if distinct.len() != levels.len() {
        return Err(RegistryError(format!("core {id} declares one level of evidence twice")));
    }
    for evidence in &core.evidence {
        if evidence.references.is_empty() {
            return Err(RegistryError(format!(
                "core {id} claims {} without naming its evidence",
                evidence.level.name()
            )));
        }
    }
    if levels.contains(&Assurance::ProvedProperties) && !levels.contains(&Assurance::Specified) {
        return Err(RegistryError(format!("core {id} claims proved properties of a model it does not specify")));
    }
    Ok(())
}

fn python_strip(text: &str) -> &str {
    text.trim_matches(crate::unicode::is_space)
}

pub fn with_name(name: &str, t: Type) -> Type {
    match t {
        Type::Record(mut record) => {
            record.name = Some(name.to_string());
            Type::Record(record)
        }
        other => other,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    Registry(RegistryError),
    MissingKey(String),
    Value(String),
    Shape(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Registry(error) => f.write_str(&error.0),
            LoadError::MissingKey(key) => f.write_str(&repr_str(key)),
            LoadError::Value(message) | LoadError::Shape(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<RegistryError> for LoadError {
    fn from(error: RegistryError) -> LoadError {
        LoadError::Registry(error)
    }
}

fn entries<'a>(declarations: &'a Json, key: &str) -> Result<&'a [(String, Json)], LoadError> {
    match declarations.get(key) {
        None => Ok(&[]),
        Some(Json::Object(entries)) => Ok(entries),
        Some(other) => Err(LoadError::Shape(format!("declarations: {key} must be an object, not {}", python_repr(other)))),
    }
}

fn text<'a>(json: &'a Json, what: &str) -> Result<&'a str, LoadError> {
    json.as_str().ok_or_else(|| LoadError::Shape(format!("declarations: {what} must be a string, not {}", python_repr(json))))
}

fn required<'a>(entry: &'a Json, key: &str) -> Result<&'a Json, LoadError> {
    match entry {
        Json::Object(_) => entry.get(key).ok_or_else(|| LoadError::MissingKey(key.to_string())),
        other => Err(LoadError::Shape(format!("declarations: a capability must be an object, not {}", python_repr(other)))),
    }
}

fn truthy(json: &Json) -> bool {
    match json {
        Json::Null => false,
        Json::Bool(b) => *b,
        Json::Int(n) => *n != 0,
        Json::BigInt(_) => true,
        Json::Float(f) => *f != 0.0,
        Json::String(s) => !s.is_empty(),
        Json::Array(items) => !items.is_empty(),
        Json::Object(fields) => !fields.is_empty(),
    }
}

fn unique(pairs: &[(String, Json)]) -> Vec<(&str, &Json)> {
    let mut found: Vec<(&str, &Json)> = Vec::new();
    for (key, value) in pairs {
        match found.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = value,
            None => found.push((key, value)),
        }
    }
    found
}

pub fn load_declarations(registry: &mut Registry, declarations: &Json) -> Result<(), LoadError> {
    if !matches!(declarations, Json::Object(_)) {
        return Err(LoadError::Shape(format!("declarations must be a JSON object, not {}", python_repr(declarations))));
    }
    for (name, variants) in unique(entries(declarations, "enums")?) {
        let Json::Object(variants) = variants else {
            return Err(LoadError::Shape(format!("declarations: enum {} must map variants to payloads", repr_str(name))));
        };
        let mut parsed = Vec::new();
        for (variant, payload) in unique(variants) {
            let payload = match payload {
                Json::Null => None,
                other => Some(registry.parse(text(other, "an enum payload")?)?),
            };
            parsed.push((variant.to_string(), payload));
        }
        registry.add_type(name, Type::Enum(Arc::new(EnumType { name: name.to_string(), variants: parsed })))?;
    }
    let mut pending: Vec<(&str, &Json)> = unique(entries(declarations, "types")?);
    while !pending.is_empty() {
        let mut progressed = false;
        let mut remaining = Vec::new();
        for (name, definition) in pending {
            let added = text(definition, "a type definition")
                .map_err(Some)
                .and_then(|t| registry.parse(t).map_err(|_| None))
                .and_then(|t| registry.add_type(name, with_name(name, t)).map_err(|_| None));
            match added {
                Ok(()) => progressed = true,
                Err(Some(shape)) => return Err(shape),
                Err(None) => remaining.push((name, definition)),
            }
        }
        pending = remaining;
        if !progressed && let Some((name, definition)) = pending.first() {
            let parsed = registry.parse(text(definition, "a type definition")?)?;
            registry.add_type(name, with_name(name, parsed))?;
            return Err(LoadError::Shape(format!("declarations: type {} cannot be resolved", repr_str(name))));
        }
    }
    let capabilities = match declarations.get("capabilities") {
        None => &[][..],
        Some(Json::Array(items)) => items.as_slice(),
        Some(other) => {
            return Err(LoadError::Shape(format!("declarations: capabilities must be a list, not {}", python_repr(other))));
        }
    };
    for entry in capabilities {
        let id = text(required(entry, "id")?, "a capability id")?.to_string();
        let effect_json = required(entry, "effect")?;
        let effect = effect_json
            .as_str()
            .and_then(Effect::from_name)
            .ok_or_else(|| LoadError::Value(format!("{} is not a valid Effect", python_repr(effect_json))))?;
        let input = registry.parse(text(required(entry, "input")?, "a capability input")?)?;
        let output = registry.parse(text(required(entry, "output")?, "a capability output")?)?;
        let description = text(required(entry, "description")?, "a capability description")?.to_string();
        let idempotent = entry.get("idempotent").is_some_and(truthy);
        registry.add_capability(Capability { id, effect, input, output, description, idempotent })?;
    }
    Ok(())
}
