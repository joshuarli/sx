use sx_kernel::syntax::{Node, NodeKind, Span};

use crate::analysis::*;
use crate::ast::*;
use crate::build::*;
use crate::diag;
use crate::lower::*;
use crate::stmts::{is_json_parse, is_tuple_source};

const GLOBAL_OBJECTS: [&str; 7] = ["Math", "JSON", "Object", "Promise", "console", "Number", "Array"];
const CLASS_GLOBALS: [&str; 12] = [
    "Set", "Map", "WeakMap", "WeakSet", "Symbol", "BigInt", "Uint8Array", "Uint16Array", "Float64Array", "ArrayBuffer", "Buffer", "Proxy",
];
pub const UNKNOWN_FUNCTIONS: [&str; 7] = ["Number", "parseFloat", "isNaN", "isFinite", "Boolean", "encodeURIComponent", "decodeURIComponent"];
const KNOWN_METHODS: [&str; 33] = [
    "map", "filter", "flatMap", "find", "some", "every", "forEach", "reduce", "includes", "indexOf", "join", "slice", "concat",
    "sort", "reverse", "push", "startsWith", "endsWith", "split", "trim", "toLowerCase", "toUpperCase", "padStart", "padEnd",
    "replace", "replaceAll", "toString", "length", "findIndex", "at", "charAt", "substring", "lastIndexOf",
];

fn kind_name(name: &str) -> bool {
    matches!(name, "Timeout" | "Unavailable" | "RateLimited" | "NotFound" | "PermissionDenied" | "Invalid" | "Conflict" | "Failed")
}

impl<'a> Lowerer<'a> {
    pub fn value_of(&mut self, e: &Expr, returning: bool) -> Node {
        let promise_ok = returning && self.promise_ok;
        self.expr_t(e, false, promise_ok)
    }

    pub fn statement_call(&mut self, e: &Expr) -> Node {
        self.expr_t(e, false, false)
    }

    pub fn hint_of(&self, e: &Expr) -> Hint {
        match &e.kind {
            ExprKind::Array(_) => Hint::List,
            ExprKind::Str(_) | ExprKind::Template(..) => Hint::Text,
            ExprKind::Ident(name) => self.lookup(name).map_or(Hint::Unknown, |v| v.hint),
            ExprKind::Index(base, _) if self.hint_of(base) == Hint::SettledList => Hint::Settled,
            ExprKind::Await(inner) => {
                if let ExprKind::Call(callee, _) = &inner.kind
                    && matches!(&callee.kind, ExprKind::Field(r, m) if m == "allSettled" && matches!(&r.kind, ExprKind::Ident(o) if o == "Promise"))
                {
                    return Hint::SettledList;
                }
                self.hint_of(inner)
            }
            ExprKind::Call(callee, _) => match &callee.kind {
                ExprKind::Field(receiver, method) => match method.as_str() {
                    "map" | "filter" | "flatMap" | "split" | "sort" | "reverse" => Hint::List,
                    "join" | "trim" | "toLowerCase" | "toUpperCase" | "padStart" | "padEnd" | "replace" | "replaceAll" | "toString" => Hint::Text,
                    "fromEntries" if matches!(&receiver.kind, ExprKind::Ident(o) if o == "Object") => Hint::Map,
                    _ => Hint::Unknown,
                },
                ExprKind::Ident(name) if name == "String" => Hint::Text,
                _ => Hint::Unknown,
            },
            _ => Hint::Unknown,
        }
    }

