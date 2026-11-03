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

pub(super) enum Slot<'a> {
    Value(ir::Node, Type),
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
                self.late_json_values.push((t.clone(), span, "js.stringify"));
                self.js_core_call("js.stringify", vec![value], span)
            }
        }
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
        enum Source {
            Whole,
            Field(String),
        }
        struct Entry {
            name: String,
            t: Type,
            item: usize,
            source: Source,
        }
        let hint = match self.hint(expected) {
            Type::Record(record) => Some(record),
            _ => None,
        };
        let mut bound: Vec<ir::Node> = Vec::new();
        let mut entries: Vec<Entry> = Vec::new();
        for (index, item) in items[1..].iter().enumerate() {
            let parts = form_items(item).unwrap_or(&[]);
            let kind = parts.first().and_then(keyword);
            match (kind, parts.len()) {
                (Some("spread"), 2) => {
                    let (value, t) = self.check(&parts[1], None, scope);
                    bound.push(value);
                    match self.resolve(&t) {
                        Type::Record(record) if !record.open => {
                            for (name, field_type) in &record.fields {
                                let entry = Entry {
                                    name: name.clone(),
                                    t: field_type.clone(),
                                    item: index,
                                    source: Source::Field(name.clone()),
                                };
                                match entries.iter_mut().find(|e| e.name == *name) {
                                    Some(existing) => *existing = entry,
                                    None => entries.push(entry),
                                }
                            }
                        }
                        Type::Never => {}
                        other => {
                            let found = describe(&self.zonk(&other));
                            self.report(
                                Diagnostic::new(
                                    "ts-spread",
                                    parts[1].span,
                                    "Only records with declared fields can be spread into an object",
                                )
                                .detail(format!("This is {found}."))
                                .fix("write the fields"),
                            );
                        }
                    }
                }
                (Some("field"), 3) => {
                    let NodeKind::String(name) = &parts[1].kind else {
                        self.report(Diagnostic::new("bad-literal", parts[1].span, "A field of record-spread names it with a string."));
                        bound.push(literal(Value::Unit, UNIT, span));
                        continue;
                    };
                    let wanted = hint.as_ref().and_then(|record| record.field(name)).cloned();
                    let (value, t) = self.check(&parts[2], wanted.as_ref(), scope);
                    bound.push(value);
                    let entry = Entry { name: name.clone(), t, item: index, source: Source::Whole };
                    match entries.iter_mut().find(|e| e.name == *name) {
                        Some(existing) => *existing = entry,
                        None => entries.push(entry),
                    }
                }
                _ => {
                    self.report(Diagnostic::new(
                        "bad-literal",
                        item.span,
                        "record-spread takes (:spread value) and (:field \"name\" value) items.",
                    ));
                    bound.push(literal(Value::Unit, UNIT, span));
                }
            }
        }
        let held = |index: usize| format!("{SPREAD}{index}");
        let fields: Vec<(String, ir::Node)> = entries
            .iter()
            .map(|entry| {
                let source = variable(&held(entry.item), span);
                let part = match &entry.source {
                    Source::Whole => source,
                    Source::Field(field) => node(Ir::Get { record: Box::new(source), field: field.clone() }, span),
                };
                (entry.name.clone(), part)
            })
            .collect();
        let mut body = node(Ir::MakeRecord { fields }, span);
        for (index, value) in bound.into_iter().enumerate().rev() {
            body = bind(&held(index), value, body, span);
        }
        let record = Type::Record(RecordType::plain(entries.into_iter().map(|e| (e.name, e.t)).collect()));
        self.coerce(body, &record, expected, span)
    }

    pub(super) fn expand<'a>(
        &mut self,
        span: Span,
        template: &str,
        slots: Vec<(&'static str, Slot<'a>)>,
        expected: Option<&Type>,
        scope: &Scope<'a>,
    ) -> Checked {
        let Some(program) = crate::reader::read(template).program else {
            self.report(Diagnostic::new("internal", span, "A js expansion did not read."));
            return self.placeholder(span);
        };
        let mut inner = scope.clone();
        let mut wrappers: Vec<(&'static str, Option<ir::Node>)> = Vec::new();
        for (name, slot) in slots {
            match slot {
                Slot::Value(value, t) => {
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
                    let slots = vec![("js-xs", Slot::Value(value, t))];
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
                let slots = vec![("js-xs", Slot::Value(value, t))];
                let template = "(let [n (count js-xs)] (map (fn [i] (nth js-xs (- (- n 1) i))) (range 0 n)))";
                self.expand(span, template, slots, expected, scope)
            }
            _ => {
                if !is_function_form(&args[0]) {
                    self.report(Diagnostic::new("type-mismatch", args[0].span, "js.find takes a function written as (fn [x] ...)."));
                    return self.placeholder(span);
                }
                let (value, t) = self.check(&args[1], None, scope);
                let slots = vec![("js-xs", Slot::Value(value, t)), ("js-f", Slot::Function(&args[0]))];
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
                let mut slots = vec![("js-xs", Slot::Value(value, t)), ("js-a", Slot::Value(start, INT))];
                let clamp = |bound: &str| {
                    format!(
                        "(if (< {bound} 0) (if (< (+ n {bound}) 0) 0 (+ n {bound})) (if (< n {bound}) n {bound}))"
                    )
                };
                let stop = match end {
                    Some(end) => {
                        slots.push(("js-b", Slot::Value(end, INT)));
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
                let slots = vec![("js-xs", Slot::Value(value, t)), ("js-x", Slot::Value(needle, needle_type))];
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
        let Type::Result(ok, err) = self.resolve(&t) else {
            if matches!(self.resolve(&t), Type::Never) {
                return self.placeholder(span);
            }
            return self.receiver_error(name, args[0].span, &t, "the result of Promise.allSettled");
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
