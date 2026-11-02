use std::collections::BTreeMap;
use std::sync::Arc;

use rusqlite::ToSql;
use rusqlite::types::Value as SqlValue;
use sx_kernel::authority::ScopeCheck;
use sx_kernel::json::{Json, python_repr, repr_str};
use sx_kernel::registry::{Capability, Effect, EndpointBinder, Registry};
use sx_kernel::types::{CallErrorKind, INT, Type, show_type};
use sx_kernel::values::{RecordValue, Value};

use super::column::Column;
use super::database::{Limits, connect, failure, rejected, transact};
use super::{check_database, resolve, scan, scope_type};
use crate::Failure;
use crate::declaration::{required, string_of, unknown_keys};
use crate::fs::{Roots, prepare, shown};

#[derive(Clone, Debug)]
struct Parameter {
    field: String,
    token: String,
    column: Column,
}

#[derive(Clone, Debug)]
struct Rows {
    fields: Vec<(String, Column)>,
    open: bool,
}

#[derive(Clone, Debug)]
enum Output {
    Changes,
    Rows(Rows),
}

#[derive(Clone, Debug)]
pub struct Statement {
    id: String,
    database: String,
    sql: String,
    parameters: Vec<Parameter>,
    output: Output,
    effect: Effect,
}

#[derive(Clone, Debug, Default)]
pub struct Statements {
    by_id: BTreeMap<String, Arc<Statement>>,
}

impl Statements {
    pub fn get(&self, id: &str) -> Option<&Arc<Statement>> {
        self.by_id.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.by_id.keys().map(String::as_str)
    }
}

#[derive(Debug, Default)]
pub struct StatementLoader {
    statements: Statements,
}

impl StatementLoader {
    pub fn new() -> StatementLoader {
        StatementLoader::default()
    }

    pub fn finish(self) -> Statements {
        self.statements
    }
}

impl EndpointBinder for StatementLoader {
    fn bind(&mut self, registry: &mut Registry, entry: &Json) -> Result<(), String> {
        let id = string_of(required(entry, "id", "a statement")?, "a statement id")?.to_string();
        let named = |message: String| format!("statement {}: {message}", repr_str(&id));
        let (statement, capability) = build(registry, entry, &id).map_err(named)?;
        let database = statement.database.clone();
        let effect = statement.effect;
        let check = ScopeCheck::new(prepare, move |scope, _input| check_database(scope, &database, effect));
        registry
            .add_checked_capability(capability, check)
            .map_err(|e| format!("statement {}: {}", repr_str(&id), e.0))?;
        self.statements.by_id.insert(id, Arc::new(statement));
        Ok(())
    }
}

fn describe(sql: &str) -> String {
    let collapsed = sql.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut shortened: String = collapsed.chars().take(80).collect();
    if collapsed.chars().count() > 80 {
        shortened.push_str("...");
    }
    shortened
}

