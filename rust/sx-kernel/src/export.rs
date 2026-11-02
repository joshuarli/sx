use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use crate::ir::{
    Binder, CapabilitySignature, Conversion, FunctionSignature, If, Lambda, Let, Loop, Match, MatchPattern, Node,
    NodeKind, PreludeOp, Program, Retry, Signatures, WaitUntil, WidenedField, WrapKind,
};
use crate::json::{Json, parse};
use crate::registry::Effect;
use crate::syntax::{Position, Span};
use crate::types::{CallErrorKind, EnumType, Primitive, RecordType, Type};
use crate::values::{MapValue, RecordValue, Value, base64_decode, base64_encode};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IrError(pub String);

impl fmt::Display for IrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for IrError {}

fn bad(what: &str, json: &Json) -> IrError {
    let shown = crate::json::dumps(json, crate::json::Layout::Spaced).unwrap_or_else(|e| e.0);
    let shown: String = shown.chars().take(120).collect();
    IrError(format!("invalid {what} in exported IR: {shown}"))
}

fn field<'a>(json: &'a Json, key: &str, what: &str) -> Result<&'a Json, IrError> {
    json.get(key).ok_or_else(|| IrError(format!("{what} in exported IR has no {key:?}")))
}

fn string(json: &Json, what: &str) -> Result<String, IrError> {
    json.as_str().map(str::to_string).ok_or_else(|| bad(what, json))
}

fn integer(json: &Json, what: &str) -> Result<i64, IrError> {
    json.as_i64().ok_or_else(|| bad(what, json))
}

fn array<'a>(json: &'a Json, what: &str) -> Result<&'a [Json], IrError> {
    json.as_array().ok_or_else(|| bad(what, json))
}

fn pair<'a>(json: &'a Json, what: &str) -> Result<(&'a Json, &'a Json), IrError> {
    match array(json, what)? {
        [a, b] => Ok((a, b)),
        _ => Err(bad(what, json)),
    }
}

fn only_entry(json: &Json) -> Option<(&str, &Json)> {
    match json.as_object()? {
        [(key, value)] => Some((key.as_str(), value)),
        _ => None,
    }
}

struct Reader<'a> {
    raw_enums: BTreeMap<String, &'a Json>,
    enums: BTreeMap<String, Arc<EnumType>>,
    resolving: Vec<String>,
}

impl<'a> Reader<'a> {
    fn enum_type(&mut self, name: &str) -> Result<Arc<EnumType>, IrError> {
        if let Some(found) = self.enums.get(name) {
            return Ok(found.clone());
        }
        if self.resolving.iter().any(|n| n == name) {
            return Err(IrError(format!("enum {name} refers to itself in exported IR")));
        }
        let Some(raw) = self.raw_enums.get(name).copied() else {
            return Err(IrError(format!("enum {name} is not defined in exported IR")));
        };
        self.resolving.push(name.to_string());
        let mut variants = Vec::new();
        for entry in array(raw, "enum variants")? {
            let (variant, payload) = pair(entry, "enum variant")?;
            let payload = if payload.is_null() { None } else { Some(self.type_of(payload)?) };
            variants.push((string(variant, "variant name")?, payload));
        }
        self.resolving.pop();
        let resolved = Arc::new(EnumType { name: name.to_string(), variants });
        self.enums.insert(name.to_string(), resolved.clone());
        Ok(resolved)
    }

    fn type_of(&mut self, json: &Json) -> Result<Type, IrError> {
        if let Some(name) = json.as_str() {
            if name == "Never" {
                return Ok(Type::Never);
            }
            return Primitive::from_name(name).map(Type::Primitive).ok_or_else(|| bad("type", json));
        }
        if let Some(entries) = json.as_object()
            && entries.iter().any(|(k, _)| k == "record")
        {
            let mut fields = Vec::new();
            for entry in array(field(json, "record", "record type")?, "record fields")? {
                let (name, field_type) = pair(entry, "record field")?;
                fields.push((string(name, "field name")?, self.type_of(field_type)?));
            }
            let name = match field(json, "name", "record type")? {
                Json::Null => None,
                other => Some(string(other, "record name")?),
            };
            let open = field(json, "open", "record type")?.as_bool().ok_or_else(|| bad("record type", json))?;
            return Ok(Type::Record(RecordType { fields, name, open }));
        }
        if let Some(entries) = json.as_object()
            && entries.iter().any(|(k, _)| k == "nominal")
        {
            let name = string(field(json, "nominal", "nominal type")?, "nominal name")?;
            let base = self.type_of(field(json, "base", "nominal type")?)?;
            return Ok(Type::Nominal { name, base: Box::new(base) });
        }
        let Some((key, inner)) = only_entry(json) else {
            return Err(bad("type", json));
        };
        Ok(match key {
            "list" => Type::List(Box::new(self.type_of(inner)?)),
            "map" => Type::Map(Box::new(self.type_of(inner)?)),
            "option" => Type::Option(Box::new(self.type_of(inner)?)),
            "tuple" => Type::Tuple(array(inner, "tuple type")?.iter().map(|t| self.type_of(t)).collect::<Result<_, _>>()?),
            "result" => {
                let (ok, err) = pair(inner, "result type")?;
                Type::Result(Box::new(self.type_of(ok)?), Box::new(self.type_of(err)?))
            }
            "enum" => Type::Enum(self.enum_type(&string(inner, "enum name")?)?),
            "var" => Type::Variable(u64::try_from(integer(inner, "type variable")?).map_err(|_| bad("type variable", inner))?),
            _ => return Err(bad("type", json)),
        })
    }

