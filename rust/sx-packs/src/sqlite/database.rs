use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::time::Duration;

use rusqlite::config::DbConfig;
use rusqlite::limits::Limit;
use rusqlite::{Connection, ErrorCode, OpenFlags, Transaction, TransactionBehavior};
use sx_kernel::registry::Effect;
use sx_kernel::types::CallErrorKind;

use crate::Failure;
use crate::fs::shown;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub busy_timeout: Duration,
    pub max_rows: usize,
}

impl Limits {
    pub const DEFAULT: Limits = Limits { busy_timeout: Duration::from_secs(5), max_rows: 100_000 };
}

pub(super) fn rejected(effect: Effect, message: String) -> Failure {
    let kind = if effect == Effect::Read { CallErrorKind::Invalid } else { CallErrorKind::Failed };
    Failure::new(kind, message)
}

pub(super) fn failure(effect: Effect, database: &str, error: &rusqlite::Error) -> Failure {
    let engine = match error {
        rusqlite::Error::SqliteFailure(code, message) => {
            Some((code.code, message.clone().unwrap_or_else(|| error.to_string())))
        }
        rusqlite::Error::SqlInputError { error: code, msg, .. } => Some((code.code, msg.clone())),
        _ => None,
    };
    let message = |detail: &str| format!("{}: {detail}", shown(database));
    let Some((code, detail)) = engine else {
        return match error {
            rusqlite::Error::InvalidParameterName(_)
            | rusqlite::Error::ExecuteReturnedResults
            | rusqlite::Error::MultipleStatement
            | rusqlite::Error::NulError(_) => rejected(effect, message(&error.to_string())),
            other => Failure::new(CallErrorKind::Failed, message(&other.to_string())),
        };
    };
    let kind = match code {
        ErrorCode::ConstraintViolation => CallErrorKind::Conflict,
        ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => CallErrorKind::Unavailable,
        ErrorCode::ReadOnly | ErrorCode::PermissionDenied | ErrorCode::AuthorizationForStatementDenied => {
            CallErrorKind::PermissionDenied
        }
        ErrorCode::Unknown | ErrorCode::TypeMismatch | ErrorCode::TooBig | ErrorCode::ParameterOutOfRange => {
            return rejected(effect, message(&detail));
        }
        _ => CallErrorKind::Failed,
    };
    Failure::new(kind, message(&detail))
}

pub(super) fn connect(path: &Path, effect: Effect, database: &str, limits: &Limits) -> Result<Connection, Failure> {
    let metadata = fs::symlink_metadata(path);
    match (&metadata, effect) {
        (Err(error), Effect::Read) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            return Err(Failure::new(CallErrorKind::NotFound, format!("{} does not exist", shown(database))));
        }
        (Ok(found), _) if found.is_dir() => {
            return Err(Failure::new(CallErrorKind::Invalid, format!("{} is a directory", shown(database))));
        }
        _ => {}
    }
    let access = match effect {
        Effect::Read => OpenFlags::SQLITE_OPEN_READ_ONLY,
        Effect::Write => OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
    };
    let flags = access | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let connection = Connection::open_with_flags(path, flags).map_err(|error| {
        let missing_parent = path.parent().is_some_and(|parent| !parent.is_dir());
        if matches!(error, rusqlite::Error::SqliteFailure(ref code, _) if code.code == ErrorCode::CannotOpen) && missing_parent {
            Failure::new(CallErrorKind::NotFound, format!("{}: the directory does not exist", shown(database)))
        } else {
            failure(effect, database, &error)
        }
    })?;
    let setup = |connection: &Connection| -> rusqlite::Result<()> {
        connection.busy_timeout(limits.busy_timeout)?;
        connection.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0)?;
        connection.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
        connection.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false)?;
        connection.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY, true)?;
        Ok(())
    };
    setup(&connection).map_err(|error| failure(effect, database, &error))?;
    Ok(connection)
}

pub(super) fn transact<T>(
    connection: &mut Connection,
    effect: Effect,
    database: &str,
    body: impl FnOnce(&Transaction<'_>) -> Result<T, Failure>,
) -> Result<T, Failure> {
    let behavior = match effect {
        Effect::Read => TransactionBehavior::Deferred,
        Effect::Write => TransactionBehavior::Immediate,
    };
    let transaction =
        connection.transaction_with_behavior(behavior).map_err(|error| failure(effect, database, &error))?;
    let produced = body(&transaction)?;
    transaction.commit().map_err(|error| failure(effect, database, &error))?;
    Ok(produced)
}