fn build(registry: &Registry, entry: &Json, id: &str) -> Result<(Statement, Capability), String> {
    if entry.get("scope").is_some() {
        return Err("a statement's scope is implied, so it declares none".to_string());
    }
    let sqlite = required(entry, "sqlite", "a statement")?;
    unknown_keys(sqlite, &["database", "statement"], "sqlite")?;
    let effect_json = required(entry, "effect", "a statement")?;
    let effect = effect_json
        .as_str()
        .and_then(Effect::from_name)
        .ok_or_else(|| format!("{} is not a valid Effect", python_repr(effect_json)))?;
    let input = registry.parse(string_of(required(entry, "input", "a statement")?, "input")?).map_err(|e| e.0)?;
    let output = registry.parse(string_of(required(entry, "output", "a statement")?, "output")?).map_err(|e| e.0)?;
    let database = string_of(required(sqlite, "database", "sqlite")?, "the database")?.to_string();
    if !database.starts_with('/') || database.contains('\0') {
        return Err(format!("database {} is not an absolute path", repr_str(&database)));
    }
    let sql = string_of(required(sqlite, "statement", "sqlite")?, "the statement")?.to_string();
    if sql.trim().is_empty() {
        return Err("the statement is empty".to_string());
    }
    let Type::Record(input_record) = &input else {
        return Err(format!("the input must be a record, not {}", show_type(&input)));
    };
    let tokens = scan::parameters(&sql)?;
    let mut parameters = Vec::new();
    for (field, t) in &input_record.fields {
        let column = Column::of(t).ok_or_else(|| {
            format!("input field {} has type {}, which is not a String, Int, Float, Bool, Bytes, or an Option of one", repr_str(field), show_type(t))
        })?;
        if !tokens.contains(field) {
            return Err(format!("input field {} is not used by a :{field} parameter of the statement", repr_str(field)));
        }
        parameters.push(Parameter { field: field.clone(), token: format!(":{field}"), column });
    }
    if let Some(missing) = tokens.iter().find(|token| input_record.field(token).is_none()) {
        return Err(format!("parameter :{missing} is not a field of the input"));
    }
    let output_shape = output_of(&output)?;
    if matches!(output_shape, Output::Changes) && effect == Effect::Read {
        return Err("an output of Int counts the rows a statement changed, so the effect must be write".to_string());
    }
    let idempotent = entry.get("idempotent").and_then(Json::as_bool).unwrap_or(false);
    let description = match entry.get("description") {
        Some(text) => string_of(text, "the description")?.to_string(),
        None => describe(&sql),
    };
    let capability = Capability {
        id: id.to_string(),
        effect,
        input,
        output,
        description,
        idempotent,
        scope: Some(scope_type()),
    };
    let statement = Statement { id: id.to_string(), database, sql, parameters, output: output_shape, effect };
    Ok((statement, capability))
}

fn output_of(output: &Type) -> Result<Output, String> {
    if *output == INT {
        return Ok(Output::Changes);
    }
    let Type::List(element) = output else {
        return Err(format!("the output must be Int or List<Record {{...}}>, not {}", show_type(output)));
    };
    let Type::Record(record) = &**element else {
        return Err(format!("the output must be Int or List<Record {{...}}>, not {}", show_type(output)));
    };
    let mut fields = Vec::new();
    for (name, t) in &record.fields {
        let column = Column::of(t).ok_or_else(|| {
            format!("output field {} has type {}, which is not a String, Int, Float, Bool, Bytes, or an Option of one", repr_str(name), show_type(t))
        })?;
        fields.push((name.clone(), column));
    }
    Ok(Output::Rows(Rows { fields, open: record.open }))
}

fn field<'v>(input: &'v Value, name: &str) -> Option<&'v Value> {
    match input {
        Value::Record(record) => record.get(name),
        _ => None,
    }
}

impl Statement {
    pub fn id(&self) -> &str {
        &self.id
    }

    fn mismatch(&self) -> Failure {
        Failure::new(CallErrorKind::Failed, format!("{} received an input that does not match its type", self.id))
    }

    fn bound(&self, input: &Value) -> Result<Vec<(String, SqlValue)>, Failure> {
        self.parameters
            .iter()
            .map(|parameter| {
                field(input, &parameter.field)
                    .and_then(|value| parameter.column.bind(value))
                    .map(|bound| (parameter.token.clone(), bound))
                    .ok_or_else(|| self.mismatch())
            })
            .collect()
    }

    pub fn perform_with(&self, limits: &Limits, scope_value: &Value, input: &Value) -> Result<Value, Failure> {
        let denied = |reason: String| Failure::new(CallErrorKind::PermissionDenied, reason);
        let roots = Roots::of(scope_value).map_err(denied)?;
        let path = resolve(&roots, &self.database, self.effect).map_err(denied)?;
        let bound = self.bound(input)?;
        let mut connection = connect(&path, self.effect, &self.database, limits)?;
        transact(&mut connection, self.effect, &self.database, |transaction| self.run(transaction, limits, &bound))
    }

