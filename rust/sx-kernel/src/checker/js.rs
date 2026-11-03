use super::*;

const SAFE_BOUND: f64 = 9_007_199_254_740_992.0;
const PROMOTED: &str = "#promoted";
const TRUTH: &str = "#truth";
const TYPED: &str = "#typed";
const ROUNDED: &str = "#rounded";
const RECEIVER: &str = "#receiver";
const SPREAD: &str = "#spread";

fn variable(name: &str, span: Span) -> ir::Node {
    node(Ir::Variable { name: name.to_string() }, span)
}

fn text(value: &str, span: Span) -> ir::Node {
    literal(Value::string(value), STRING, span)
}

fn boolean(value: bool, span: Span) -> ir::Node {
    literal(Value::Bool(value), BOOL, span)
}

fn integer(value: i64, span: Span) -> ir::Node {
    literal(Value::Int(value), INT, span)
}

fn bind(name: &str, value: ir::Node, body: ir::Node, span: Span) -> ir::Node {
    node(Ir::Let(ir::Let { binder: Binder::Name(name.to_string()), value: Box::new(value), body: Box::new(body) }), span)
}

fn discard(value: ir::Node, body: ir::Node, span: Span) -> ir::Node {
    node(Ir::Let(ir::Let { binder: Binder::Discard, value: Box::new(value), body: Box::new(body) }), span)
}

fn branch(condition: ir::Node, then: ir::Node, otherwise: ir::Node, span: Span) -> ir::Node {
    node(
        Ir::If(ir::If { condition: Box::new(condition), then: Box::new(then), otherwise: Box::new(otherwise) }),
        span,
    )
}

fn present(binder: &str) -> MatchPattern {
    MatchPattern::Some(Box::new(MatchPattern::Any(Binder::Name(binder.to_string()))))
}

fn when_present(value: ir::Node, binder: &str, then: ir::Node, otherwise: ir::Node, span: Span) -> ir::Node {
    node(
        Ir::Match(ir::Match {
            scrutinee: Box::new(value),
            clauses: vec![(present(binder), then), (MatchPattern::None, otherwise)],
        }),
        span,
    )
}

fn trap(t: &Type, span: Span) -> ir::Node {
    let empty = node(Ir::MakeList { items: Vec::new(), element: t.clone() }, span);
    prelude(PreludeOp::Nth, vec![empty, integer(0, span)], span)
}

fn some_of(value: ir::Node, wrapped: &Type, span: Span) -> ir::Node {
    node(Ir::Wrap { kind: WrapKind::Some, value: Box::new(value), wrapped_type: wrapped.clone() }, span)
}

pub(super) fn guard_present(narrowed: &Narrowed, body: ir::Node, result: &Type, span: Span) -> ir::Node {
    narrowed.iter().rev().fold(body, |body, (name, fact)| match fact {
        Fact::Present(_) => when_present(variable(name, span), name, body, trap(result, span), span),
        Fact::Variants { .. } => body,
    })
}

fn variant_operands<'a>(operands: &[&'a Node; 2]) -> Option<(&'a Node, &'a str, bool)> {
    [(operands[0], operands[1]), (operands[1], operands[0])].into_iter().find_map(|(subject, literal)| {
        let NodeKind::String(variant) = &literal.kind else {
            return None;
        };
        match form_items(subject) {
            Some([get, inner, key]) if symbol(get) == Some("get") && keyword(key) == Some("tag") => Some((inner, variant.as_str(), true)),
            _ if matches!(subject.kind, NodeKind::String(_)) => None,
            _ => Some((subject, variant.as_str(), false)),
        }
    })
}

pub(super) enum Slot<'a> {
    Value(Box<(ir::Node, Type)>),
    Function(&'a Node),
}