    fn value(&mut self, json: &Json) -> Result<Value, IrError> {
        match json.as_str() {
            Some("unit") => return Ok(Value::Unit),
            Some("none") => return Ok(Value::None),
            Some(_) => return Err(bad("value", json)),
            None => {}
        }
        let Some((key, inner)) = only_entry(json) else {
            return Err(bad("value", json));
        };
        Ok(match key {
            "bool" => Value::Bool(inner.as_bool().ok_or_else(|| bad("bool value", inner))?),
            "int" => Value::Int(integer(inner, "int value")?),
            "float" => match inner {
                Json::Float(f) => Value::float(*f),
                _ => return Err(bad("float value", inner)),
            },
            "bytes" => {
                let text = string(inner, "bytes value")?;
                match base64_decode(&text) {
                    Some(raw) if base64_encode(&raw) == text => Value::Bytes(raw.into()),
                    _ => return Err(bad("bytes value", inner)),
                }
            }
            "string" => Value::string(&string(inner, "string value")?),
            "some" => Value::some(self.value(inner)?),
            "ok" => Value::Ok(Arc::new(self.value(inner)?)),
            "err" => Value::Err(Arc::new(self.value(inner)?)),
            "list" => Value::list(array(inner, "list value")?.iter().map(|v| self.value(v)).collect::<Result<_, _>>()?),
            "tuple" => {
                Value::tuple(array(inner, "tuple value")?.iter().map(|v| self.value(v)).collect::<Result<_, _>>()?)
            }
            "record" => {
                let mut fields = Vec::new();
                for entry in array(inner, "record value")? {
                    let (name, item) = pair(entry, "record field value")?;
                    fields.push((Arc::from(string(name, "field name")?.as_str()), self.value(item)?));
                }
                Value::Record(RecordValue::new(fields))
            }
            "map" => {
                let mut entries = Vec::new();
                for entry in array(inner, "map value")? {
                    let (name, item) = pair(entry, "map entry")?;
                    entries.push((Arc::from(string(name, "map key")?.as_str()), self.value(item)?));
                }
                Value::Map(MapValue::from_pairs(entries))
            }
            "variant" => {
                let enum_name = string(field(inner, "enum", "variant value")?, "variant enum")?;
                let name = string(field(inner, "name", "variant value")?, "variant name")?;
                let payload = match field(inner, "payload", "variant value")? {
                    Json::Null => None,
                    other => Some(self.value(other)?),
                };
                Value::variant(&enum_name, &name, payload)
            }
            _ => return Err(bad("value", json)),
        })
    }

    fn binder(&mut self, json: &Json) -> Result<Binder, IrError> {
        if json.as_str() == Some("discard") {
            return Ok(Binder::Discard);
        }
        match only_entry(json) {
            Some(("bind", name)) => Ok(Binder::Name(string(name, "binder name")?)),
            Some(("tuple", items)) => {
                Ok(Binder::Tuple(array(items, "tuple binder")?.iter().map(|b| self.binder(b)).collect::<Result<_, _>>()?))
            }
            _ => Err(bad("binder", json)),
        }
    }