    pub fn perform(&self, scope: &Value, input: &Value) -> Result<Value, Failure> {
        self.perform_with(&Limits::DEFAULT, scope, input)
    }

    fn run(&self, transaction: &rusqlite::Transaction<'_>, limits: &Limits, bound: &[(String, SqlValue)]) -> Result<Value, Failure> {
        let fail = |error: rusqlite::Error| failure(self.effect, &self.database, &error);
        let reject = |message: String| rejected(self.effect, format!("{}: {message}", shown(&self.database)));
        let mut prepared = transaction.prepare(&self.sql).map_err(fail)?;
        if self.effect == Effect::Read && !prepared.readonly() {
            return Err(reject("the statement changes the database, so it cannot be a read".to_string()));
        }
        if prepared.parameter_count() != self.parameters.len() {
            return Err(Failure::new(
                CallErrorKind::Failed,
                format!("{}: SQLite found {} parameters where the declaration has {}", self.id, prepared.parameter_count(), self.parameters.len()),
            ));
        }
        let named: Vec<(&str, &dyn ToSql)> = bound.iter().map(|(token, value)| (token.as_str(), value as &dyn ToSql)).collect();
        match &self.output {
            Output::Changes => {
                if prepared.column_count() > 0 {
                    return Err(reject("the statement returns rows, so its output must be a List<Record {...}>, not Int".to_string()));
                }
                match prepared.execute(named.as_slice()) {
                Ok(changed) => i64::try_from(changed)
                    .map(Value::Int)
                    .map_err(|_| Failure::new(CallErrorKind::Failed, format!("{}: the changed row count is too large", self.id))),
                Err(error) => Err(fail(error)),
                }
            }
            Output::Rows(shape) => {
                if prepared.column_count() == 0 {
                    return Err(reject("the statement returns no rows, so its output must be Int, not a list".to_string()));
                }
                let names: Vec<String> = prepared.column_names().map(str::to_string).collect();
                let plan = shape.plan(&names).map_err(reject)?;
                let mut rows = prepared.query(named.as_slice()).map_err(fail)?;
                let mut collected = Vec::new();
                while let Some(row) = rows.next().map_err(fail)? {
                    if collected.len() >= limits.max_rows {
                        return Err(Failure::new(
                            CallErrorKind::Failed,
                            format!("{}: the statement returned more than {} rows; add a LIMIT", shown(&self.database), limits.max_rows),
                        ));
                    }
                    let mut fields = Vec::with_capacity(plan.len());
                    for (index, name, column) in &plan {
                        let stored = row.get_ref(*index).map_err(fail)?;
                        let value = column.read(stored).map_err(|problem| {
                            reject(format!("row {}, column {}: {problem}", collected.len() + 1, repr_str(name)))
                        })?;
                        fields.push((Arc::from(*name), value));
                    }
                    collected.push(Value::Record(RecordValue::new(fields)));
                }
                Ok(Value::list(collected))
            }
        }
    }
}

impl Rows {
    fn plan<'s>(&'s self, names: &[String]) -> Result<Vec<(usize, &'s str, Column)>, String> {
        for (at, name) in names.iter().enumerate() {
            if names.iter().take(at).any(|earlier| earlier == name) {
                return Err(format!("the statement returns a column named {} twice; alias one with AS", repr_str(name)));
            }
            if !self.open && !self.fields.iter().any(|(declared, _)| declared == name) {
                return Err(format!(
                    "the statement returns a column {} that the output record does not declare; declare it or make the record open",
                    repr_str(name)
                ));
            }
        }
        self.fields
            .iter()
            .map(|(declared, column)| {
                names
                    .iter()
                    .position(|name| name == declared)
                    .map(|index| (index, declared.as_str(), *column))
                    .ok_or_else(|| {
                        format!("the output record declares {} ({}), but the statement returns no such column", repr_str(declared), column.show())
                    })
            })
            .collect()
    }
}