impl<'r> Checker<'r> {
    fn firm<'a>(&mut self, arg: &'a Node, scope: &Scope<'a>) -> Checked {
        let (value, t) = self.check(arg, None, scope);
        let resolved = self.resolve(&t);
        if let Type::Variable(id) = resolved {
            match self.shape(id).cloned() {
                Some(Shape::Bracket { element, .. }) => {
                    self.trial(&t, &list(element));
                }
                Some(Shape::Brace) => {
                    self.trial(&t, &empty_record());
                }
                None => {}
            }
        }
        (value, t)
    }

    fn textual(&self, t: &Type) -> bool {
        match self.resolve(t) {
            Type::Primitive(Primitive::String) => true,
            Type::Enum(e) => Checker::nullary(&e),
            Type::Nominal(n) => n.base == STRING,
            _ => false,
        }
    }

    fn primitive_like(&self, t: &Type) -> bool {
        self.textual(t) || matches!(self.resolve(t), Type::Primitive(Primitive::Int | Primitive::Float | Primitive::Bool))
    }

    fn js_core_call(&mut self, name: &str, args: Vec<ir::Node>, span: Span) -> ir::Node {
        if self.registry.function(name).is_none() {
            self.report(Diagnostic::new(
                "unknown-function",
                span,
                format!("{name} belongs to the js core, which this registry does not load."),
            ));
        }
        self.functions_used.insert(name.to_string());
        node(Ir::CoreCall { function: name.to_string(), args }, span)
    }

    pub(super) fn promote_numbers(&mut self, value: &ir::Node, actual: &Type, expected: &Type, span: Span) -> Option<ir::Node> {
        self.promote(value.clone(), actual, expected, span)
    }

    fn promote(&mut self, value: ir::Node, actual: &Type, expected: &Type, span: Span) -> Option<ir::Node> {
        let (a, e) = (self.resolve(actual), self.resolve(expected));
        let mark = self.snapshot();
        if self.unify(&a, &e) {
            return Some(value);
        }
        self.rollback(mark);
        match (&a, &e) {
            (Type::Primitive(Primitive::Int), Type::Primitive(Primitive::Float)) => {
                Some(prelude(PreludeOp::Float, vec![value], span))
            }
            (Type::Option(from), Type::Option(to)) => {
                let converted = self.promote(variable(PROMOTED, span), from, to, span)?;
                let wrapped = some_of(converted, &e, span);
                Some(when_present(value, PROMOTED, wrapped, literal(Value::None, e.clone(), span), span))
            }
            (_, Type::Option(inner)) if !matches!(a, Type::Never) => {
                let converted = self.promote(value, &a, inner, span)?;
                Some(some_of(converted, &e, span))
            }
            (Type::List(from), Type::List(to)) => {
                let converted = self.promote(variable(PROMOTED, span), from, to, span)?;
                let lambda = Lambda {
                    params: vec![Binder::Name(PROMOTED.to_string())],
                    param_types: vec![(**from).clone()],
                    result: (**to).clone(),
                    body: Box::new(converted),
                    captured: None,
                    span,
                };
                Some(prelude(PreludeOp::HigherOrder(HigherOrder::Map), vec![lambda_node(lambda), value], span))
            }
            (Type::Tuple(from), Type::Tuple(to)) if from.len() == to.len() => {
                let mut items = Vec::new();
                for (index, (x, y)) in from.iter().zip(to).enumerate() {
                    let part = prelude(PreludeOp::TupleItem, vec![variable(PROMOTED, span), integer(index as i64, span)], span);
                    items.push(self.promote(part, x, y, span)?);
                }
                Some(bind(PROMOTED, value, node(Ir::MakeTuple { items }, span), span))
            }
            (Type::Record(from), Type::Record(to)) if record_names(from) == record_names(to) => {
                let mut fields = Vec::new();
                for (name, wanted) in &to.fields {
                    let held = from.field(name)?.clone();
                    let part = node(Ir::Get { record: Box::new(variable(PROMOTED, span)), field: name.clone() }, span);
                    fields.push((name.clone(), self.promote(part, &held, wanted, span)?));
                }
                Some(bind(PROMOTED, value, node(Ir::MakeRecord { fields }, span), span))
            }
            _ => None,
        }
    }

    pub(super) fn safe_decoded(&mut self, value: ir::Node, t: &Type, span: Span) -> ir::Node {
        match self.guarded(value.clone(), t, span) {
            Some(guarded) => guarded,
            None => value,
        }
    }

    fn guarded(&mut self, value: ir::Node, t: &Type, span: Span) -> Option<ir::Node> {
        let resolved = self.resolve(t);
        match &resolved {
            Type::Primitive(Primitive::Int) => Some(self.safe(value, span)),
            Type::Option(inner) => {
                let body = self.guarded(variable(PROMOTED, span), inner, span)?;
                Some(when_present(value, PROMOTED, some_of(body, &resolved, span), literal(Value::None, resolved.clone(), span), span))
            }
            Type::List(element) => {
                let body = self.guarded(variable(PROMOTED, span), element, span)?;
                let lambda = Lambda {
                    params: vec![Binder::Name(PROMOTED.to_string())],
                    param_types: vec![(**element).clone()],
                    result: (**element).clone(),
                    body: Box::new(body),
                    captured: None,
                    span,
                };
                Some(prelude(PreludeOp::HigherOrder(HigherOrder::Map), vec![lambda_node(lambda), value], span))
            }
            Type::Tuple(parts) => {
                let mut changed = false;
                let mut items = Vec::new();
                for (index, part) in parts.iter().enumerate() {
                    let held = prelude(PreludeOp::TupleItem, vec![variable(PROMOTED, span), integer(index as i64, span)], span);
                    match self.guarded(held.clone(), part, span) {
                        Some(guarded) => {
                            changed = true;
                            items.push(guarded);
                        }
                        None => items.push(held),
                    }
                }
                changed.then(|| bind(PROMOTED, value, node(Ir::MakeTuple { items }, span), span))
            }
            Type::Record(record) => {
                let mut changed = false;
                let mut fields = Vec::new();
                for (name, field_type) in &record.fields {
                    let held = node(Ir::Get { record: Box::new(variable(PROMOTED, span)), field: name.clone() }, span);
                    match self.guarded(held.clone(), field_type, span) {
                        Some(guarded) => {
                            changed = true;
                            fields.push((name.clone(), guarded));
                        }
                        None => fields.push((name.clone(), held)),
                    }
                }
                changed.then(|| bind(PROMOTED, value, node(Ir::MakeRecord { fields }, span), span))
            }
            Type::Map(value_type) => {
                let pair = Type::Tuple(vec![STRING, (**value_type).clone()]);
                let entry = |index: i64| {
                    prelude(PreludeOp::TupleItem, vec![variable(PROMOTED, span), integer(index, span)], span)
                };
                let body = self.guarded(entry(1), value_type, span)?;
                let lambda = Lambda {
                    params: vec![Binder::Name(PROMOTED.to_string())],
                    param_types: vec![pair.clone()],
                    result: pair,
                    body: Box::new(node(Ir::MakeTuple { items: vec![entry(0), body] }, span)),
                    captured: None,
                    span,
                };
                let entries = prelude(PreludeOp::Entries, vec![value], span);
                let mapped = prelude(PreludeOp::HigherOrder(HigherOrder::Map), vec![lambda_node(lambda), entries], span);
                Some(prelude(PreludeOp::IntoMap, vec![mapped], span))
            }
            Type::Enum(found) => {
                let mut changed = false;
                let mut clauses = Vec::new();
                for (variant, payload) in &found.variants {
                    let guarded = payload.as_ref().and_then(|payload| self.guarded(variable(PROMOTED, span), payload, span));
                    changed |= guarded.is_some();
                    let held = payload.is_some().then(|| Box::new(guarded.unwrap_or_else(|| variable(PROMOTED, span))));
                    let pattern = MatchPattern::Variant {
                        variant: variant.clone(),
                        inner: payload.as_ref().map(|_| Box::new(MatchPattern::Any(Binder::Name(PROMOTED.to_string())))),
                    };
                    let rebuilt = node(
                        Ir::MakeVariant { enum_name: found.name.clone(), variant: variant.clone(), payload: held },
                        span,
                    );
                    clauses.push((pattern, rebuilt));
                }
                changed.then(|| node(Ir::Match(ir::Match { scrutinee: Box::new(value), clauses }), span))
            }
            Type::Result(ok, err) => {
                let guarded_ok = self.guarded(variable(PROMOTED, span), ok, span);
                let guarded_err = self.guarded(variable(PROMOTED, span), err, span);
                if guarded_ok.is_none() && guarded_err.is_none() {
                    return None;
                }
                let arm = |kind: WrapKind, guarded: Option<ir::Node>| {
                    let inner = guarded.unwrap_or_else(|| variable(PROMOTED, span));
                    node(Ir::Wrap { kind, value: Box::new(inner), wrapped_type: resolved.clone() }, span)
                };
                let clauses = vec![
                    (
                        MatchPattern::Ok(Box::new(MatchPattern::Any(Binder::Name(PROMOTED.to_string())))),
                        arm(WrapKind::Ok, guarded_ok),
                    ),
                    (
                        MatchPattern::Err(Box::new(MatchPattern::Any(Binder::Name(PROMOTED.to_string())))),
                        arm(WrapKind::Err, guarded_err),
                    ),
                ];
                Some(node(Ir::Match(ir::Match { scrutinee: Box::new(value), clauses }), span))
            }
            _ => None,
        }
    }

    pub(super) fn present_when_not_optional(&mut self, value: ir::Node, t: Type, span: Span) -> Checked {
        match self.resolve(&t) {
            Type::Option(_) | Type::Variable(_) | Type::Never => (value, t),
            _ => {
                let wrapped = option(t);
                (some_of(value, &wrapped, span), wrapped)
            }
        }
    }

    pub(super) fn settled_shape(&mut self, value: ir::Node, t: &Type, span: Span) -> Option<(ir::Node, Type)> {
        let resolved = self.resolve(t);
        match &resolved {
            Type::Result(ok, err) => {
                let (fulfilled, ok_type) = match self.settled_shape(variable(PROMOTED, span), ok, span) {
                    Some(shaped) => shaped,
                    None => (variable(PROMOTED, span), (**ok).clone()),
                };
                let (rejected, err_type) = match self.settled_shape(variable(PROMOTED, span), err, span) {
                    Some(shaped) => shaped,
                    None => (variable(PROMOTED, span), (**err).clone()),
                };
                let ok_record = Type::Record(RecordType::plain(vec![("status".to_string(), STRING), ("value".to_string(), ok_type)]));
                let err_record =
                    Type::Record(RecordType::plain(vec![("status".to_string(), STRING), ("reason".to_string(), err_type)]));
                let _sx = crate::types::use_style(crate::types::Style::Sx);
                let shaped = Arc::new(EnumType {
                    name: format!(
                        "{}{} | {}>",
                        crate::values::SHAPED_PREFIX,
                        crate::types::show_definition(&ok_record),
                        crate::types::show_definition(&err_record)
                    ),
                    variants: vec![
                        ("Fulfilled".to_string(), Some(ok_record)),
                        ("Rejected".to_string(), Some(err_record)),
                    ],
                });
                let arm = |variant: &str, status: &str, field: &str, payload: ir::Node| {
                    let record = node(
                        Ir::MakeRecord {
                            fields: vec![("status".to_string(), text(status, span)), (field.to_string(), payload)],
                        },
                        span,
                    );
                    node(
                        Ir::MakeVariant {
                            enum_name: shaped.name.clone(),
                            variant: variant.to_string(),
                            payload: Some(Box::new(record)),
                        },
                        span,
                    )
                };
                let clauses = vec![
                    (
                        MatchPattern::Ok(Box::new(MatchPattern::Any(Binder::Name(PROMOTED.to_string())))),
                        arm("Fulfilled", "fulfilled", "value", fulfilled),
                    ),
                    (
                        MatchPattern::Err(Box::new(MatchPattern::Any(Binder::Name(PROMOTED.to_string())))),
                        arm("Rejected", "rejected", "reason", rejected),
                    ),
                ];
                let made = node(Ir::Match(ir::Match { scrutinee: Box::new(value), clauses }), span);
                Some((made, Type::Enum(shaped)))
            }
            Type::Option(inner) => {
                let (body, shaped_type) = self.settled_shape(variable(PROMOTED, span), inner, span)?;
                let wrapped = option(shaped_type);
                Some((
                    when_present(value, PROMOTED, some_of(body, &wrapped, span), literal(Value::None, wrapped.clone(), span), span),
                    wrapped,
                ))
            }
            Type::List(element) => {
                let (body, shaped_type) = self.settled_shape(variable(PROMOTED, span), element, span)?;
                let lambda = Lambda {
                    params: vec![Binder::Name(PROMOTED.to_string())],
                    param_types: vec![(**element).clone()],
                    result: shaped_type.clone(),
                    body: Box::new(body),
                    captured: None,
                    span,
                };
                Some((prelude(PreludeOp::HigherOrder(HigherOrder::Map), vec![lambda_node(lambda), value], span), list(shaped_type)))
            }
            Type::Tuple(parts) => {
                let mut changed = false;
                let mut items = Vec::new();
                let mut types = Vec::new();
                for (index, part) in parts.iter().enumerate() {
                    let held = prelude(PreludeOp::TupleItem, vec![variable(PROMOTED, span), integer(index as i64, span)], span);
                    match self.settled_shape(held.clone(), part, span) {
                        Some((shaped, shaped_type)) => {
                            changed = true;
                            items.push(shaped);
                            types.push(shaped_type);
                        }
                        None => {
                            items.push(held);
                            types.push(part.clone());
                        }
                    }
                }
                changed.then(|| (bind(PROMOTED, value, node(Ir::MakeTuple { items }, span), span), Type::Tuple(types)))
            }
            Type::Record(record) => {
                let mut changed = false;
                let mut fields = Vec::new();
                let mut types = Vec::new();
                for (name, field_type) in &record.fields {
                    let held = node(Ir::Get { record: Box::new(variable(PROMOTED, span)), field: name.clone() }, span);
                    match self.settled_shape(held.clone(), field_type, span) {
                        Some((shaped, shaped_type)) => {
                            changed = true;
                            fields.push((name.clone(), shaped));
                            types.push((name.clone(), shaped_type));
                        }
                        None => {
                            fields.push((name.clone(), held));
                            types.push((name.clone(), field_type.clone()));
                        }
                    }
                }
                changed.then(|| (bind(PROMOTED, value, node(Ir::MakeRecord { fields }, span), span), Type::Record(RecordType::plain(types))))
            }
            _ => None,
        }
    }

    pub(super) fn js_number_list(&mut self, checked: &[Checked], span: Span) -> Option<ir::Node> {
        let kinds: Vec<Type> = checked.iter().map(|(_, t)| self.resolve(t)).collect();
        if !kinds.iter().all(|t| *t == INT || *t == FLOAT) || !kinds.contains(&INT) || !kinds.contains(&FLOAT) {
            return None;
        }
        let items = checked
            .iter()
            .zip(&kinds)
            .map(|((value, _), kind)| {
                if *kind == INT { prelude(PreludeOp::Float, vec![value.clone()], span) } else { value.clone() }
            })
            .collect();
        Some(node(Ir::MakeList { items, element: FLOAT }, span))
    }

    pub(super) fn js_ordered_key(&mut self, mut function: Lambda) -> Lambda {
        let span = function.span;
        let body = std::mem::replace(&mut function.body, Box::new(boolean(false, span)));
        function.body = Box::new(self.js_core_call("js.order-key", vec![*body], span));
        function
    }

    fn safe(&self, value: ir::Node, span: Span) -> ir::Node {
        prelude(PreludeOp::SafeInt, vec![value], span)
    }

    pub(super) fn js_operator<'a>(
        &mut self,
        syntax: &'a Node,
        items: &'a [Node],
        op: &str,
        expected: Option<&Type>,
        scope: &Scope<'a>,
    ) -> Option<Checked> {
        let span = syntax.span;
        let args = &items[1..];
        let (low, high) = match op {
            "-" => (1, 2),
            "+" | "*" | "/" | "rem" | "<" | "<=" | ">" | ">=" | "=" | "not=" => (2, 2),
            "not" | "floor" | "ceil" | "round" => (1, 1),
            "str" => (1, usize::MAX),
            _ => return None,
        };
        if !self.arity(span, items, low, (high != usize::MAX).then_some(high), signature(op)) {
            return Some(self.placeholder(span));
        }
        Some(match op {
            "+" => self.js_plus(span, args, expected, scope),
            "-" | "*" => {
                let operands: Vec<Checked> = args.iter().map(|arg| self.check(arg, None, scope)).collect();
                let prelude_op = if op == "-" { PreludeOp::Subtract } else { PreludeOp::Multiply };
                self.js_arithmetic(prelude_op, operands, args, span, expected)
            }
            "/" => {
                let operands: Vec<Checked> = args.iter().map(|arg| self.check(arg, None, scope)).collect();
                self.js_arithmetic(PreludeOp::Divide, operands, args, span, expected)
            }
            "rem" => self.js_remainder(span, args, expected, scope),
            "<" | "<=" | ">" | ">=" => self.js_compare(op, span, args, expected, scope),
            "=" | "not=" => self.js_equal(op, span, args, expected, scope),
            "not" => self.js_not(span, args, expected, scope),
            "floor" | "ceil" | "round" => self.js_round(op, span, args, expected, scope),
            _ => self.js_str(span, args, expected, scope),
        })
    }

    fn js_arithmetic(
        &mut self,
        op: PreludeOp,
        operands: Vec<Checked>,
        args: &[Node],
        span: Span,
        expected: Option<&Type>,
    ) -> Checked {
        let float = op == PreludeOp::Divide || operands.iter().any(|(_, t)| self.resolve(t) == FLOAT);
        let number = if float { FLOAT } else { INT };
        let mut coerced = Vec::new();
        for ((value, t), arg) in operands.into_iter().zip(args) {
            let options = Coercion { source: Some(arg), ..Coercion::default() };
            coerced.push(self.coerce_with(value, &t, Some(&number), arg.span, options).0);
        }
        let single = coerced.len() == 1;
        let mut made = prelude(op, coerced, span);
        if number == INT && !single && op != PreludeOp::Divide {
            made = self.safe(made, span);
        }
        self.coerce(made, &number, expected, span)
    }

    fn js_plus<'a>(&mut self, span: Span, args: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        let operands: Vec<Checked> = args.iter().map(|arg| self.check(arg, None, scope)).collect();
        if operands.iter().any(|(_, t)| self.textual(t)) {
            let mut parts = Vec::new();
            for ((value, t), arg) in operands.into_iter().zip(args) {
                match self.js_text(value, &t, arg.span) {
                    Some(part) => parts.push(part),
                    None => {
                        let found = describe(&self.zonk(&t));
                        self.report(
                            Diagnostic::new(
                                "js-coercion",
                                arg.span,
                                format!("This + converts {found} to text, which JavaScript does with a different result."),
                            )
                            .detail("Only a String, Int, Float, or Bool can be added to a String.")
                            .fix("write the text you mean"),
                        );
                    }
                }
            }
            return self.coerce(prelude(PreludeOp::Str, parts, span), &STRING, expected, span);
        }
        self.js_arithmetic(PreludeOp::Add, operands, args, span, expected)
    }

    fn js_remainder<'a>(&mut self, span: Span, args: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        let mut operands = Vec::new();
        for arg in args {
            let (value, t) = self.check(arg, None, scope);
            if self.resolve(&t) == FLOAT {
                self.report(
                    Diagnostic::new("js-float-remainder", arg.span, "% on a Float is not supported.")
                        .detail("JavaScript's remainder of a Float can be NaN or a fraction; only Int % Int is supported.")
                        .fix("Math.floor(x) % n"),
                );
                operands.push(value);
                continue;
            }
            operands.push(self.coerce_with(value, &t, Some(&INT), arg.span, Coercion::default()).0);
        }
        self.coerce(prelude(PreludeOp::Rem, operands, span), &INT, expected, span)
    }

    fn js_compare<'a>(
        &mut self,
        op: &str,
        span: Span,
        args: &'a [Node],
        expected: Option<&Type>,
        scope: &Scope<'a>,
    ) -> Checked {
        let prelude_op = PreludeOp::from_name(op).unwrap_or(PreludeOp::Less);
        let mut operands: Vec<Checked> = args.iter().map(|arg| self.check(arg, None, scope)).collect();
        let kinds: Vec<Type> = operands.iter().map(|(_, t)| self.resolve(t)).collect();
        if kinds.iter().all(|t| *t == STRING) {
            let keys = operands.drain(..).map(|(value, _)| self.js_core_call("js.order-key", vec![value], span)).collect();
            return self.coerce(prelude(prelude_op, keys, span), &BOOL, expected, span);
        }
        if kinds.iter().any(|t| *t == INT || *t == FLOAT) {
            let number = if kinds.contains(&FLOAT) { FLOAT } else { INT };
            let mut coerced = Vec::new();
            for ((value, t), arg) in operands.into_iter().zip(args) {
                let options = Coercion { source: Some(arg), ..Coercion::default() };
                coerced.push(self.coerce_with(value, &t, Some(&number), arg.span, options).0);
            }
            return self.coerce(prelude(prelude_op, coerced, span), &BOOL, expected, span);
        }
        let first = self.resolve(&kinds[0]);
        if !matches!(first, Type::Never) && !self.is_open(&first) {
            let hint = matches!(first, Type::Option(_)).then(|| OPTIONAL_HINT.to_string());
            let message =
                format!("{op} compares integers, floats, or strings, but this is {}.", describe(&self.zonk(&first)));
            self.report(Diagnostic::new("type-mismatch", args[0].span, message).maybe_detail(hint));
        }
        let values = operands.into_iter().map(|(value, _)| value).collect();
        self.coerce(prelude(prelude_op, values, span), &BOOL, expected, span)
    }

    fn js_equal<'a>(
        &mut self,
        op: &str,
        span: Span,
        args: &'a [Node],
        expected: Option<&Type>,
        scope: &Scope<'a>,
    ) -> Checked {
        let prelude_op = if op == "=" { PreludeOp::Equal } else { PreludeOp::NotEqual };
        let against_null = args.iter().position(|arg| symbol(arg) == Some("none"));
        if let Some(at) = against_null {
            let other = &args[1 - at];
            self.check(other, None, scope);
            self.report(
                Diagnostic::new("js-null-equality", span, "=== null and !== null are not supported.")
                    .detail(
                        "A value that is absent can be undefined as well as null in JavaScript, so only a presence \
                         test is supported.",
                    )
                    .fix("x == null"),
            );
            return self.coerce(boolean(false, span), &BOOL, expected, span);
        }
        if let Some(checked) = self.variant_equality(op, span, args, expected, scope) {
            return checked;
        }
        let variant_first = symbol(&args[0]).is_some_and(|name| first_upper(name) && scope.get(name).is_none());
        let string_first = matches!(args[0].kind, NodeKind::String(_)) && !matches!(args[1].kind, NodeKind::String(_));
        let (one, two) = if variant_first || string_first { (1, 0) } else { (0, 1) };
        let (first, first_type) = self.check(&args[one], None, scope);
        let resolved_first = self.resolve(&first_type);
        let hint = matches!(&resolved_first, Type::Enum(e) if Checker::nullary(e)).then(|| first_type.clone());
        let (second, second_type) = self.check(&args[two], hint.as_ref(), scope);
        let (pair, pair_types) = if one == 0 {
            (vec![first, second], [first_type, second_type])
        } else {
            (vec![second, first], [second_type, first_type])
        };
        let base = |checker: &Checker<'_>, t: &Type| match checker.resolve(t) {
            Type::Option(inner) => (checker.resolve(&inner), true),
            other => (other, false),
        };
        let ((left, left_option), (right, right_option)) = (base(self, &pair_types[0]), base(self, &pair_types[1]));
        if matches!(left, Type::Never) || matches!(right, Type::Never) {
            return self.coerce(prelude(prelude_op, pair, span), &BOOL, expected, span);
        }
        let open = |checker: &Checker<'_>, t: &Type| checker.is_open(t);
        if !(self.primitive_like(&left) || open(self, &left)) || !(self.primitive_like(&right) || open(self, &right)) {
            self.report(
                Diagnostic::new("js-equality", span, "=== compares references here, and Sx compares values.")
                    .detail("Only strings, numbers, booleans, and optional ones of those can be compared.")
                    .fix("compare fields"),
            );
            return self.coerce(boolean(false, span), &BOOL, expected, span);
        }
        let number = (left == INT || left == FLOAT) && (right == INT || right == FLOAT);
        let base_type = if number && (left == FLOAT || right == FLOAT) {
            FLOAT
        } else if self.is_open(&left) {
            right.clone()
        } else {
            left.clone()
        };
        let target = if left_option || right_option { option(base_type) } else { base_type };
        let mut coerced = Vec::new();
        for (index, value) in pair.into_iter().enumerate() {
            let arg = &args[if one == 0 { index } else { 1 - index }];
            let options = Coercion { source: Some(arg), ..Coercion::default() };
            coerced.push(self.coerce_with(value, &pair_types[index], Some(&target), arg.span, options).0);
        }
        self.coerce(prelude(prelude_op, coerced, span), &BOOL, expected, span)
    }

    fn js_not<'a>(&mut self, span: Span, args: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        let (value, t) = self.check(&args[0], None, scope);
        if let Type::Option(_) = self.resolve(&t) {
            let absent = when_present(value, PROMOTED, boolean(false, span), boolean(true, span), span);
            return self.coerce(absent, &BOOL, expected, span);
        }
        let value = self.coerce_with(value, &t, Some(&BOOL), args[0].span, Coercion::default()).0;
        self.coerce(prelude(PreludeOp::Not, vec![value], span), &BOOL, expected, span)
    }

    fn js_round<'a>(
        &mut self,
        op: &str,
        span: Span,
        args: &'a [Node],
        expected: Option<&Type>,
        scope: &Scope<'a>,
    ) -> Checked {
        let (value, t) = self.check(&args[0], None, scope);
        if self.resolve(&t) == INT {
            return self.coerce(value, &INT, expected, span);
        }
        let value = self.coerce_with(value, &t, Some(&FLOAT), args[0].span, Coercion::default()).0;
        let prelude_op = PreludeOp::from_name(op).unwrap_or(PreludeOp::Floor);
        let float = |n: f64| literal(Value::float(n), FLOAT, span);
        let low = prelude(PreludeOp::GreaterEqual, vec![variable(ROUNDED, span), float(-SAFE_BOUND)], span);
        let high = prelude(PreludeOp::LessEqual, vec![variable(ROUNDED, span), float(SAFE_BOUND)], span);
        let within = prelude(PreludeOp::And, vec![low, high], span);
        let rounded = prelude(prelude_op, vec![variable(ROUNDED, span)], span);
        let guarded = bind(ROUNDED, value, branch(within, rounded, trap(&INT, span), span), span);
        self.coerce(guarded, &INT, expected, span)
    }

    fn js_str<'a>(&mut self, span: Span, args: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        let mut parts = Vec::new();
        for arg in args {
            let (value, t) = self.check(arg, None, scope);
            match self.js_text(value.clone(), &t, arg.span) {
                Some(part) => parts.push(part),
                None => {
                    let found = describe(&self.zonk(&t));
                    self.report(
                        Diagnostic::new("js-coercion", arg.span, format!("JavaScript would write {found} as text differently."))
                            .detail("Only a String, Int, Float, Bool, or an optional one of those can be written as text."),
                    );
                    parts.push(value);
                }
            }
        }
        self.coerce(prelude(PreludeOp::Str, parts, span), &STRING, expected, span)
    }

    pub(super) fn js_text(&mut self, value: ir::Node, t: &Type, span: Span) -> Option<ir::Node> {
        let resolved = self.resolve(t);
        match &resolved {
            Type::Primitive(Primitive::String) => Some(value),
            Type::Primitive(Primitive::Int | Primitive::Bool) => Some(prelude(PreludeOp::Str, vec![value], span)),
            Type::Primitive(Primitive::Float) => Some(self.js_core_call("js.float-text", vec![value], span)),
            Type::Enum(e) if Checker::nullary(e) => Some(prelude(PreludeOp::Str, vec![value], span)),
            Type::Nominal(n) if n.base == STRING => Some(value),
            Type::Primitive(Primitive::Unit) | Type::Option(_) if self.is_null_only(&resolved) => {
                Some(discard(value, text("null", span), span))
            }
            Type::Option(inner) => {
                let shown = self.js_text(variable(PROMOTED, span), inner, span)?;
                Some(when_present(value, PROMOTED, shown, text("null", span), span))
            }
            _ => None,
        }
    }

    fn is_null_only(&self, t: &Type) -> bool {
        match self.resolve(t) {
            Type::Primitive(Primitive::Unit) => true,
            Type::Option(inner) => matches!(self.resolve(&inner), Type::Never | Type::Variable(_)),
            _ => false,
        }
    }

    pub(super) fn js_displayed(&mut self, value: ir::Node, t: &Type, span: Span) -> ir::Node {
        match self.js_text(value.clone(), t, span) {
            Some(shown) => shown,
            None => {
                let (value, t) = self.settled_shape(value.clone(), t, span).unwrap_or((value, t.clone()));
                self.late_json_values.push((t, span, "js.stringify"));
                self.js_core_call("js.stringify", vec![value], span)
            }
        }
    }

    pub(super) fn present_operand<'a>(&self, node: &'a Node, scope: &Scope<'a>) -> Option<&'a Node> {
        if self.dialect != Dialect::JavaScript || scope.get("present").is_some() {
            return None;
        }
        match form_items(node)? {
            [head, operand] if symbol(head) == Some("present") => Some(operand),
            _ => None,
        }
    }

    pub(super) fn check_present_test<'a>(
        &mut self,
        operand: &'a Node,
        node: &'a Node,
        scope: &Scope<'a>,
        who: &'static str,
    ) -> Test {
        let (value, t) = self.outside_tail(|c| c.check(operand, None, scope));
        match self.resolve(&t) {
            Type::Option(inner_type) if !matches!(self.resolve(&inner_type), Type::Never) => {
                self.presence_test(value, &inner_type, operand, scope, who)
            }
            _ => {
                let condition = self.presence_node(value, &t, node.span, operand.span);
                Test::plain(condition)
            }
        }
    }

    pub(super) fn form_present<'a>(
        &mut self,
        span: Span,
        items: &'a [Node],
        expected: Option<&Type>,
        scope: &Scope<'a>,
    ) -> Checked {
        if !self.arity(span, items, 1, Some(1), "value") {
            return self.placeholder(span);
        }
        let (value, t) = self.firm(&items[1], scope);
        let presence = self.presence_node(value, &t, span, items[1].span);
        self.coerce(presence, &BOOL, expected, span)
    }

    fn presence_node(&mut self, value: ir::Node, t: &Type, span: Span, at: Span) -> ir::Node {
        match self.resolve(t) {
            Type::Option(_) => when_present(value, PROMOTED, boolean(true, span), boolean(false, span), span),
            Type::Primitive(Primitive::Unit) => discard(value, boolean(false, span), span),
            Type::Variable(_) => {
                self.report(Diagnostic::new(
                    "unresolved-type",
                    at,
                    "The checker cannot tell the type of this value, so it cannot say whether it is null.",
                ));
                boolean(false, span)
            }
            _ => discard(value, boolean(true, span), span),
        }
    }

    pub(super) fn with_narrowings<'a>(&mut self, mut test: Test, node: &'a Node, scope: &Scope<'a>) -> Test {
        if self.dialect == Dialect::JavaScript {
            let (when_true, when_false) = self.narrowings(node, scope);
            test.narrow_true = when_true;
            test.narrow_false = when_false;
        }
        test
    }

    fn narrowings<'a>(&mut self, node: &'a Node, scope: &Scope<'a>) -> (Narrowed, Narrowed) {
        let none = (Vec::new(), Vec::new());
        let Some([head, rest @ ..]) = form_items(node) else {
            return none;
        };
        let Some(head) = symbol(head).filter(|h| scope.get(h).is_none()) else {
            return none;
        };
        let literal = |node: &Node, expected: bool| matches!(node.kind, NodeKind::Bool(b) if b == expected);
        let merge = |mut first: Narrowed, second: Narrowed| {
            for entry in second {
                match first.iter_mut().find(|(name, _)| *name == entry.0) {
                    None => first.push(entry),
                    Some((_, Fact::Variants { names, .. })) => {
                        if let (_, Fact::Variants { names: other, .. }) = &entry {
                            names.retain(|n| other.contains(n));
                        }
                    }
                    Some(_) => {}
                }
            }
            first
        };
        match (head, rest) {
            ("present" | "truthy", [operand]) => {
                let Some(name) = symbol(operand).filter(|n| scope.is_value(n)) else {
                    return none;
                };
                let Some(Binding::Value(t)) = scope.get(name) else {
                    return none;
                };
                match self.resolve(t) {
                    Type::Option(inner) if !matches!(self.resolve(&inner), Type::Never) => {
                        (vec![(name.to_string(), Fact::Present(*inner))], Vec::new())
                    }
                    _ => none,
                }
            }
            ("not", [operand]) => {
                let (when_true, when_false) = self.narrowings(operand, scope);
                (when_false, when_true)
            }
            ("=" | "not=", [first, second]) => {
                let Some((subject, variant, _)) = variant_operands(&[first, second]) else {
                    return none;
                };
                let Some((name, shaped)) = self.variant_subject(subject, scope) else {
                    return none;
                };
                if shaped.variant(variant).is_none() {
                    return none;
                }
                let current: Vec<String> = match scope.variants_of(&name) {
                    Some(names) => names.to_vec(),
                    None => shaped.variants.iter().map(|(v, _)| v.clone()).collect(),
                };
                let narrowed = |names: Vec<String>| {
                    if names.is_empty() || names.len() == current.len() {
                        return Vec::new();
                    }
                    vec![(name.clone(), Fact::Variants { enum_type: Type::Enum(shaped.clone()), names })]
                };
                let named = narrowed(current.iter().filter(|v| *v == variant).cloned().collect());
                let rest = narrowed(current.iter().filter(|v| *v != variant).cloned().collect());
                if head == "=" { (named, rest) } else { (rest, named) }
            }
            ("if", [first, then, otherwise]) if literal(otherwise, false) => {
                let (first_true, _) = self.narrowings(first, scope);
                let (then_true, _) = self.narrowings(then, scope);
                (merge(first_true, then_true), Vec::new())
            }
            ("if", [first, then, otherwise]) if literal(then, true) => {
                let (_, first_false) = self.narrowings(first, scope);
                let (_, otherwise_false) = self.narrowings(otherwise, scope);
                (Vec::new(), merge(first_false, otherwise_false))
            }
            _ => none,
        }
    }

    fn variant_subject<'a>(&self, subject: &'a Node, scope: &Scope<'a>) -> Option<(String, Arc<EnumType>)> {
        let name = symbol(subject).filter(|n| scope.is_value(n))?;
        let Some(Binding::Value(t)) = scope.get(name) else {
            return None;
        };
        match self.resolve(t) {
            Type::Enum(e) if !e.name.starts_with(crate::values::SHAPED_PREFIX) && !Checker::nullary(&e) => {
                Some((name.to_string(), e))
            }
            _ => None,
        }
    }

    fn variant_is(value: ir::Node, found: &EnumType, variant: &str, negate: bool, span: Span) -> ir::Node {
        let clauses = found
            .variants
            .iter()
            .map(|(name, payload)| {
                let inner = payload.as_ref().map(|_| Box::new(MatchPattern::Any(Binder::Discard)));
                (MatchPattern::Variant { variant: name.clone(), inner }, boolean((name == variant) != negate, span))
            })
            .collect();
        node(Ir::Match(ir::Match { scrutinee: Box::new(value), clauses }), span)
    }

    pub(super) fn variant_equality<'a>(
        &mut self,
        op: &str,
        span: Span,
        args: &'a [Node],
        expected: Option<&Type>,
        scope: &Scope<'a>,
    ) -> Option<Checked> {
        let (subject, variant, through_tag) = variant_operands(&[&args[0], &args[1]])?;
        if !through_tag && self.variant_subject(subject, scope).is_none() {
            return None;
        }
        let speculation = Speculation::start(self);
        let (value, t) = self.check(subject, None, scope);
        let eligible = match self.resolve(&t) {
            Type::Enum(found) if !found.name.starts_with(crate::values::SHAPED_PREFIX) && !Checker::nullary(&found) => {
                match found.variant(variant) {
                    Some(Some(_)) => through_tag.then_some(found),
                    Some(None) => (!through_tag).then_some(found),
                    None => None,
                }
            }
            _ => None,
        };
        let Some(found) = eligible else {
            speculation.undo(self);
            return None;
        };
        let test = Checker::variant_is(value, &found, variant, op == "not=", span);
        Some(self.coerce(test, &BOOL, expected, span))
    }

    pub(super) fn enum_read(
        &mut self,
        value: ir::Node,
        found: &EnumType,
        field: &str,
        narrowed: Option<&[String]>,
        span: Span,
        expected: Option<&Type>,
    ) -> Option<Checked> {
        let names: Vec<String> = match narrowed {
            Some(names) => names.to_vec(),
            None => found.variants.iter().map(|(v, _)| v.clone()).collect(),
        };
        let carried = |name: &String| matches!(found.variant(name), Some(Some(_)));
        let (read, t) = match field {
            "tag" if names.iter().all(carried) => {
                let clauses = found
                    .variants
                    .iter()
                    .map(|(name, payload)| {
                        let inner = payload.as_ref().map(|_| Box::new(MatchPattern::Any(Binder::Discard)));
                        let body = if names.contains(name) { text(name, span) } else { trap(&STRING, span) };
                        (MatchPattern::Variant { variant: name.clone(), inner }, body)
                    })
                    .collect();
                (node(Ir::Match(ir::Match { scrutinee: Box::new(value), clauses }), span), STRING)
            }
            "value" if names.len() == 1 && carried(&names[0]) => {
                let Some(Some(payload)) = found.variant(&names[0]).cloned() else {
                    return None;
                };
                let clauses = found
                    .variants
                    .iter()
                    .map(|(name, held)| {
                        let (inner, body) = match held {
                            Some(_) if *name == names[0] => (
                                Some(Box::new(MatchPattern::Any(Binder::Name(PROMOTED.to_string())))),
                                variable(PROMOTED, span),
                            ),
                            Some(_) => (Some(Box::new(MatchPattern::Any(Binder::Discard))), trap(&payload, span)),
                            None => (None, trap(&payload, span)),
                        };
                        (MatchPattern::Variant { variant: name.clone(), inner }, body)
                    })
                    .collect::<Vec<_>>();
                (node(Ir::Match(ir::Match { scrutinee: Box::new(value), clauses }), span), payload)
            }
            _ => return None,
        };
        Some(self.coerce(read, &t, expected, span))
    }

    pub(super) fn form_truthy<'a>(
        &mut self,
        span: Span,
        items: &'a [Node],
        expected: Option<&Type>,
        scope: &Scope<'a>,
    ) -> Checked {
        if !self.arity(span, items, 1, Some(1), "value") {
            return self.placeholder(span);
        }
        let (value, t) = self.firm(&items[1], scope);
        let truth = self.truthiness(value, &t, span, items[1].span);
        self.coerce(truth, &BOOL, expected, span)
    }

    fn truthiness(&mut self, value: ir::Node, t: &Type, span: Span, at: Span) -> ir::Node {
        let resolved = self.resolve(t);
        let not_equal = |value: ir::Node, empty: ir::Node| prelude(PreludeOp::NotEqual, vec![value, empty], span);
        match &resolved {
            Type::Never => boolean(false, span),
            Type::Primitive(Primitive::Bool) => value,
            Type::Primitive(Primitive::String) => not_equal(value, text("", span)),
            Type::Primitive(Primitive::Int) => not_equal(value, integer(0, span)),
            Type::Primitive(Primitive::Float) => not_equal(value, literal(Value::float(0.0), FLOAT, span)),
            Type::Primitive(Primitive::Unit) => discard(value, boolean(false, span), span),
            Type::Primitive(Primitive::Bytes) | Type::Nominal(_) => {
                self.report(Diagnostic::new(
                    "js-truthiness",
                    at,
                    format!("Whether {} is truthy depends on its text, which Sx cannot test here.", describe(&self.zonk(&resolved))),
                ));
                boolean(false, span)
            }
            Type::Option(inner) => {
                let inner = self.resolve(inner);
                let scalar = matches!(
                    inner,
                    Type::Primitive(Primitive::Bool | Primitive::String | Primitive::Int | Primitive::Float)
                );
                let some = if scalar { self.truthiness(variable(TRUTH, span), &inner, span, at) } else { boolean(true, span) };
                when_present(value, TRUTH, some, boolean(false, span), span)
            }
            Type::Variable(_) => {
                self.report(Diagnostic::new(
                    "unresolved-type",
                    at,
                    "The checker cannot tell the type of this condition, so it cannot say when it is truthy.",
                ));
                boolean(false, span)
            }
            _ => discard(value, boolean(true, span), span),
        }
    }

    pub(super) fn form_typeof<'a>(
        &mut self,
        span: Span,
        items: &'a [Node],
        expected: Option<&Type>,
        scope: &Scope<'a>,
    ) -> Checked {
        if !self.arity(span, items, 1, Some(1), "value") {
            return self.placeholder(span);
        }
        let (value, t) = self.firm(&items[1], scope);
        let name = self.typeof_node(value, &t, span);
        self.coerce(name, &STRING, expected, span)
    }

    fn typeof_node(&mut self, value: ir::Node, t: &Type, span: Span) -> ir::Node {
        let resolved = self.resolve(t);
        match &resolved {
            Type::Option(inner) => {
                let inner = self.resolve(inner);
                if matches!(inner, Type::Never | Type::Variable(_)) {
                    return discard(value, text("object", span), span);
                }
                let shown = self.typeof_node(variable(TYPED, span), &inner, span);
                when_present(value, TYPED, shown, text("object", span), span)
            }
            Type::Enum(e) if !Checker::nullary(e) && e.variants.iter().any(|(_, payload)| payload.is_none()) => {
                let clauses = e
                    .variants
                    .iter()
                    .map(|(name, payload)| {
                        let inner = payload.as_ref().map(|_| Box::new(MatchPattern::Any(Binder::Discard)));
                        let kind = if payload.is_some() { "object" } else { "string" };
                        (MatchPattern::Variant { variant: name.clone(), inner }, text(kind, span))
                    })
                    .collect();
                discard(value.clone(), node(Ir::Match(ir::Match { scrutinee: Box::new(value), clauses }), span), span)
            }
            _ => {
                let name = match &resolved {
                    Type::Primitive(Primitive::Int | Primitive::Float) => "number",
                    Type::Primitive(Primitive::Bool) => "boolean",
                    Type::Primitive(Primitive::String | Primitive::Bytes) | Type::Nominal(_) => "string",
                    Type::Enum(e) if Checker::nullary(e) => "string",
                    _ => "object",
                };
                discard(value, text(name, span), span)
            }
        }
    }

    fn is_array_node(&mut self, value: ir::Node, t: &Type, span: Span) -> ir::Node {
        match self.resolve(t) {
            Type::Option(inner) => {
                let shown = self.is_array_node(variable(TYPED, span), &inner, span);
                when_present(value, TYPED, shown, boolean(false, span), span)
            }
            Type::List(_) | Type::Tuple(_) => discard(value, boolean(true, span), span),
            _ => discard(value, boolean(false, span), span),
        }
    }

    pub(super) fn form_record_spread<'a>(
        &mut self,
        span: Span,
        items: &'a [Node],
        expected: Option<&Type>,
        scope: &Scope<'a>,
    ) -> Checked {
        #[derive(Clone)]
        enum Source {
            Whole(usize),
            Field(usize, String),
            Text(&'static str),
            Settled,
        }
        #[derive(Clone)]
        struct Entry {
            name: String,
            t: Type,
            source: Source,
        }
        enum Part {
            Fields(Vec<Entry>),
            Settled(Type, Type),
        }
        let hint = match self.hint(expected) {
            Type::Record(record) => Some(record),
            _ => None,
        };
        let mut bound: Vec<ir::Node> = Vec::new();
        let mut parts: Vec<Part> = Vec::new();
        for (index, item) in items[1..].iter().enumerate() {
            let pieces = form_items(item).unwrap_or(&[]);
            let kind = pieces.first().and_then(keyword);
            match (kind, pieces.len()) {
                (Some("spread"), 2) => {
                    let (value, t) = self.check(&pieces[1], None, scope);
                    bound.push(value);
                    match self.resolve(&t) {
                        Type::Record(record) if !record.open => {
                            let entries = record
                                .fields
                                .iter()
                                .map(|(name, field_type)| Entry {
                                    name: name.clone(),
                                    t: field_type.clone(),
                                    source: Source::Field(index, name.clone()),
                                })
                                .collect();
                            parts.push(Part::Fields(entries));
                        }
                        Type::Result(ok, err) if err.is_call_error() => parts.push(Part::Settled(*ok, *err)),
                        Type::Never => parts.push(Part::Fields(Vec::new())),
                        other => {
                            let found = describe(&self.zonk(&other));
                            self.report(
                                Diagnostic::new(
                                    "ts-spread",
                                    pieces[1].span,
                                    "Only records with declared fields can be spread into an object",
                                )
                                .detail(format!("This is {found}."))
                                .fix("write the fields"),
                            );
                            parts.push(Part::Fields(Vec::new()));
                        }
                    }
                }
                (Some("field"), 3) => {
                    let NodeKind::String(name) = &pieces[1].kind else {
                        self.report(Diagnostic::new("bad-literal", pieces[1].span, "A field of record-spread names it with a string."));
                        bound.push(literal(Value::Unit, UNIT, span));
                        parts.push(Part::Fields(Vec::new()));
                        continue;
                    };
                    let wanted = hint.as_ref().and_then(|record| record.field(name)).cloned();
                    let (value, t) = self.check(&pieces[2], wanted.as_ref(), scope);
                    bound.push(value);
                    parts.push(Part::Fields(vec![Entry { name: name.clone(), t, source: Source::Whole(index) }]));
                }
                _ => {
                    self.report(Diagnostic::new(
                        "bad-literal",
                        item.span,
                        "record-spread takes (:spread value) and (:field \"name\" value) items.",
                    ));
                    bound.push(literal(Value::Unit, UNIT, span));
                    parts.push(Part::Fields(Vec::new()));
                }
            }
        }
        let settled: Vec<usize> =
            parts.iter().enumerate().filter(|(_, p)| matches!(p, Part::Settled(..))).map(|(at, _)| at).collect();
        if settled.len() > 1 {
            self.report(
                Diagnostic::new("ts-spread", span, "Only records with declared fields can be spread into an object")
                    .detail("A literal can spread at most one result of Promise.allSettled.")
                    .fix("write the fields"),
            );
            return self.placeholder(span);
        }
        let shapes: Vec<Option<bool>> = if settled.is_empty() { vec![None] } else { vec![Some(true), Some(false)] };
        let held = |index: usize| format!("{SPREAD}{index}");
        let mut built: Vec<(Vec<Entry>, ir::Node)> = Vec::new();
        for shape in shapes {
            let mut entries: Vec<Entry> = Vec::new();
            for part in &parts {
                let contributed = match (part, shape) {
                    (Part::Fields(fields), _) => fields.clone(),
                    (Part::Settled(ok, _), Some(true)) => vec![
                        Entry { name: "status".to_string(), t: STRING, source: Source::Text("fulfilled") },
                        Entry { name: "value".to_string(), t: ok.clone(), source: Source::Settled },
                    ],
                    (Part::Settled(_, err), _) => vec![
                        Entry { name: "status".to_string(), t: STRING, source: Source::Text("rejected") },
                        Entry { name: "reason".to_string(), t: err.clone(), source: Source::Settled },
                    ],
                };
                for entry in contributed {
                    match entries.iter_mut().find(|e| e.name == entry.name) {
                        Some(existing) => *existing = entry,
                        None => entries.push(entry),
                    }
                }
            }
            let fields: Vec<(String, ir::Node)> = entries
                .iter()
                .map(|entry| {
                    let part = match &entry.source {
                        Source::Whole(at) => variable(&held(*at), span),
                        Source::Field(at, field) => {
                            node(Ir::Get { record: Box::new(variable(&held(*at), span)), field: field.clone() }, span)
                        }
                        Source::Text(shown) => text(shown, span),
                        Source::Settled => variable(PROMOTED, span),
                    };
                    (entry.name.clone(), part)
                })
                .collect();
            built.push((entries, node(Ir::MakeRecord { fields }, span)));
        }
        let (result_type, mut body) = match settled.first() {
            None => {
                let Some((entries, record)) = built.pop() else {
                    return self.placeholder(span);
                };
                (Type::Record(RecordType::plain(entries.into_iter().map(|e| (e.name, e.t)).collect())), record)
            }
            Some(at) => {
                let mut records = built.into_iter();
                let (Some((ok_entries, ok_record)), Some((err_entries, err_record))) = (records.next(), records.next()) else {
                    return self.placeholder(span);
                };
                let to_type = |entries: Vec<Entry>| Type::Record(RecordType::plain(entries.into_iter().map(|e| (e.name, e.t)).collect()));
                let (ok_type, err_type) = (to_type(ok_entries), to_type(err_entries));
                let _sx = crate::types::use_style(crate::types::Style::Sx);
                let shaped = Arc::new(EnumType {
                    name: format!("{}{} | {}>", crate::values::SHAPED_PREFIX, crate::types::show_definition(&ok_type), crate::types::show_definition(&err_type)),
                    variants: vec![("Fulfilled".to_string(), Some(ok_type)), ("Rejected".to_string(), Some(err_type))],
                });
                let arm = |variant: &str, record: ir::Node| {
                    node(
                        Ir::MakeVariant {
                            enum_name: shaped.name.clone(),
                            variant: variant.to_string(),
                            payload: Some(Box::new(record)),
                        },
                        span,
                    )
                };
                let clauses = vec![
                    (MatchPattern::Ok(Box::new(MatchPattern::Any(Binder::Name(PROMOTED.to_string())))), arm("Fulfilled", ok_record)),
                    (MatchPattern::Err(Box::new(MatchPattern::Any(Binder::Name(PROMOTED.to_string())))), arm("Rejected", err_record)),
                ];
                let scrutinee = Box::new(variable(&held(*at), span));
                (Type::Enum(shaped), node(Ir::Match(ir::Match { scrutinee, clauses }), span))
            }
        };
        for (index, value) in bound.into_iter().enumerate().rev() {
            body = bind(&held(index), value, body, span);
        }
        self.coerce(body, &result_type, expected, span)
    }

    pub(super) fn shaped_enum(&self, t: &Type) -> Option<Arc<EnumType>> {
        match self.resolve(t) {
            Type::Enum(e) if e.name.starts_with(crate::values::SHAPED_PREFIX) => Some(e),
            _ => None,
        }
    }

    pub(super) fn shaped_get(&mut self, value: ir::Node, shaped: &EnumType, field: &str, span: Span, at: Span) -> Checked {
        let mut found: Option<Type> = None;
        for (_, payload) in &shaped.variants {
            if let Some(Type::Record(record)) = payload
                && let Some(t) = record.field(field)
            {
                match &found {
                    None => found = Some(t.clone()),
                    Some(first) if first != t => {
                        self.report(Diagnostic::new(
                            "type-mismatch",
                            at,
                            format!("The field {field} has a different type in each shape of this object."),
                        ));
                        return self.placeholder(span);
                    }
                    Some(_) => {}
                }
            }
        }
        let Some(result) = found else {
            self.report(Diagnostic::new("unknown-field", at, format!("No shape of this object has a field {field}.")));
            return self.placeholder(span);
        };
        let clauses = shaped
            .variants
            .iter()
            .map(|(variant, payload)| {
                let has = matches!(payload, Some(Type::Record(record)) if record.field(field).is_some());
                let body = if has {
                    node(Ir::Get { record: Box::new(variable(PROMOTED, span)), field: field.to_string() }, span)
                } else {
                    trap(&result, span)
                };
                let pattern = MatchPattern::Variant {
                    variant: variant.clone(),
                    inner: Some(Box::new(MatchPattern::Any(Binder::Name(PROMOTED.to_string())))),
                };
                (pattern, body)
            })
            .collect();
        (node(Ir::Match(ir::Match { scrutinee: Box::new(value), clauses }), span), result)
    }

    pub(super) fn expand<'a>(
        &mut self,
        span: Span,
        template: &str,
        slots: Vec<(&'static str, Slot<'a>)>,
        expected: Option<&Type>,
        scope: &Scope<'a>,
    ) -> Checked {
        let Some(mut program) = crate::reader::read(template).program else {
            self.report(Diagnostic::new("internal", span, "A js expansion did not read."));
            return self.placeholder(span);
        };
        program.respan(span);
        let mut inner = scope.clone();
        let mut wrappers: Vec<(&'static str, Option<ir::Node>)> = Vec::new();
        for (name, slot) in slots {
            match slot {
                Slot::Value(held) => {
                    let (value, t) = *held;
                    inner = inner.value(name, t);
                    wrappers.push((name, Some(value)));
                }
                Slot::Function(form) => {
                    self.check_fn_shape(form);
                    let binding = FunctionBinding { form, scope: scope.clone(), name: name.to_string() };
                    inner = inner.with(name, Binding::Function(Rc::new(binding)));
                    wrappers.push((name, None));
                }
            }
        }
        let (mut body, t) = self.check(&program, expected, &inner);
        for (name, value) in wrappers.into_iter().rev() {
            body = match value {
                Some(value) => bind(name, value, body, span),
                None => node(Ir::LocalFunction { name: name.to_string(), body: Box::new(body) }, span),
            };
        }
        (body, t)
    }

    fn receiver_error(&mut self, name: &str, at: Span, found: &Type, wanted: &str) -> Checked {
        let found = self.zonk(found);
        self.report(Diagnostic::new(
            "type-mismatch",
            at,
            format!("{name} takes {wanted} here, but this is {}.", describe(&found)),
        ));
        self.placeholder(at)
    }

    pub(super) fn js_function<'a>(
        &mut self,
        span: Span,
        name: &str,
        items: &'a [Node],
        expected: Option<&Type>,
        scope: &Scope<'a>,
    ) -> Checked {
        let args = &items[1..];
        let (low, high) = match name {
            "js.slice" => (1, 3),
            "js.length" | "js.reverse" | "js.is-array" | "js.is-integer" | "js.settled-status" | "js.settled-value"
            | "js.settled-reason" => (1, 1),
            "js.index-of" | "js.includes" | "js.join" | "js.find" | "js.parse" => (2, 2),
            "js.stringify" => (1, 2),
            _ => return self.js_registered(span, name, items, expected, scope),
        };
        if !self.arity(span, items, low, Some(high), "receiver") {
            return self.placeholder(span);
        }
        match name {
            "js.length" => {
                let (value, t) = self.firm(&args[0], scope);
                match self.resolve(&t) {
                    Type::Primitive(Primitive::String) => {
                        let made = self.js_core_call("js.length", vec![value], span);
                        self.coerce(made, &INT, expected, span)
                    }
                    Type::List(_) => self.coerce(prelude(PreludeOp::Count, vec![value], span), &INT, expected, span),
                    Type::Tuple(parts) => {
                        let made = discard(value, integer(parts.len() as i64, span), span);
                        self.coerce(made, &INT, expected, span)
                    }
                    Type::Never => self.placeholder(span),
                    other => self.receiver_error(name, args[0].span, &other, "a String or a list"),
                }
            }
            "js.is-array" => {
                let (value, t) = self.firm(&args[0], scope);
                let made = self.is_array_node(value, &t, span);
                self.coerce(made, &BOOL, expected, span)
            }
            "js.is-integer" => {
                let (value, t) = self.check(&args[0], None, scope);
                match self.resolve(&t) {
                    Type::Primitive(Primitive::Int) => self.coerce(discard(value, boolean(true, span), span), &BOOL, expected, span),
                    _ => {
                        let value = self.coerce_with(value, &t, Some(&FLOAT), args[0].span, Coercion::default()).0;
                        let made = self.js_core_call("js.is-integer", vec![value], span);
                        self.coerce(made, &BOOL, expected, span)
                    }
                }
            }
            "js.settled-status" | "js.settled-value" | "js.settled-reason" => self.settled(name, span, args, expected, scope),
            "js.stringify" => {
                let (value, t) = self.check(&args[0], None, scope);
                let (value, t) = self.settled_shape(value.clone(), &t, args[0].span).unwrap_or((value, t));
                self.late_json_values.push((t, args[0].span, "js.stringify"));
                let mut call = vec![value];
                let function = if args.len() == 2 {
                    call.push(self.check(&args[1], Some(&INT), scope).0);
                    "js.stringify-indented"
                } else {
                    "js.stringify"
                };
                let made = self.js_core_call(function, call, span);
                self.coerce(made, &STRING, expected, span)
            }
            "js.parse" => self.js_parse(span, args, expected, scope),
            "js.slice" => self.js_slice(span, args, expected, scope),
            "js.index-of" | "js.includes" => self.js_search(name, span, args, expected, scope),
            "js.join" => {
                let (separator, _) = self.check(&args[0], Some(&STRING), scope);
                let (value, t) = self.check(&args[1], None, scope);
                let element = self.list_element(&t, &args[1], "join");
                if self.is_open(&element) {
                    self.trial(&element, &STRING);
                }
                let texts = if self.resolve(&element) == STRING {
                    value
                } else {
                    let slots = vec![("js-xs", Slot::Value(Box::new((value, t))))];
                    self.expand(span, "(map (fn [x] (str x)) js-xs)", slots, None, scope).0
                };
                let made = self.js_core_call("js.join", vec![separator, texts], span);
                self.coerce(made, &STRING, expected, span)
            }
            "js.reverse" => {
                let (value, t) = self.firm(&args[0], scope);
                if !matches!(self.resolve(&t), Type::List(_)) {
                    return self.receiver_error(name, args[0].span, &t, "a list");
                }
                let slots = vec![("js-xs", Slot::Value(Box::new((value, t))))];
                let template = "(let [n (count js-xs)] (map (fn [i] (nth js-xs (- (- n 1) i))) (range 0 n)))";
                self.expand(span, template, slots, expected, scope)
            }
            _ => {
                if !is_function_form(&args[0]) {
                    self.report(Diagnostic::new("type-mismatch", args[0].span, "js.find takes a function written as (fn [x] ...)."));
                    return self.placeholder(span);
                }
                let (value, t) = self.check(&args[1], None, scope);
                let slots = vec![("js-xs", Slot::Value(Box::new((value, t)))), ("js-f", Slot::Function(&args[0]))];
                let template = "(reduce (fn [found x] (match found (some _) found none (if (truthy (js-f x)) (some x) none))) none js-xs)";
                self.expand(span, template, slots, expected, scope)
            }
        }
    }

    fn js_registered<'a>(
        &mut self,
        span: Span,
        name: &str,
        items: &'a [Node],
        expected: Option<&Type>,
        scope: &Scope<'a>,
    ) -> Checked {
        match self.registry.function(name) {
            Some(function) => self.core(span, items, function, expected, scope),
            None => self.unknown_head(span, items, scope),
        }
    }

    fn js_slice<'a>(&mut self, span: Span, args: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        let (value, t) = self.firm(&args[0], scope);
        let start = match args.get(1) {
            Some(arg) => self.check(arg, Some(&INT), scope).0,
            None => integer(0, span),
        };
        let end = args.get(2).map(|arg| self.check(arg, Some(&INT), scope).0);
        match self.resolve(&t) {
            Type::Primitive(Primitive::String) => {
                let end = end.unwrap_or_else(|| self.js_core_call("js.length", vec![variable(RECEIVER, span)], span));
                let made = self.js_core_call("js.slice", vec![variable(RECEIVER, span), start, end], span);
                self.coerce(bind(RECEIVER, value, made, span), &STRING, expected, span)
            }
            Type::List(_) => {
                let mut slots = vec![("js-xs", Slot::Value(Box::new((value, t)))), ("js-a", Slot::Value(Box::new((start, INT))))];
                let clamp = |bound: &str| {
                    format!(
                        "(if (< {bound} 0) (if (< (+ n {bound}) 0) 0 (+ n {bound})) (if (< n {bound}) n {bound}))"
                    )
                };
                let stop = match end {
                    Some(end) => {
                        slots.push(("js-b", Slot::Value(Box::new((end, INT)))));
                        clamp("js-b")
                    }
                    None => "n".to_string(),
                };
                let template = format!(
                    "(let [n (count js-xs) s {} e {stop}] (if (< s e) (map (fn [i] (nth js-xs i)) (range s e)) []))",
                    clamp("js-a")
                );
                self.expand(span, &template, slots, expected, scope)
            }
            Type::Never => self.placeholder(span),
            other => self.receiver_error("js.slice", args[0].span, &other, "a String or a list"),
        }
    }

    fn js_search<'a>(
        &mut self,
        name: &str,
        span: Span,
        args: &'a [Node],
        expected: Option<&Type>,
        scope: &Scope<'a>,
    ) -> Checked {
        let (value, t) = self.firm(&args[0], scope);
        let searching = name == "js.index-of";
        match self.resolve(&t) {
            Type::Primitive(Primitive::String) => {
                let (needle, _) = self.check(&args[1], Some(&STRING), scope);
                let found = self.js_core_call("js.index-of", vec![value, needle], span);
                if searching {
                    return self.coerce(found, &INT, expected, span);
                }
                let contained = prelude(PreludeOp::GreaterEqual, vec![found, integer(0, span)], span);
                self.coerce(contained, &BOOL, expected, span)
            }
            Type::List(_) => {
                let (needle, needle_type) = self.check(&args[1], None, scope);
                let slots = vec![("js-xs", Slot::Value(Box::new((value, t)))), ("js-x", Slot::Value(Box::new((needle, needle_type))))];
                let template = if searching {
                    "(reduce (fn [found p] (if (< found 0) (if (= (nth p 1) js-x) (nth p 0) found) found)) -1 (enumerate js-xs))"
                } else {
                    "(any? (fn [x] (= x js-x)) js-xs)"
                };
                self.expand(span, template, slots, expected, scope)
            }
            Type::Never => self.placeholder(span),
            other => self.receiver_error(name, args[0].span, &other, "a String or a list"),
        }
    }

    fn settled<'a>(
        &mut self,
        name: &str,
        span: Span,
        args: &'a [Node],
        expected: Option<&Type>,
        scope: &Scope<'a>,
    ) -> Checked {
        let (value, t) = self.check(&args[0], None, scope);
        self.settled_read(name, span, (value, t), args[0].span, expected)
    }

    pub(super) fn settled_read(
        &mut self,
        name: &str,
        span: Span,
        (value, t): Checked,
        at: Span,
        expected: Option<&Type>,
    ) -> Checked {
        let Type::Result(ok, err) = self.resolve(&t) else {
            if matches!(self.resolve(&t), Type::Never) {
                return self.placeholder(span);
            }
            return self.receiver_error(name, at, &t, "the result of Promise.allSettled");
        };
        let ok = self.resolve(&ok);
        let clauses = |checker: &mut Checker<'_>, fulfilled: ir::Node, rejected: ir::Node| {
            let _ = checker;
            vec![
                (MatchPattern::Ok(Box::new(MatchPattern::Any(Binder::Name(PROMOTED.to_string())))), fulfilled),
                (MatchPattern::Err(Box::new(MatchPattern::Any(Binder::Name(PROMOTED.to_string())))), rejected),
            ]
        };
        let (arms, result) = match name {
            "js.settled-status" => (clauses(self, text("fulfilled", span), text("rejected", span)), STRING),
            "js.settled-value" => (clauses(self, variable(PROMOTED, span), trap(&ok, span)), ok),
            _ => {
                let wrapped = option((*err).clone());
                let rejected = some_of(variable(PROMOTED, span), &wrapped, span);
                (clauses(self, literal(Value::None, wrapped.clone(), span), rejected), wrapped)
            }
        };
        let made = node(Ir::Match(ir::Match { scrutinee: Box::new(value), clauses: arms }), span);
        self.coerce(made, &result, expected, span)
    }

    fn js_parse<'a>(&mut self, span: Span, args: &'a [Node], expected: Option<&Type>, scope: &Scope<'a>) -> Checked {
        let NodeKind::String(type_text) = &args[0].kind else {
            self.report(Diagnostic::new(
                "ts-json-parse-type",
                args[0].span,
                "JSON.parse needs the type of the document",
            ));
            return self.placeholder(span);
        };
        let named: BTreeMap<String, Type> = self.registry.types().map(|(n, t)| (n.to_string(), t.clone())).collect();
        let target = match crate::types::parse_type(type_text, &named) {
            Ok(t) => t,
            Err(error) => {
                self.report(Diagnostic::new("ts-json-parse-type", args[0].span, error.to_string()));
                return self.placeholder(span);
            }
        };
        if let Some(problem) = encodable_problem(&target) {
            self.report(Diagnostic::new("ts-json-parse-type", args[0].span, problem));
            return self.placeholder(span);
        }
        let (document, _) = self.check(&args[1], Some(&STRING), scope);
        let made = self.js_core_call("js.parse", vec![text(type_text, args[0].span), document], span);
        self.coerce(made, &target, expected, span)
    }
}
