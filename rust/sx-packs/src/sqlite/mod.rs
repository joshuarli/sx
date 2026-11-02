mod column;
mod database;
mod scan;
mod statement;

use std::path::PathBuf;

use rusqlite::fallible_iterator::FallibleIterator;
use rusqlite::{Batch, Error};
use sx_kernel::authority::{ScopeCheck, Verdict};
use sx_kernel::registry::{Capability, Effect, Pack, PackCapability};
use sx_kernel::types::{CallErrorKind, RecordType, STRING, Type, UNIT};
use sx_kernel::values::Value;

use crate::fs::{self, Roots, prepare, resolve_read, resolve_write, shown, text_field};
use crate::{Failure, Performer};

pub use database::Limits;
pub use statement::{Statement, StatementLoader, Statements};

pub const EXECUTE: &str = "sqlite.execute";

pub fn scope_type() -> Type {
    fs::scope_type()
}

fn execute_input() -> Type {
    Type::Record(RecordType::plain(vec![("database".to_string(), STRING), ("sql".to_string(), STRING)]))
}

pub fn pack() -> Pack {
    Pack {
        id: "sqlite".to_string(),
        types: Vec::new(),
        capabilities: vec![PackCapability {
            capability: Capability {
                id: EXECUTE.to_string(),
                effect: Effect::Write,
                input: execute_input(),
                output: UNIT,
                description: "Runs SQL statements with no parameters, such as CREATE TABLE, in one transaction.".to_string(),
                idempotent: false,
                scope: Some(scope_type()),
            },
            check: Some(ScopeCheck::new(prepare, check_execute)),
        }],
    }
}

pub fn performer(capability: &str) -> Option<Performer> {
    match capability {
        EXECUTE => Some(execute),
        _ => None,
    }
}

pub(crate) fn resolve(roots: &Roots, database: &str, effect: Effect) -> Result<PathBuf, String> {
    match effect {
        Effect::Read => resolve_read(roots, database),
        Effect::Write => resolve_write(roots, database),
    }
}

pub(crate) fn check_database(scope: &Value, database: &str, effect: Effect) -> Verdict {
    match Roots::of(scope).and_then(|roots| resolve(&roots, database, effect)) {
        Ok(_) => Verdict::Allowed,
        Err(reason) => Verdict::Denied(reason),
    }
}

fn check_execute(scope: &Value, input: &Value) -> Verdict {
    match text_field(input, "database") {
        Some(database) => check_database(scope, database, Effect::Write),
        None => Verdict::Denied("the input has no database".to_string()),
    }
}

pub fn execute_with(limits: &Limits, scope: &Value, input: &Value) -> Result<Value, Failure> {
    let (Some(database), Some(sql)) = (text_field(input, "database"), text_field(input, "sql")) else {
        return Err(Failure::new(
            CallErrorKind::Failed,
            format!("{EXECUTE} received an input that does not match its type"),
        ));
    };
    let denied = |reason: String| Failure::new(CallErrorKind::PermissionDenied, reason);
    let roots = Roots::of(scope).map_err(denied)?;
    let path = resolve(&roots, database, Effect::Write).map_err(denied)?;
    let mut connection = database::connect(&path, Effect::Write, database, limits)?;
    database::transact(&mut connection, Effect::Write, database, |transaction| {
        let fail = |error: Error| database::failure(Effect::Write, database, &error);
        let mut statements = Batch::new(transaction, sql);
        while let Some(mut statement) = statements.next().map_err(fail)? {
            if statement.parameter_count() > 0 {
                return Err(Failure::new(
                    CallErrorKind::Failed,
                    format!("{}: {EXECUTE} binds no parameters, and a statement has some", shown(database)),
                ));
            }
            match statement.execute([]) {
                Ok(_) => {}
                Err(Error::ExecuteReturnedResults) => {
                    return Err(Failure::new(
                        CallErrorKind::Failed,
                        format!("{}: {EXECUTE} runs statements that return no rows", shown(database)),
                    ));
                }
                Err(error) => return Err(fail(error)),
            }
        }
        Ok(Value::Unit)
    })
}

fn execute(scope: &Value, input: &Value) -> Result<Value, Failure> {
    execute_with(&Limits::DEFAULT, scope, input)
}
