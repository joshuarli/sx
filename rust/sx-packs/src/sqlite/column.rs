use rusqlite::types::{Value as SqlValue, ValueRef};
use sx_kernel::types::{BOOL, BYTES, FLOAT, INT, STRING, Type};
use sx_kernel::values::Value;

const EXACT_FLOAT_INTEGER: u64 = 1 << 53;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Scalar {
    String,
    Int,
    Float,
    Bool,
    Bytes,
}

impl Scalar {
    fn of(t: &Type) -> Option<Scalar> {
        [
            (STRING, Scalar::String),
            (INT, Scalar::Int),
            (FLOAT, Scalar::Float),
            (BOOL, Scalar::Bool),
            (BYTES, Scalar::Bytes),
        ]
        .into_iter()
        .find(|(known, _)| known == t)
        .map(|(_, scalar)| scalar)
    }

    fn name(self) -> &'static str {
        match self {
            Scalar::String => "String",
            Scalar::Int => "Int",
            Scalar::Float => "Float",
            Scalar::Bool => "Bool",
            Scalar::Bytes => "Bytes",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Column {
    scalar: Scalar,
    nullable: bool,
}

fn storage_class(value: ValueRef<'_>) -> &'static str {
    match value {
        ValueRef::Null => "NULL",
        ValueRef::Integer(_) => "INTEGER",
        ValueRef::Real(_) => "REAL",
        ValueRef::Text(_) => "TEXT",
        ValueRef::Blob(_) => "BLOB",
    }
}

impl Column {
    pub(super) fn of(t: &Type) -> Option<Column> {
        match t {
            Type::Option(inner) => Scalar::of(inner).map(|scalar| Column { scalar, nullable: true }),
            other => Scalar::of(other).map(|scalar| Column { scalar, nullable: false }),
        }
    }

    pub(super) fn show(self) -> String {
        if self.nullable { format!("Option<{}>", self.scalar.name()) } else { self.scalar.name().to_string() }
    }

    pub(super) fn bind(self, value: &Value) -> Option<SqlValue> {
        let present = match (self.nullable, value) {
            (true, Value::None) => return Some(SqlValue::Null),
            (true, Value::Some(inner)) => &**inner,
            (false, other) => other,
            (true, _) => return None,
        };
        match (self.scalar, present) {
            (Scalar::String, Value::String(s)) => Some(SqlValue::Text(s.to_string())),
            (Scalar::Int, Value::Int(n)) => Some(SqlValue::Integer(*n)),
            (Scalar::Float, Value::Float(f)) => Some(SqlValue::Real(f.0)),
            (Scalar::Bool, Value::Bool(b)) => Some(SqlValue::Integer(i64::from(*b))),
            (Scalar::Bytes, Value::Bytes(b)) => Some(SqlValue::Blob(b.to_vec())),
            _ => None,
        }
    }

    pub(super) fn read(self, stored: ValueRef<'_>) -> Result<Value, String> {
        let mismatch = |detail: &str| format!("expected {}, found {detail}", self.show());
        let present = match (self.scalar, stored) {
            (_, ValueRef::Null) if self.nullable => return Ok(Value::None),
            (_, ValueRef::Null) => return Err(mismatch("NULL")),
            (Scalar::String, ValueRef::Text(bytes)) => match std::str::from_utf8(bytes) {
                Ok(text) => Value::string(text),
                Err(_) => return Err(mismatch("TEXT that is not valid UTF-8")),
            },
            (Scalar::Int, ValueRef::Integer(n)) => Value::Int(n),
            (Scalar::Float, ValueRef::Real(f)) if f.is_finite() => Value::float(f),
            (Scalar::Float, ValueRef::Real(_)) => return Err(mismatch("a REAL outside the finite Float range")),
            (Scalar::Float, ValueRef::Integer(n)) if n.unsigned_abs() <= EXACT_FLOAT_INTEGER => Value::float(n as f64),
            (Scalar::Float, ValueRef::Integer(_)) => return Err(mismatch("an INTEGER too large for an exact Float")),
            (Scalar::Bool, ValueRef::Integer(0)) => Value::Bool(false),
            (Scalar::Bool, ValueRef::Integer(1)) => Value::Bool(true),
            (Scalar::Bool, ValueRef::Integer(n)) => return Err(mismatch(&format!("the INTEGER {n}, which is neither 0 nor 1"))),
            (Scalar::Bytes, ValueRef::Blob(bytes)) => Value::Bytes(bytes.into()),
            (_, other) => return Err(mismatch(storage_class(other))),
        };
        Ok(if self.nullable { Value::some(present) } else { present })
    }
}