    fn pattern(&mut self, json: &Json) -> Result<MatchPattern, IrError> {
        if json.as_str() == Some("none") {
            return Ok(MatchPattern::None);
        }
        if let Some(entries) = json.as_object()
            && entries.len() == 2
            && entries.iter().any(|(k, _)| k == "variant")
        {
            let variant = string(field(json, "variant", "variant pattern")?, "variant name")?;
            let inner = match field(json, "inner", "variant pattern")? {
                Json::Null => None,
                other => Some(Box::new(self.pattern(other)?)),
            };
            return Ok(MatchPattern::Variant { variant, inner });
        }
        match only_entry(json) {
            Some(("some", inner)) => Ok(MatchPattern::Some(Box::new(self.pattern(inner)?))),
            Some(("ok", inner)) => Ok(MatchPattern::Ok(Box::new(self.pattern(inner)?))),
            Some(("err", inner)) => Ok(MatchPattern::Err(Box::new(self.pattern(inner)?))),
            Some(("literal", value)) => Ok(MatchPattern::Literal(self.value(value)?)),
            Some(("any", binder)) => Ok(MatchPattern::Any(self.binder(binder)?)),
            _ => Err(bad("pattern", json)),
        }
    }

    fn optional_conversion(&mut self, json: &Json) -> Result<Option<Conversion>, IrError> {
        if json.is_null() {
            return Ok(None);
        }
        self.conversion(json).map(Some)
    }

    fn conversion(&mut self, json: &Json) -> Result<Conversion, IrError> {
        if json.as_str() == Some("variant-name") {
            return Ok(Conversion::VariantName);
        }
        match only_entry(json) {
            Some(("to-some", inner)) => Ok(Conversion::ToSome(self.optional_conversion(inner)?.map(Box::new))),
            Some(("each-field", fields)) => {
                let mut converted = Vec::new();
                for entry in array(fields, "each-field conversion")? {
                    let (name, inner) = pair(entry, "field conversion")?;
                    converted.push((string(name, "field name")?, self.optional_conversion(inner)?));
                }
                Ok(Conversion::EachField(converted))
            }
            Some(("widen-record", fields)) => {
                let mut widened = Vec::new();
                for entry in array(fields, "widen-record conversion")? {
                    let [name, present, inner, field_type] = array(entry, "widened field")? else {
                        return Err(bad("widened field", entry));
                    };
                    widened.push(WidenedField {
                        name: string(name, "field name")?,
                        present: present.as_bool().ok_or_else(|| bad("widened field", entry))?,
                        conversion: self.optional_conversion(inner)?,
                        field_type: self.type_of(field_type)?,
                    });
                }
                Ok(Conversion::WidenRecord(widened))
            }
            Some(("each-item", items)) => Ok(Conversion::EachItem(
                array(items, "each-item conversion")?.iter().map(|c| self.optional_conversion(c)).collect::<Result<_, _>>()?,
            )),
            Some(("each-element", element)) => Ok(Conversion::EachElement(Box::new(self.conversion(element)?))),
            _ => Err(bad("conversion", json)),
        }
    }