    fn is_stringy(&self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Str(_) | ExprKind::Template(..) => true,
            ExprKind::Binary(BinaryOp::Add, l, r) => self.is_stringy(l) || self.is_stringy(r),
            _ => self.hint_of(e) == Hint::Text,
        }
    }

    pub fn is_bool_expr(&self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Bool(_) => true,
            ExprKind::Unary(UnaryOp::Not, _) => true,
            ExprKind::Binary(op, l, r) => match op {
                BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge | BinaryOp::StrictEq | BinaryOp::StrictNe | BinaryOp::LooseEqNull | BinaryOp::LooseNeNull => true,
                BinaryOp::And | BinaryOp::Or => self.is_bool_expr(l) && self.is_bool_expr(r),
                _ => false,
            },
            ExprKind::Call(callee, _) => matches!(&callee.kind, ExprKind::Field(_, m) if matches!(m.as_str(), "includes" | "startsWith" | "endsWith" | "some" | "every" | "isArray" | "isInteger")),
            _ => false,
        }
    }

    pub fn condition(&mut self, e: &Expr) -> Node {
        let span = e.span;
        match &e.kind {
            ExprKind::Bool(b) => boolean(*b, span),
            ExprKind::Ident(name) if self.lookup(name).is_some_and(|v| v.catch.is_some()) => boolean(true, span),
            ExprKind::Unary(UnaryOp::Not, inner) => {
                let c = self.condition(inner);
                app("not", vec![c], span)
            }
            ExprKind::Binary(BinaryOp::And, l, r) => {
                let a = self.condition(l);
                let b = self.condition(r);
                app("if", vec![a, b, boolean(false, span)], span)
            }
            ExprKind::Binary(BinaryOp::Or, l, r) => {
                let a = self.condition(l);
                let b = self.condition(r);
                app("if", vec![a, boolean(true, span), b], span)
            }
            ExprKind::Binary(BinaryOp::LooseEqNull, x, _) => {
                let v = self.expr_t(x, true, false);
                app("not", vec![app("present", vec![v], span)], span)
            }
            ExprKind::Binary(BinaryOp::LooseNeNull, x, _) => {
                let v = self.expr_t(x, true, false);
                app("present", vec![v], span)
            }
            ExprKind::Binary(op @ (BinaryOp::StrictEq | BinaryOp::StrictNe), l, r) if matches!(r.kind, ExprKind::Null) || matches!(l.kind, ExprKind::Null) => {
                let operand = if matches!(r.kind, ExprKind::Null) { l } else { r };
                let v = self.expr_t(operand, false, false);
                app(if *op == BinaryOp::StrictEq { "=" } else { "not=" }, vec![v, none(span)], span)
            }
            _ if self.is_bool_expr(e) => self.expr_t(e, true, false),
            _ => {
                let v = self.expr_t(e, true, false);
                app("truthy", vec![v], span)
            }
        }
    }

    pub fn expr_t(&mut self, e: &Expr, tolerant: bool, promise_ok: bool) -> Node {
        let span = e.span;
        match &e.kind {
            ExprKind::Number(Number::Int(n)) => int(*n, span),
            ExprKind::Number(Number::Float(f)) => float(*f, span),
            ExprKind::Str(s) => string(s, span),
            ExprKind::Bool(b) => boolean(*b, span),
            ExprKind::Null => none(span),
            ExprKind::Template(quasis, parts) => self.template(quasis, parts, span),
            ExprKind::Ident(name) => self.ident_value(name, span),
            ExprKind::Array(items) => self.array_literal(items, span),
            ExprKind::Object(items) => self.object_literal(items, span, false),
            ExprKind::Field(base, name) => self.field_access(base, name, span),
            ExprKind::Index(base, index) => self.index_access(base, index, span),
            ExprKind::Chain(head, links) => {
                if !tolerant {
                    self.report(diag("ts-optional-chain", span, "The result of ?. is undefined, not null").fix("x == null ? null : x.f"));
                }
                self.chain(head, links, span)
            }
            ExprKind::Call(callee, args) => self.call_expr(e, callee, args, false, promise_ok),
            ExprKind::Await(inner) => match &inner.kind {
                ExprKind::Call(callee, args) => self.call_expr(inner, callee, args, true, promise_ok),
                _ => {
                    self.expr_t(inner, false, false);
                    self.reject_fix("ts-promise-value", span, "Await calls where they are made", "await a call directly")
                }
            },
            ExprKind::Arrow(_) => self.reject("ts-function-value", span, "A function may only be passed to a builtin or called by name"),
            ExprKind::Unary(op, inner) => self.unary(*op, inner, span),
            ExprKind::Binary(op, l, r) => self.binary(*op, l, r, span, tolerant),
            ExprKind::Conditional(c, a, b) => {
                let cond = self.condition(c);
                let then = self.expr_t(a, false, promise_ok);
                let otherwise = self.expr_t(b, false, promise_ok);
                app("if", vec![cond, then, otherwise], span)
            }
            ExprKind::Assign(..) | ExprKind::Update { .. } => self.reject(
                "ts-mutation-pattern",
                span,
                "An assignment is only supported as a statement in a mutation pattern, not inside a larger expression",
            ),
            ExprKind::Cast(inner, annotation) => {
                if is_json_parse(inner) {
                    self.typed_parse(inner, annotation)
                } else {
                    self.reject("ts-cast", span, "A cast cannot be checked")
                }
            }
            ExprKind::New(name, _) => self.new_value(name, span),
            ExprKind::Rejected => unit(span),
        }
    }

    fn new_value(&mut self, name: &str, span: Span) -> Node {
        match name {
            "Error" => self.reject("ts-throw", span, "new Error(text) may only be thrown"),
            "Date" => self.reject_fix("ts-date", span, "Date is not supported", "pass the timestamp as text"),
            "RegExp" => self.reject_fix("ts-regexp", span, "Regular expressions are not supported", "includes, startsWith, split"),
            "Function" => self.reject("ts-eval", span, "Code cannot be built at run time"),
            "Array" | "Promise" => self.reject("ts-unsupported-class", span, format!("{name} is not supported")),
            other if CLASS_GLOBALS.contains(&other) => {
                self.reject_fix("ts-unsupported-class", span, format!("{other} is not supported"), "use arrays and records")
            }
            _ => self.reject_fix("ts-class", span, "Classes are not supported", "use object literals"),
        }
    }

    pub fn ident_value(&mut self, name: &str, span: Span) -> Node {
        if let Some(var) = self.lookup(name).cloned() {
            if var.function.is_some() {
                return self.reject("ts-function-value", span, "A function may only be passed to a builtin or called by name");
            }
            if var.catch == Some(CatchKind::Mixed { json_parse: false }) || matches!(var.catch, Some(CatchKind::Mixed { .. })) {
                return self.reject_fix(
                    "ts-error-use",
                    span,
                    "This catch may also see a plain Error, so only e.message is supported",
                    "e.message",
                );
            }
            if var.uninit {
                return self.reject_fix("ts-uninitialized", span, "This name may be read before it is assigned", "give it a value");
            }
            return sym(&var.sx, span);
        }
        match name {
            "searchTools" | "describeTool" | "describeNamespace" | "ALL_TOOLS" => {
                self.reject_fix("ts-tools-escape", span, "tools can only be called as tools.namespace.name(…)", "tools.namespace.name(input)")
            }
            "tools" => self.reject_fix("ts-tools-escape", span, "tools can only be called as tools.namespace.name(…)", "tools.namespace.name(input)"),
            "undefined" => self.reject_fix("ts-undefined", span, "undefined does not exist; absent values are null", "== null"),
            "this" => self.reject_fix("ts-class", span, "Classes are not supported", "use object literals"),
            "NaN" | "Infinity" => self.reject("ts-unknown-builtin", span, format!("No builtin {name}")),
            "eval" | "Function" => self.reject("ts-eval", span, "Code cannot be built at run time"),
            "Date" => self.reject_fix("ts-date", span, "Date is not supported", "pass the timestamp as text"),
            "RegExp" => self.reject_fix("ts-regexp", span, "Regular expressions are not supported", "includes, startsWith, split"),
            "Intl" => self.reject("ts-locale", span, "Output depends on the locale"),
            "require" | "module" | "exports" => self.reject_fix("ts-import-export", span, "Programs have no modules", "use the tools directly"),
            "setTimeout" | "setInterval" | "setImmediate" => {
                self.reject_fix("ts-nondeterministic", span, "A program must be deterministic", "sleep")
            }
            other if CLASS_GLOBALS.contains(&other) => {
                self.reject_fix("ts-unsupported-class", span, format!("{other} is not supported"), "use arrays and records")
            }
            other if GLOBAL_OBJECTS.contains(&other) || matches!(other, "String" | "parseInt" | "Error" | "Promise") => {
                self.reject("ts-unknown-builtin", span, format!("No builtin {other} as a value"))
            }
            other => sym(other, span),
        }
    }

    fn template(&mut self, quasis: &[String], parts: &[Expr], span: Span) -> Node {
        let mut items: Vec<Node> = Vec::new();
        for (index, quasi) in quasis.iter().enumerate() {
            if !quasi.is_empty() {
                items.push(string(quasi, span));
            }
            if let Some(part) = parts.get(index) {
                items.push(self.stringified(part));
            }
        }
        match items.len() {
            0 => string("", span),
            1 if matches!(items[0].kind, NodeKind::String(_)) => items.remove(0),
            _ => app("str", items, span),
        }
    }

    fn stringified(&mut self, e: &Expr) -> Node {
        if let ExprKind::Ident(name) = &e.kind
            && let Some(var) = self.lookup(name).cloned()
            && let Some(kind) = var.catch
        {
            return match kind {
                CatchKind::ToolOnly => {
                    let message = app("get", vec![sym(&var.sx, e.span), kw("message", e.span)], e.span);
                    app("str", vec![string("CallError: ", e.span), message], e.span)
                }
                CatchKind::Mixed { .. } => self.reject_fix(
                    "ts-error-use",
                    e.span,
                    "This catch may also see a plain Error, so only e.message is supported",
                    "e.message",
                ),
            };
        }
        self.expr_t(e, false, false)
    }

    fn array_literal(&mut self, items: &[ArrayItem], span: Span) -> Node {
        let mut groups: Vec<Node> = Vec::new();
        let mut plain: Vec<Node> = Vec::new();
        let mut has_spread = false;
        for item in items {
            match item {
                ArrayItem::Item(e) => plain.push(self.expr_t(e, false, false)),
                ArrayItem::Spread(e) => {
                    has_spread = true;
                    if !plain.is_empty() {
                        groups.push(list(std::mem::take(&mut plain), span));
                    }
                    groups.push(self.expr_t(e, false, false));
                }
            }
        }
        if !has_spread {
            return list(plain, span);
        }
        if !plain.is_empty() {
            groups.push(list(plain, span));
        }
        let mut iter = groups.into_iter();
        let Some(mut joined) = iter.next() else {
            return list(Vec::new(), span);
        };
        for next in iter {
            joined = app("concat", vec![joined, next], span);
        }
        joined
    }

    pub fn object_literal(&mut self, items: &[ObjectItem], span: Span, tool_input: bool) -> Node {
        let has_spread = items.iter().any(|i| matches!(i, ObjectItem::Spread(_)));
        if !has_spread {
            let mut fields: Vec<(String, Node)> = Vec::new();
            for item in items {
                if let ObjectItem::Field { key, value, .. } = item {
                    if tool_input && matches!(value.kind, ExprKind::Null) {
                        continue;
                    }
                    let node = self.expr_t(value, tool_input, false);
                    fields.push((key.clone(), node));
                }
            }
            return record(fields, span);
        }
        let mut parts: Vec<Node> = Vec::new();
        for item in items {
            match item {
                ObjectItem::Spread(operand) => {
                    if !is_name_or_member(operand) {
                        let d = diag("ts-spread", operand.span, "Only records with declared fields can be spread into an object").fix("write the fields");
                        self.report(d);
                        continue;
                    }
                    let node = self.expr_t(operand, false, false);
                    parts.push(form(vec![kw("spread", operand.span), node], operand.span));
                }
                ObjectItem::Field { key, value, key_span } => {
                    let node = self.expr_t(value, false, false);
                    parts.push(form(vec![kw("field", *key_span), string(key, *key_span), node], *key_span));
                }
            }
        }
        app("record-spread", parts, span)
    }

    fn field_access(&mut self, base: &Expr, name: &str, span: Span) -> Node {
        if let ExprKind::Ident(object) = &base.kind {
            if object == "tools" && !self.is_bound("tools") {
                return self.reject_fix("ts-tools-escape", span, "tools can only be called as tools.namespace.name(…)", "tools.namespace.name(input)");
            }
            if let Some(var) = self.lookup(object).cloned()
                && let Some(kind) = var.catch
            {
                return match (name, kind) {
                    ("message", CatchKind::Mixed { json_parse: true }) => self.reject_fix(
                        "ts-error-use",
                        span,
                        "A JSON.parse failure has an engine-specific message, so e.message cannot be reproduced",
                        "validate the text before parsing",
                    ),
                    ("message", _) => app("get", vec![sym(&var.sx, base.span), kw("message", span)], span),
                    ("kind", CatchKind::ToolOnly) => app("get", vec![sym(&var.sx, base.span), kw("kind", span)], span),
                    _ => self.reject_fix("ts-error-use", span, format!("e.{name} is not supported in this catch"), "e.message"),
                };
            }
            if !self.is_bound(object) && (GLOBAL_OBJECTS.contains(&object.as_str()) || object == "String") {
                return self.reject("ts-unknown-builtin", span, format!("No builtin {object}.{name} as a value"));
            }
        }
        if let ExprKind::Call(callee, args) = &base.kind
            && name == "length"
            && let Some(map) = object_keys_target(callee, args)
        {
            let m = self.expr_t(map, false, false);
            return app("count", vec![m], span);
        }
        let receiver = self.expr_t(base, false, false);
        if name == "length" {
            return app("js.length", vec![receiver], span);
        }
        if self.hint_of(base) == Hint::Settled {
            match name {
                "status" => return app("js.settled-status", vec![receiver], span),
                "value" => return app("js.settled-value", vec![receiver], span),
                "reason" => return app("js.settled-reason", vec![receiver], span),
                _ => {}
            }
        }
        app("get", vec![receiver, kw(name, span)], span)
    }

    fn index_access(&mut self, base: &Expr, index: &Expr, span: Span) -> Node {
        let receiver = self.expr_t(base, false, false);
        if let ExprKind::Str(key) = &index.kind {
            return app("get", vec![receiver, kw(key, index.span)], span);
        }
        let key = self.expr_t(index, false, false);
        app("js.index", vec![receiver, key], span)
    }

    fn unary(&mut self, op: UnaryOp, inner: &Expr, span: Span) -> Node {
        match op {
            UnaryOp::Not => {
                let c = self.condition(inner);
                app("not", vec![c], span)
            }
            UnaryOp::Neg => match &inner.kind {
                ExprKind::Number(Number::Int(n)) => int(-n, span),
                ExprKind::Number(Number::Float(f)) => float(-f, span),
                _ => {
                    let v = self.expr_t(inner, false, false);
                    app("-", vec![v], span)
                }
            },
            UnaryOp::Plus => self.expr_t(inner, false, false),
            UnaryOp::TypeOf => {
                if matches!(inner.kind, ExprKind::Chain(..)) {
                    return self.reject("ts-optional-chain", span, "typeof of an optional chain is not supported");
                }
                let v = self.expr_t(inner, false, false);
                app("typeof", vec![v], span)
            }
        }
    }

    pub fn arithmetic(&mut self, op: AssignOp, a: Node, b: Node, span: Span) -> Node {
        let stringy = |n: &Node| matches!(n.kind, NodeKind::String(_)) || head(n) == Some("str");
        match op {
            AssignOp::Add if stringy(&a) || stringy(&b) => self.concat_str(a, b, span),
            AssignOp::Add => app("+", vec![a, b], span),
            AssignOp::Sub => app("-", vec![a, b], span),
            AssignOp::Mul => app("*", vec![a, b], span),
            AssignOp::Div => app("/", vec![a, b], span),
            AssignOp::Rem => app("rem", vec![a, b], span),
            AssignOp::Set => b,
        }
    }

    fn concat_str(&mut self, a: Node, b: Node, span: Span) -> Node {
        let mut items: Vec<Node> = Vec::new();
        if head(&a) == Some("str")
            && let NodeKind::Form(parts) = a.kind
        {
            items.extend(parts.into_vec().into_iter().skip(1));
        } else {
            items.push(a);
        }
        items.push(b);
        app("str", items, span)
    }

    fn binary(&mut self, op: BinaryOp, l: &Expr, r: &Expr, span: Span, tolerant: bool) -> Node {
        let _ = tolerant;
        match op {
            BinaryOp::Add => {
                let stringy = self.is_stringy(l) || self.is_stringy(r);
                let a = if stringy { self.stringified(l) } else { self.expr_t(l, false, false) };
                let b = if stringy { self.stringified(r) } else { self.expr_t(r, false, false) };
                if stringy { self.concat_str(a, b, span) } else { app("+", vec![a, b], span) }
            }
            BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                let a = self.expr_t(l, false, false);
                let b = self.expr_t(r, false, false);
                let head = match op {
                    BinaryOp::Sub => "-",
                    BinaryOp::Mul => "*",
                    BinaryOp::Div => "/",
                    BinaryOp::Rem => "rem",
                    BinaryOp::Lt => "<",
                    BinaryOp::Le => "<=",
                    BinaryOp::Gt => ">",
                    _ => ">=",
                };
                app(head, vec![a, b], span)
            }
            BinaryOp::StrictEq | BinaryOp::StrictNe => {
                let negate = op == BinaryOp::StrictNe;
                let undefined_text = |e: &Expr| matches!(&e.kind, ExprKind::Str(s) if s == "undefined");
                let typeof_expr = |e: &Expr| matches!(e.kind, ExprKind::Unary(UnaryOp::TypeOf, _));
                if (undefined_text(r) && typeof_expr(l)) || (undefined_text(l) && typeof_expr(r)) {
                    return self.reject_fix("ts-undefined", span, "undefined does not exist; absent values are null", "== null");
                }
                if matches!(&r.kind, ExprKind::Ident(n) if n == "undefined" && !self.is_bound("undefined"))
                    || matches!(&l.kind, ExprKind::Ident(n) if n == "undefined" && !self.is_bound("undefined"))
                {
                    return self.reject_fix("ts-undefined", span, "undefined does not exist; absent values are null", "== null");
                }
                if matches!(r.kind, ExprKind::Null) || matches!(l.kind, ExprKind::Null) {
                    let operand = if matches!(r.kind, ExprKind::Null) { l } else { r };
                    let v = self.expr_t(operand, false, false);
                    return app(if negate { "not=" } else { "=" }, vec![v, none(span)], span);
                }
                let literal = |e: &Expr| matches!(e.kind, ExprKind::Str(_) | ExprKind::Number(_) | ExprKind::Bool(_));
                let a = self.expr_t(l, literal(r), false);
                let b = self.expr_t(r, literal(l), false);
                app(if negate { "not=" } else { "=" }, vec![a, b], span)
            }
            BinaryOp::LooseEqNull | BinaryOp::LooseNeNull => {
                let v = self.expr_t(l, true, false);
                let present = app("present", vec![v], span);
                if op == BinaryOp::LooseNeNull { present } else { app("not", vec![present], span) }
            }
            BinaryOp::And | BinaryOp::Or if self.is_bool_expr(l) => {
                let test = self.condition(l);
                let b = self.expr_t(r, false, false);
                if op == BinaryOp::And {
                    app("if", vec![test, b, boolean(false, span)], span)
                } else {
                    app("if", vec![test, boolean(true, span), b], span)
                }
            }
            BinaryOp::And | BinaryOp::Or => {
                let a = self.expr_t(l, op == BinaryOp::Or, false);
                let b = self.expr_t(r, false, false);
                let t = self.fresh("t");
                let test = app("truthy", vec![sym(&t, span)], span);
                let chosen = if op == BinaryOp::And {
                    app("if", vec![test, b, sym(&t, span)], span)
                } else {
                    app("if", vec![test, sym(&t, span), b], span)
                };
                let_(vec![(sym(&t, span), a)], chosen, span)
            }
            BinaryOp::Coalesce => {
                if let ExprKind::Field(base, name) = &l.kind {
                    let receiver = self.expr_t(base, false, false);
                    let default = self.expr_t(r, false, false);
                    return app("get", vec![receiver, kw(name, l.span), default], span);
                }
                let x = self.expr_t(l, true, false);
                let default = self.expr_t(r, false, false);
                let v = self.fresh("v");
                form(
                    vec![sym("match", span), x, app("some", vec![sym(&v, span)], span), sym(&v, span), none(span), default],
                    span,
                )
            }
        }
    }

    fn apply_links(&self, base: Expr, links: &[Link]) -> Expr {
        let mut current = base;
        for link in links {
            let span = sx_kernel::syntax::Span::new(current.span.start, match link {
                Link::Access { span, .. } | Link::Call { span, .. } => span.end,
            });
            current = match link {
                Link::Access { access: Access::Field(name), .. } => Expr { kind: ExprKind::Field(Box::new(current), name.clone()), span },
                Link::Access { access: Access::Index(index), .. } => Expr { kind: ExprKind::Index(Box::new(current), index.clone()), span },
                Link::Call { args, .. } => Expr { kind: ExprKind::Call(Box::new(current), args.clone()), span },
            };
        }
        current
    }

    fn chain(&mut self, head: &Expr, links: &[Link], span: Span) -> Node {
        let optional = |link: &Link| match link {
            Link::Access { optional, .. } | Link::Call { optional, .. } => *optional,
        };
        let Some(first) = links.iter().position(optional) else {
            let applied = self.apply_links(head.clone(), links);
            return self.expr_t(&applied, false, false);
        };
        let base = self.apply_links(head.clone(), &links[..first]);
        let base_node = self.expr_t(&base, false, false);
        let temp_js = format!("?{}", self.counter + 1);
        let temp_sx = self.fresh("v");
        let mut template = self.plain_var();
        template.sx = temp_sx.clone();
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(temp_js.clone(), template);
        }
        let mut remaining: Vec<Link> = links[first..].to_vec();
        match &mut remaining[0] {
            Link::Access { optional, .. } | Link::Call { optional, .. } => *optional = false,
        }
        let inner_head = Expr { kind: ExprKind::Ident(temp_js), span: head.span };
        let inner = if remaining.iter().any(optional) {
            self.chain(&inner_head, &remaining, span)
        } else {
            let applied = self.apply_links(inner_head, &remaining);
            self.expr_t(&applied, false, false)
        };
        form(vec![sym("match", span), base_node, app("some", vec![sym(&temp_sx, span)], span), inner, none(span), none(span)], span)
    }

    pub fn type_text(&self, annotation: &TypeAnn, depth: usize) -> Option<String> {
        if depth > 20 {
            return None;
        }
        Some(match annotation {
            TypeAnn::Primitive("string") | TypeAnn::Literal(_) => "String".to_string(),
            TypeAnn::Primitive("number" | "Float") => "Float".to_string(),
            TypeAnn::Primitive("Int") => "Int".to_string(),
            TypeAnn::Primitive("boolean") => "Bool".to_string(),
            TypeAnn::Primitive(_) => return None,
            TypeAnn::List(inner) => format!("List<{}>", self.type_text(inner, depth + 1)?),
            TypeAnn::Tuple(items) => {
                let parts: Option<Vec<String>> = items.iter().map(|t| self.type_text(t, depth + 1)).collect();
                format!("Tuple<{}>", parts?.join(", "))
            }
            TypeAnn::Map(value) => format!("Map<String, {}>", self.type_text(value, depth + 1)?),
            TypeAnn::Object(fields) => {
                let mut parts = Vec::new();
                for (name, optional, t) in fields {
                    let text = self.type_text(t, depth + 1)?;
                    parts.push(if *optional { format!("{name}: Option<{text}>") } else { format!("{name}: {text}") });
                }
                format!("Record {{{}}}", parts.join(", "))
            }
            TypeAnn::Union(members) => {
                let non_null: Vec<&TypeAnn> = members.iter().filter(|m| **m != TypeAnn::Primitive("null")).collect();
                if non_null.len() == members.len() {
                    if members.iter().all(|m| matches!(m, TypeAnn::Literal(_))) {
                        return Some("String".to_string());
                    }
                    return None;
                }
                match non_null.as_slice() {
                    [only] => format!("Option<{}>", self.type_text(only, depth + 1)?),
                    _ => return None,
                }
            }
            TypeAnn::Name(name) => self.type_text(self.types.get(name)?, depth + 1)?,
            TypeAnn::Promise(_) | TypeAnn::Unknown | TypeAnn::Any | TypeAnn::Void => return None,
        })
    }

    pub fn typed_parse(&mut self, call: &Expr, annotation: &TypeAnn) -> Node {
        let span = call.span;
        let ExprKind::Call(_, args) = &call.kind else {
            return unit(span);
        };
        let Some(text) = self.type_text(annotation, 0) else {
            return self.reject_fix(
                "ts-json-parse-type",
                span,
                "JSON.parse needs the type of the document",
                "JSON.parse(t) as { … }",
            );
        };
        if args.len() != 1 {
            return self.reject("ts-json", span, "JSON.parse takes the text only");
        }
        let value = self.expr_t(&args[0], false, false);
        app("js.parse", vec![string(&text, span), value], span)
    }

    pub fn reorder(&mut self, receiver: Node, method: &str, args: &[Expr], span: Span) -> Node {
        if method == "reverse" {
            return app("js.reverse", vec![receiver], span);
        }
        self.sort_node(receiver, args.first(), span)
    }

    fn sort_node(&mut self, receiver: Node, comparator: Option<&Expr>, span: Span) -> Node {
        let Some(comparator) = comparator else {
            return app("sort-by", vec![sym("identity", span), receiver], span);
        };
        let accepted = (|| -> Option<(Function, bool)> {
            let ExprKind::Arrow(function) = &comparator.kind else { return None };
            Some((function.as_ref().clone(), true))
        })();
        let Some((function, _)) = accepted else {
            return self.reject_fix("ts-sort-comparator", comparator.span, "A sort comparator must be (a, b) => k(a) - k(b) or k(b) - k(a)", "(a, b) => k(a) - k(b)");
        };
        let invalid = |l: &mut Self| {
            l.reject_fix("ts-sort-comparator", comparator.span, "A sort comparator must be (a, b) => k(a) - k(b) or k(b) - k(a)", "(a, b) => k(a) - k(b)")
        };
        let [pa, pb] = function.params.as_slice() else {
            return invalid(self);
        };
        let (PatternKind::Ident(a), PatternKind::Ident(b)) = (&pa.kind, &pb.kind) else {
            return invalid(self);
        };
        let body = match &function.body {
            FunctionBody::Expr(e) => e.as_ref(),
            FunctionBody::Block(stmts) => match stmts.as_slice() {
                [Stmt { kind: StmtKind::Return(Some(e)), .. }] => e,
                _ => return invalid(self),
            },
        };
        let ExprKind::Binary(BinaryOp::Sub, left, right) = &body.kind else {
            return invalid(self);
        };
        let (ascending, key_a, key_b) = if expr_mentions(left, a) && !expr_mentions(left, b) && expr_mentions(right, b) && !expr_mentions(right, a) {
            (true, left, right)
        } else if expr_mentions(left, b) && !expr_mentions(left, a) && expr_mentions(right, a) && !expr_mentions(right, b) {
            (false, right, left)
        } else {
            return invalid(self);
        };
        self.scopes.push(Scope::new());
        let sx_a = self.declare(a, self.plain_var());
        let mut alias = self.plain_var();
        alias.sx = sx_a.clone();
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(b.clone(), alias);
        }
        let node_a = self.expr_t(key_a, false, false);
        let node_b = self.expr_t(key_b, false, false);
        self.scopes.pop();
        if sx_kernel::syntax::show(&node_a) != sx_kernel::syntax::show(&node_b) {
            return invalid(self);
        }
        let key = if ascending { node_a } else { app("-", vec![node_a], span) };
        let key_fn = form(vec![sym("fn", span), list(vec![sym(&sx_a, span)], span), key], span);
        app("sort-by", vec![key_fn, receiver], span)
    }

    pub fn loop_step(&mut self, e: &Expr, keys: &[String]) -> Node {
        let span = e.span;
        let ExprKind::Call(callee, args) = &e.kind else {
            return self.reject("ts-unsupported", span, "A loop function must return next({…}) or done(value)");
        };
        match (&callee.kind, args.as_slice()) {
            (ExprKind::Ident(name), [value]) if name == "done" => self.expr_t(value, false, false),
            (ExprKind::Ident(name), [Expr { kind: ExprKind::Object(items), .. }]) if name == "next" => {
                let mut values: Vec<Node> = Vec::new();
                for key in keys {
                    let found = items.iter().find_map(|item| match item {
                        ObjectItem::Field { key: k, value, .. } if k == key => Some(value),
                        _ => None,
                    });
                    match found {
                        Some(value) => values.push(self.expr_t(value, false, false)),
                        None => {
                            return self.reject("ts-unsupported", span, format!("next must name every loop variable, including {key}"));
                        }
                    }
                }
                app("recur", values, span)
            }
            _ => self.reject("ts-unsupported", span, "A loop function must return next({…}) or done(value)"),
        }
    }
}

fn is_name_or_member(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Ident(_) => true,
        ExprKind::Field(base, _) => is_name_or_member(base),
        ExprKind::Index(base, _) => is_name_or_member(base),
        _ => false,
    }
}

pub fn object_keys_target<'e>(callee: &'e Expr, args: &'e [Expr]) -> Option<&'e Expr> {
    match (&callee.kind, args) {
        (ExprKind::Field(object, method), [target]) if method == "keys" && matches!(&object.kind, ExprKind::Ident(o) if o == "Object") => Some(target),
        _ => None,
    }
}

pub fn known_method(name: &str) -> bool {
    KNOWN_METHODS.contains(&name)
}

pub fn is_kind_name(name: &str) -> bool {
    kind_name(name)
}

pub fn tuple_source(e: &Expr) -> bool {
    is_tuple_source(e)
}