    fn span(json: &Json) -> Result<Span, IrError> {
        let parts = array(json, "span")?;
        let [start_line, start_column, end_line, end_column] = parts else {
            return Err(bad("span", json));
        };
        let number = |part: &Json| {
            part.as_i64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| bad("span", json))
        };
        Ok(Span::new(
            Position { line: number(start_line)?, column: number(start_column)? },
            Position { line: number(end_line)?, column: number(end_column)? },
        ))
    }

    fn nodes(&mut self, json: &Json, what: &str) -> Result<Vec<Node>, IrError> {
        array(json, what)?.iter().map(|n| self.node(n)).collect()
    }

    fn boxed(&mut self, json: &Json, key: &str, what: &str) -> Result<Box<Node>, IrError> {
        Ok(Box::new(self.node(field(json, key, what)?)?))
    }

    fn lambda(&mut self, json: &Json) -> Result<Lambda, IrError> {
        match self.node(json)?.kind {
            NodeKind::Lambda(lambda) => Ok(lambda),
            _ => Err(bad("lambda", json)),
        }
    }

    fn durations(json: &Json, what: &str) -> Result<Vec<i64>, IrError> {
        array(json, what)?.iter().map(|d| integer(d, what)).collect()
    }

    fn node(&mut self, json: &Json) -> Result<Node, IrError> {
        let name = string(field(json, "node", "node")?, "node name")?;
        let span = Reader::span(field(json, "span", &name)?)?;
        let what = name.as_str();
        let get = |key: &str| field(json, key, what);
        let kind = match what {
            "Literal" => NodeKind::Literal { value: self.value(get("value")?)?, value_type: self.type_of(get("type")?)? },
            "Variable" => NodeKind::Variable { name: string(get("name")?, "variable name")? },
            "Let" => NodeKind::Let(Let {
                binder: self.binder(get("binder")?)?,
                value: self.boxed(json, "value", what)?,
                body: self.boxed(json, "body", what)?,
            }),
            "LocalFunction" => {
                NodeKind::LocalFunction { name: string(get("name")?, "function name")?, body: self.boxed(json, "body", what)? }
            }
            "If" => NodeKind::If(If {
                condition: self.boxed(json, "condition", what)?,
                then: self.boxed(json, "then", what)?,
                otherwise: self.boxed(json, "otherwise", what)?,
            }),
            "Match" => {
                let scrutinee = self.boxed(json, "scrutinee", what)?;
                let mut clauses = Vec::new();
                for clause in array(get("clauses")?, "match clauses")? {
                    let (pattern, body) = pair(clause, "match clause")?;
                    clauses.push((self.pattern(pattern)?, self.node(body)?));
                }
                NodeKind::Match(Match { scrutinee, clauses })
            }
            "Lambda" => {
                let params = array(get("params")?, "lambda params")?.iter().map(|b| self.binder(b)).collect::<Result<_, _>>()?;
                let param_types =
                    array(get("param_types")?, "lambda param types")?.iter().map(|t| self.type_of(t)).collect::<Result<_, _>>()?;
                let captured = match get("captured")? {
                    Json::Null => None,
                    other => Some(string(other, "captured name")?),
                };
                NodeKind::Lambda(Lambda {
                    params,
                    param_types,
                    result: self.type_of(get("result")?)?,
                    body: self.boxed(json, "body", what)?,
                    captured,
                    span,
                })
            }
            "ApplyLocal" => NodeKind::ApplyLocal {
                name: string(get("name")?, "function name")?,
                function: self.lambda(get("function")?)?,
                args: self.nodes(get("args")?, "arguments")?,
            },
            "Prelude" => {
                let op_name = string(get("op")?, "prelude op")?;
                let op = PreludeOp::from_name(&op_name)
                    .ok_or_else(|| IrError(format!("unknown prelude op {op_name:?} in exported IR")))?;
                NodeKind::Prelude { op, args: self.nodes(get("args")?, "arguments")? }
            }
            "CoreCall" => NodeKind::CoreCall {
                function: string(get("function")?, "core function")?,
                args: self.nodes(get("args")?, "arguments")?,
            },
            "Call" => NodeKind::Call {
                capability: string(get("capability")?, "capability")?,
                argument: self.boxed(json, "argument", what)?,
            },
            "Get" => NodeKind::Get { record: self.boxed(json, "record", what)?, field: string(get("field")?, "field name")? },
            "MakeRecord" => {
                let mut fields = Vec::new();
                for entry in array(get("fields")?, "record fields")? {
                    let (name, value) = pair(entry, "record field")?;
                    fields.push((string(name, "field name")?, self.node(value)?));
                }
                NodeKind::MakeRecord { fields }
            }
            "MakeList" => NodeKind::MakeList {
                items: self.nodes(get("items")?, "list items")?,
                element: self.type_of(get("element")?)?,
            },
            "MakeTuple" => NodeKind::MakeTuple { items: self.nodes(get("items")?, "tuple items")? },
            "MakeBracket" => NodeKind::MakeBracket {
                items: self.nodes(get("items")?, "bracket items")?,
                shape: self.type_of(get("shape")?)?,
            },
            "MakeEmptyBrace" => NodeKind::MakeEmptyBrace { shape: self.type_of(get("shape")?)? },
            "Wrap" => {
                let kind = match string(get("kind")?, "wrap kind")?.as_str() {
                    "some" => WrapKind::Some,
                    "ok" => WrapKind::Ok,
                    "err" => WrapKind::Err,
                    _ => return Err(bad("wrap kind", get("kind")?)),
                };
                NodeKind::Wrap { kind, value: self.boxed(json, "value", what)?, wrapped_type: self.type_of(get("type")?)? }
            }
            "MakeVariant" => NodeKind::MakeVariant {
                enum_name: string(get("enum")?, "variant enum")?,
                variant: string(get("variant")?, "variant name")?,
                payload: match get("payload")? {
                    Json::Null => None,
                    other => Some(Box::new(self.node(other)?)),
                },
            },
            "Coerce" => NodeKind::Coerce {
                value: self.boxed(json, "value", what)?,
                conversion: self.conversion(get("conversion")?)?,
            },
            "Attempt" => NodeKind::Attempt { body: self.boxed(json, "body", what)? },
            "Retry" => {
                let kinds = array(get("kinds")?, "retry kinds")?
                    .iter()
                    .map(|k| k.as_str().and_then(CallErrorKind::from_name).ok_or_else(|| bad("retry kind", k)))
                    .collect::<Result<_, _>>()?;
                NodeKind::Retry(Retry {
                    delays: Reader::durations(get("delays")?, "retry delays")?,
                    kinds,
                    body: self.boxed(json, "body", what)?,
                })
            }
            "WaitUntil" => NodeKind::WaitUntil(WaitUntil {
                intervals: Reader::durations(get("intervals")?, "wait intervals")?,
                predicate: self.lambda(get("predicate")?)?,
                body: self.boxed(json, "body", what)?,
            }),
            "Sleep" => NodeKind::Sleep { milliseconds: integer(get("milliseconds")?, "sleep milliseconds")? },
            "Loop" => NodeKind::Loop(Loop {
                maximum: integer(get("maximum")?, "loop maximum")?,
                binders: array(get("binders")?, "loop binders")?.iter().map(|b| self.binder(b)).collect::<Result<_, _>>()?,
                initial: self.nodes(get("initial")?, "loop initial values")?,
                body: self.boxed(json, "body", what)?,
            }),
            "Recur" => NodeKind::Recur { args: self.nodes(get("args")?, "recur arguments")? },
            "Par" => NodeKind::Par { branches: self.nodes(get("branches")?, "par branches")? },
            "ParMap" => NodeKind::ParMap {
                limit: integer(get("limit")?, "par-map limit")?,
                function: self.lambda(get("function")?)?,
                items: self.boxed(json, "items", what)?,
            },
            _ => return Err(IrError(format!("unknown IR node {what:?}"))),
        };
        Ok(Node { kind, span })
    }
}

fn reader_for(enums: &Json) -> Result<Reader<'_>, IrError> {
    let entries = enums.as_object().ok_or_else(|| bad("enums", enums))?;
    Ok(Reader { raw_enums: entries.iter().map(|(k, v)| (k.clone(), v)).collect(), enums: BTreeMap::new(), resolving: Vec::new() })
}

pub fn read_type(json: &Json, enums: &Json) -> Result<Type, IrError> {
    reader_for(enums)?.type_of(json)
}

pub fn read_program(json: &Json) -> Result<Program, IrError> {
    let mut reader = reader_for(field(json, "enums", "program")?)?;
    let body = reader.node(field(json, "body", "program")?)?;
    let result = reader.type_of(field(json, "result", "program")?)?;
    let capabilities =
        array(field(json, "capabilities", "program")?, "capabilities")?.iter().map(|c| string(c, "capability")).collect::<Result<_, _>>()?;
    let mut inputs = BTreeMap::new();
    let declared = field(json, "inputs", "program")?;
    for (name, input_type) in declared.as_object().ok_or_else(|| bad("inputs", declared))? {
        inputs.insert(name.clone(), reader.type_of(input_type)?);
    }
    let signatures_json = field(json, "signatures", "program")?;
    let mut signatures = Signatures::default();
    let capability_json = field(signatures_json, "capabilities", "signatures")?;
    for (id, entry) in capability_json.as_object().ok_or_else(|| bad("capability signatures", capability_json))? {
        let effect_json = field(entry, "effect", "capability signature")?;
        let effect = effect_json.as_str().and_then(Effect::from_name).ok_or_else(|| bad("effect", effect_json))?;
        let idempotent_json = field(entry, "idempotent", "capability signature")?;
        signatures.capabilities.insert(
            id.clone(),
            CapabilitySignature {
                effect,
                idempotent: idempotent_json.as_bool().ok_or_else(|| bad("idempotent", idempotent_json))?,
                input: reader.type_of(field(entry, "input", "capability signature")?)?,
                output: reader.type_of(field(entry, "output", "capability signature")?)?,
            },
        );
    }
    let function_json = field(signatures_json, "functions", "signatures")?;
    for (name, entry) in function_json.as_object().ok_or_else(|| bad("function signatures", function_json))? {
        let params = array(field(entry, "params", "function signature")?, "function params")?
            .iter()
            .map(|t| reader.type_of(t))
            .collect::<Result<_, _>>()?;
        let output = reader.type_of(field(entry, "output", "function signature")?)?;
        signatures.functions.insert(name.clone(), FunctionSignature { params, output });
    }
    let names: Vec<String> = reader.raw_enums.keys().cloned().collect();
    for name in names {
        reader.enum_type(&name)?;
    }
    Ok(Program { body, result, capabilities, inputs, enums: reader.enums, signatures })
}

pub fn read_program_text(text: &str) -> Result<Program, IrError> {
    let json = parse(text).map_err(|e| IrError(format!("exported IR is not JSON: {e}")))?;
    read_program(&json)
}
