use sx_kernel::syntax::{Node, NodeKind, Span};

use crate::ast::*;
use crate::build::*;
use crate::diag;
use crate::exprs::*;
use crate::lower::*;

const MUTATING_METHODS: [&str; 6] = ["pop", "shift", "unshift", "splice", "fill", "copyWithin"];
const REGEXP_METHODS: [&str; 5] = ["match", "matchAll", "test", "exec", "search"];
const LOCALE_METHODS: [&str; 5] = ["toLocaleString", "toLocaleDateString", "toLocaleTimeString", "localeCompare", "toLocaleUpperCase"];
const DATE_METHODS: [&str; 8] = ["toISOString", "getTime", "getFullYear", "getMonth", "getDate", "getDay", "getHours", "toJSON"];

impl<'a> Lowerer<'a> {
    pub fn call_expr(&mut self, e: &Expr, callee: &Expr, args: &[Expr], awaited: bool, promise_ok: bool) -> Node {
        let span = e.span;
        match &callee.kind {
            ExprKind::Ident(name) => self.ident_call(name, callee, args, span, awaited, promise_ok),
            ExprKind::Field(receiver, method) => {
                if let ExprKind::Field(tools, namespace) = &receiver.kind
                    && matches!(&tools.kind, ExprKind::Ident(t) if t == "tools") && !self.is_bound("tools")
                {
                    return self.tool_call(namespace, method, args, span, awaited, promise_ok);
                }
                if let ExprKind::Ident(object) = &receiver.kind
                    && !self.is_bound(object)
                    && !matches!(object.as_str(), "tools")
                    && self.is_global_object(object)
                {
                    return self.global_call(object, method, args, span, awaited, promise_ok);
                }
                self.method_call(receiver, method, args, span)
            }
            _ => self.reject("ts-function-value", span, "Only named functions, tools and builtins can be called"),
        }
    }

    fn is_global_object(&self, name: &str) -> bool {
        matches!(
            name,
            "Math" | "JSON" | "Object" | "Promise" | "console" | "Number" | "Array" | "Date" | "Intl" | "String" | "Symbol" | "Set" | "Map"
                | "BigInt" | "RegExp" | "Reflect" | "Proxy" | "process" | "window" | "globalThis" | "Buffer"
        )
    }

    fn tool_call(&mut self, namespace: &str, name: &str, args: &[Expr], span: Span, awaited: bool, promise_ok: bool) -> Node {
        let key = format!("{namespace}.{name}");
        let Some(id) = self.tools.get(&key).cloned() else {
            let mut d = diag("unknown-tool", span, format!("No tool {key}"));
            if let Some(near) = self.closest_tools(&key) {
                d = d.fix(format!("tools.{near}"));
            }
            self.report(d);
            return unit(span);
        };
        if !awaited && !promise_ok {
            return self.reject_fix("ts-promise-value", span, "Await calls where they are made", "await tools.…");
        }
        let input = match args {
            [] => record(Vec::new(), span),
            [Expr { kind: ExprKind::Object(items), span: object_span }] => self.object_literal(items, *object_span, true),
            [other] => self.expr_t(other, false, false),
            _ => return self.reject("ts-unsupported", span, "A tool takes one input object"),
        };
        app("call", vec![sym(&id, span), input], span)
    }

    fn ident_call(&mut self, name: &str, callee: &Expr, args: &[Expr], span: Span, awaited: bool, promise_ok: bool) -> Node {
        if let Some(var) = self.lookup(name).cloned() {
            return match var.function {
                Some(is_async) => {
                    if is_async && !awaited && !promise_ok {
                        return self.reject_fix("ts-promise-value", span, "Await calls where they are made", "await the call");
                    }
                    let mut nodes = Vec::new();
                    for arg in args {
                        if matches!(arg.kind, ExprKind::Arrow(_)) {
                            nodes.push(self.reject("ts-function-value", arg.span, "A function may only be passed to a builtin"));
                        } else {
                            nodes.push(self.expr_t(arg, false, false));
                        }
                    }
                    app(&var.sx, nodes, span)
                }
                None => self.reject("ts-function-value", span, "Only declared functions, tools and builtins can be called"),
            };
        }
        match name {
            "parseInt" => match args {
                [text, Expr { kind: ExprKind::Number(Number::Int(10)), .. }] => {
                    let v = self.expr_t(text, false, false);
                    app("js.parse-int", vec![v], span)
                }
                _ => self.reject_fix("ts-unknown-builtin", span, "parseInt needs the radix 10", "parseInt(s, 10)"),
            },
            "String" => match args {
                [value] => {
                    let v = self.stringified_value(value);
                    if head(&v) == Some("str") { v } else { app("str", vec![v], span) }
                }
                _ => self.reject("ts-unknown-builtin", span, "String takes one value"),
            },
            "retry" | "waitUntil" | "parMap" | "loop" | "sleep" => self.bounded(name, args, span, awaited),
            "next" | "done" => self.reject("ts-unsupported", span, "next and done are only valid as return next(…) or return done(…) in a loop function"),
            other if UNKNOWN_FUNCTIONS.contains(&other) => {
                self.reject_fix("ts-unknown-builtin", span, format!("No builtin {other}"), "use arithmetic and comparisons")
            }
            other => {
                let before = self.diagnostics.len();
                self.ident_value(other, callee.span);
                if self.diagnostics.len() > before {
                    return unit(span);
                }
                self.reject("ts-unknown-builtin", span, format!("No builtin {other}"))
            }
        }
    }

    fn stringified_value(&mut self, e: &Expr) -> Node {
        let template = Expr { kind: ExprKind::Template(vec![String::new(), String::new()], vec![e.clone()]), span: e.span };
        let node = self.expr_t(&template, false, false);
        if head(&node) == Some("str") {
            if let NodeKind::Form(items) = node.kind {
                let mut parts = items.into_vec();
                if parts.len() == 2 {
                    return parts.remove(1);
                }
                return form(parts, e.span);
            }
            return unit(e.span);
        }
        node
    }

    fn global_call(&mut self, object: &str, method: &str, args: &[Expr], span: Span, awaited: bool, promise_ok: bool) -> Node {
        match (object, method) {
            ("Math", "floor" | "ceil") if args.len() == 1 => {
                let v = self.expr_t(&args[0], false, false);
                app(method, vec![v], span)
            }
            ("Math", "round" | "trunc" | "abs") if args.len() == 1 => {
                let v = self.expr_t(&args[0], false, false);
                let x = self.fresh("x");
                let xs = || sym(&x, span);
                let body = match method {
                    "trunc" => app(
                        "if",
                        vec![app("<", vec![xs(), float(0.0, span)], span), app("ceil", vec![xs()], span), app("floor", vec![xs()], span)],
                        span,
                    ),
                    "abs" => app("if", vec![app("<", vec![xs(), int(0, span)], span), app("-", vec![xs()], span), xs()], span),
                    _ => {
                        let f = self.fresh("f");
                        let fraction = app("-", vec![xs(), app("float", vec![sym(&f, span)], span)], span);
                        let rounded = app(
                            "if",
                            vec![
                                app(">=", vec![fraction, float(0.5, span)], span),
                                app("+", vec![sym(&f, span), int(1, span)], span),
                                sym(&f, span),
                            ],
                            span,
                        );
                        return let_(
                            vec![(xs(), v), (sym(&f, span), app("floor", vec![xs()], span))],
                            rounded,
                            span,
                        );
                    }
                };
                let_(vec![(xs(), v)], body, span)
            }
            ("Math", "max" | "min") if args.len() == 2 => {
                let a = self.expr_t(&args[0], false, false);
                let b = self.expr_t(&args[1], false, false);
                let (x, y) = (self.fresh("a"), self.fresh("b"));
                let compare = app(if method == "max" { ">" } else { "<" }, vec![sym(&x, span), sym(&y, span)], span);
                let pick = app("if", vec![compare, sym(&x, span), sym(&y, span)], span);
                let_(vec![(sym(&x, span), a), (sym(&y, span), b)], pick, span)
            }
            ("Math", "random") => self.reject_fix("ts-nondeterministic", span, "A program must be deterministic", "sleep"),
            ("Math", _) => self.reject("ts-unknown-builtin", span, format!("No builtin Math.{method}")),
            ("JSON", "stringify") => match args {
                [value] => {
                    let v = self.expr_t(value, false, false);
                    app("js.stringify", vec![v], span)
                }
                [value, Expr { kind: ExprKind::Null, .. }, Expr { kind: ExprKind::Number(Number::Int(n)), .. }] => {
                    let v = self.expr_t(value, false, false);
                    app("js.stringify", vec![v, int(*n, span)], span)
                }
                _ => self.reject("ts-json", span, "JSON.stringify takes one value, or (value, null, number)"),
            },
            ("JSON", "parse") => self.reject_fix("ts-json-parse-type", span, "JSON.parse needs the type of the document", "JSON.parse(t) as { … }"),
            ("Object", "fromEntries") if args.len() == 1 => {
                let v = self.expr_t(&args[0], false, false);
                app("into-map", vec![v], span)
            }
            ("Object", "assign") => self.reject_fix("ts-mutating-method", span, "Object.assign changes an object in place", "build a new object"),
            ("Object", "keys" | "values" | "entries") => self.reject_fix(
                "ts-map-order",
                span,
                "The order of a Map's keys is not preserved",
                "Object.keys(m).sort()",
            ),
            ("Object", _) => self.reject("ts-unknown-builtin", span, format!("No builtin Object.{method}")),
            ("Promise", "all") => self.promise_all(args, false, span, awaited, promise_ok),
            ("Promise", "allSettled") => self.promise_all(args, true, span, awaited, promise_ok),
            ("Promise", _) => self.reject("ts-unknown-builtin", span, format!("No builtin Promise.{method}")),
            ("console", "log") => {
                if self.parallel > 0 {
                    return self.reject("ts-log-in-parallel", span, "console.log is not supported in a parallel branch");
                }
                let nodes: Vec<Node> = args.iter().map(|a| self.expr_t(a, false, false)).collect();
                app("log", nodes, span)
            }
            ("console", _) => self.reject_fix("ts-console", span, format!("console.{method} is not supported"), "console.log"),
            ("Number", "isInteger") if args.len() == 1 => {
                let v = self.expr_t(&args[0], false, false);
                app("js.is-integer", vec![v], span)
            }
            ("Array", "isArray") if args.len() == 1 => {
                let v = self.expr_t(&args[0], false, false);
                app("js.is-array", vec![v], span)
            }
            ("Date", "now") => self.reject_fix("ts-nondeterministic", span, "A program must be deterministic", "sleep"),
            ("Date", _) => self.reject_fix("ts-date", span, "Date is not supported", "pass the timestamp as text"),
            ("Intl", _) => self.reject("ts-locale", span, "Output depends on the locale"),
            ("RegExp", _) => self.reject_fix("ts-regexp", span, "Regular expressions are not supported", "includes, startsWith, split"),
            ("Set" | "Map" | "Symbol" | "BigInt" | "Buffer" | "Proxy" | "Reflect", _) => {
                self.reject_fix("ts-unsupported-class", span, format!("{object} is not supported"), "use arrays and records")
            }
            (other, _) => self.reject("ts-unknown-builtin", span, format!("No builtin {other}.{method}")),
        }
    }

    fn promise_all(&mut self, args: &[Expr], settled: bool, span: Span, awaited: bool, promise_ok: bool) -> Node {
        if !awaited && !promise_ok {
            return self.reject_fix("ts-promise-value", span, "Await calls where they are made", "await Promise.all(…)");
        }
        let [arg] = args else {
            return self.reject("ts-promise-value", span, "Promise.all takes one array");
        };
        self.parallel += 1;
        let result = match &arg.kind {
            ExprKind::Array(items) => {
                let mut branches: Vec<Node> = Vec::new();
                for item in items {
                    let ArrayItem::Item(item) = item else {
                        branches.push(self.reject_fix("ts-spread", item_span(item), "Only records with declared fields can be spread into an object", "write the branches out"));
                        continue;
                    };
                    if !matches!(item.kind, ExprKind::Call(..)) {
                        branches.push(self.reject_fix("ts-promise-value", item.span, "Await calls where they are made", "pass the calls directly"));
                        continue;
                    }
                    let node = self.expr_t(item, false, true);
                    branches.push(if settled { app("attempt", vec![node], item.span) } else { node });
                }
                app("par", branches, span)
            }
            ExprKind::Call(callee, call_args) => match (&callee.kind, call_args.as_slice()) {
                (ExprKind::Field(source, method), [f]) if method == "map" => {
                    let items = self.expr_t(source, false, false);
                    let element_hint = if self.hint_of(source) == Hint::SettledList { Hint::Settled } else { Hint::Unknown };
                    self.param_hint = Some(element_hint);
                    let (function, _) = self.callback(f, false, true);
                    let function = if settled { self.wrap_attempt(function, span) } else { function };
                    app("par-map", vec![record(vec![("limit".to_string(), int(16, span))], span), function, items], span)
                }
                _ => self.reject("ts-promise-value", span, "Promise.all takes an array literal or xs.map(f)"),
            },
            _ => self.reject("ts-promise-value", span, "Promise.all takes an array literal or xs.map(f)"),
        };
        self.parallel -= 1;
        result
    }

    fn wrap_attempt(&mut self, function: Node, span: Span) -> Node {
        if head(&function) == Some("fn")
            && let NodeKind::Form(items) = function.kind
        {
            let mut parts = items.into_vec();
            if let Some(body) = parts.pop() {
                parts.push(app("attempt", vec![body], span));
            }
            return form(parts, span);
        }
        let x = self.fresh("x");
        let call = app("attempt", vec![form(vec![function, sym(&x, span)], span)], span);
        form(vec![sym("fn", span), list(vec![sym(&x, span)], span), call], span)
    }

    pub fn callback(&mut self, f: &Expr, index_ok: bool, promise_ok: bool) -> (Node, bool) {
        self.callback_with(f, if index_ok { 2 } else { 1 }, index_ok, promise_ok)
    }

    pub fn callback_with(&mut self, f: &Expr, max_params: usize, index_ok: bool, promise_ok: bool) -> (Node, bool) {
        match &f.kind {
            ExprKind::Arrow(function) => {
                let n = function.params.len();
                if n > max_params {
                    self.param_hint = None;
                    let node = self.reject("ts-callback-arity", f.span, "The builtin supplies only the element and, where the index is allowed, its index");
                    return (node, false);
                }
                let indexed = n == 2 && index_ok;
                let (params, body) = self.function_parts(function, promise_ok, indexed);
                (form(vec![sym("fn", f.span), params, body], f.span), indexed)
            }
            ExprKind::Ident(name) => {
                self.param_hint = None;
                match self.lookup(name).cloned() {
                    Some(var) if var.function.is_some() => (sym(&var.sx, f.span), false),
                    None => (self.reject("ts-unknown-builtin", f.span, format!("No builtin {name} as a callback")), false),
                    _ => (self.reject("ts-function-value", f.span, "A function may only be an arrow or the name of a declared function"), false),
                }
            }
            _ => {
                self.param_hint = None;
                (self.reject("ts-function-value", f.span, "A function may only be an arrow or the name of a declared function"), false)
            }
        }
    }

    pub fn method_call(&mut self, receiver: &Expr, method: &str, args: &[Expr], span: Span) -> Node {
        if let ExprKind::Call(inner_callee, inner_args) = &receiver.kind
            && let Some(target) = object_keys_target(inner_callee, inner_args)
            && method == "sort"
            && args.is_empty()
        {
            let m = self.expr_t(target, false, false);
            return app("sort-by", vec![sym("identity", span), app("keys", vec![m], span)], span);
        }
        if MUTATING_METHODS.contains(&method) {
            return self.reject_fix("ts-mutating-method", span, format!("{method} changes an array in place"), "build a new array");
        }
        if REGEXP_METHODS.contains(&method) {
            return self.reject_fix("ts-regexp", span, "Regular expressions are not supported", "includes, startsWith, split");
        }
        if LOCALE_METHODS.contains(&method) {
            return self.reject("ts-locale", span, "Output depends on the locale");
        }
        if DATE_METHODS.contains(&method) {
            return self.reject("ts-date", span, "Date is not supported");
        }
        if method == "push" {
            return self.reject("ts-mutation-pattern", span, "push is only supported as a statement on an array this function owns");
        }
        let element_hint = if self.hint_of(receiver) == Hint::SettledList { Hint::Settled } else { Hint::Unknown };
        let mut text = self.hint_of(receiver);
        if let ExprKind::Ident(name) = &receiver.kind
            && let Some(var) = self.lookup(name)
            && let Some(kind) = var.catch
        {
            let _ = kind;
            text = Hint::Unknown;
        }
        let r = self.expr_t(receiver, false, false);
        let arg = |l: &mut Self, index: usize| -> Option<Node> { args.get(index).map(|a| l.expr_t(a, false, false)) };
        match method {
            "map" | "filter" | "flatMap" | "find" | "some" | "every" | "forEach" => {
                let [f] = args else {
                    return self.reject("ts-callback-arity", span, format!("{method} takes one function"));
                };
                self.param_hint = Some(element_hint);
                let (function, indexed) = self.callback(f, true, false);
                let source = if indexed { app("enumerate", vec![r], span) } else { r };
                match method {
                    "map" => app("map", vec![function, source], span),
                    "flatMap" => app("flatten", vec![app("map", vec![function, source], span)], span),
                    "some" => app("any?", vec![function, source], span),
                    "every" => app("all?", vec![function, source], span),
                    "forEach" => let_(vec![(sym("_", span), app("map", vec![function, source], span))], unit(span), span),
                    "filter" if indexed => {
                        let p = self.fresh("p");
                        let kept = app("filter", vec![function, source], span);
                        let pick = form(vec![sym("fn", span), list(vec![sym(&p, span)], span), app("nth", vec![sym(&p, span), int(1, span)], span)], span);
                        app("map", vec![pick, kept], span)
                    }
                    "filter" => app("filter", vec![function, source], span),
                    _ if indexed => {
                        let p = self.fresh("p");
                        let found = app("js.find", vec![function, source], span);
                        form(
                            vec![
                                sym("match", span),
                                found,
                                app("some", vec![sym(&p, span)], span),
                                some(app("nth", vec![sym(&p, span), int(1, span)], span), span),
                                none(span),
                                none(span),
                            ],
                            span,
                        )
                    }
                    _ => app("js.find", vec![function, source], span),
                }
            }
            "reduce" => {
                let [f, init] = args else {
                    return self.reject_fix("ts-reduce-init", span, "reduce needs an initial value", "xs.reduce(f, init)");
                };
                self.param_hint = None;
                let (function, _) = self.callback_with(f, 2, false, false);
                let initial = self.expr_t(init, false, false);
                app("reduce", vec![function, initial, r], span)
            }
            "includes" => {
                let Some(x) = arg(self, 0) else { return self.reject("ts-unknown-builtin", span, "includes takes one value") };
                match text {
                    Hint::Text => app("text.contains?", vec![r, x], span),
                    Hint::List => app("contains?", vec![r, x], span),
                    _ => app("js.includes", vec![r, x], span),
                }
            }
            "indexOf" => {
                let Some(x) = arg(self, 0) else { return self.reject("ts-unknown-builtin", span, "indexOf takes one value") };
                app("js.index-of", vec![r, x], span)
            }
            "join" => {
                let Some(sep) = arg(self, 0) else {
                    return self.reject_fix("ts-unknown-builtin", span, "join needs a separator", "xs.join(\",\")");
                };
                app("js.join", vec![sep, r], span)
            }
            "slice" => {
                let mut nodes = vec![r];
                nodes.extend(args.iter().map(|a| self.expr_t(a, false, false)));
                app("js.slice", nodes, span)
            }
            "concat" => {
                let Some(x) = arg(self, 0) else { return self.reject("ts-unknown-builtin", span, "concat takes one array") };
                app("concat", vec![r, x], span)
            }
            "sort" => self.sort_node_pub(r, args.first(), span),
            "reverse" => app("js.reverse", vec![r], span),
            "startsWith" | "endsWith" => {
                let Some(x) = arg(self, 0) else { return self.reject("ts-unknown-builtin", span, format!("{method} takes one string")) };
                app(if method == "startsWith" { "text.starts-with?" } else { "text.ends-with?" }, vec![r, x], span)
            }
            "split" => {
                let Some(sep) = arg(self, 0) else { return self.reject("ts-unknown-builtin", span, "split takes a separator") };
                app("text.split", vec![r, sep], span)
            }
            "trim" => app("js.trim", vec![r], span),
            "toLowerCase" => app("js.to-lower", vec![r], span),
            "toUpperCase" => app("js.to-upper", vec![r], span),
            "padStart" | "padEnd" => {
                let (Some(n), Some(s)) = (arg(self, 0), arg(self, 1)) else {
                    return self.reject("ts-unknown-builtin", span, format!("{method} needs a length and a fill string"));
                };
                app(if method == "padStart" { "js.pad-start" } else { "js.pad-end" }, vec![r, n, s], span)
            }
            "replace" | "replaceAll" => {
                let (Some(a), Some(b)) = (arg(self, 0), arg(self, 1)) else {
                    return self.reject("ts-unknown-builtin", span, format!("{method} takes two strings"));
                };
                app(if method == "replace" { "js.replace-first" } else { "text.replace" }, vec![r, a, b], span)
            }
            "toString" if args.is_empty() => app("str", vec![r], span),
            _ => {
                let shown = self.source_text(receiver.span);
                let names: Vec<&str> = known_methods();
                let mut d = diag("ts-unknown-builtin", span, format!("No builtin {method} on {shown}"));
                if let Some(near) = sx_kernel::checker::closest(method, &names) {
                    d = d.fix(near);
                }
                self.report(d);
                unit(span)
            }
        }
    }

    pub fn sort_node_pub(&mut self, receiver: Node, comparator: Option<&Expr>, span: Span) -> Node {
        let args: Vec<Expr> = comparator.cloned().into_iter().collect();
        self.reorder(receiver, "sort", &args, span)
    }

    fn duration_ms(&mut self, e: &Expr) -> Option<i64> {
        match &e.kind {
            ExprKind::Number(Number::Int(n)) if *n >= 0 => Some(*n),
            _ => {
                self.report(diag("ts-bound-not-literal", e.span, "The bound must be a literal").fix("write the milliseconds as an integer literal"));
                None
            }
        }
    }

    fn options(&mut self, e: &Expr) -> Vec<(String, Node)> {
        let ExprKind::Object(items) = &e.kind else {
            self.report(diag("ts-bound-not-literal", e.span, "The bound must be a literal").fix("pass an object literal"));
            return Vec::new();
        };
        let mut found: Vec<(&'static str, Node)> = Vec::new();
        for item in items {
            let ObjectItem::Field { key, value, key_span } = item else {
                self.report(diag("ts-bound-not-literal", e.span, "The bound must be a literal"));
                continue;
            };
            let span = value.span;
            let duration_list = |l: &mut Self, value: &Expr| -> Option<Vec<Node>> {
                let ExprKind::Array(items) = &value.kind else {
                    l.report(diag("ts-bound-not-literal", value.span, "The bound must be a literal"));
                    return None;
                };
                let mut out = Vec::new();
                for item in items {
                    let ArrayItem::Item(item) = item else { return None };
                    out.push(duration(l.duration_ms(item)?, item.span));
                }
                Some(out)
            };
            let entry: Option<(&'static str, Node)> = match key.as_str() {
                "delaysMs" => duration_list(self, value).map(|d| ("delays", list(d, span))),
                "backoffMs" => duration_list(self, value).map(|d| ("backoff", list(d, span))),
                "withinMs" => self.duration_ms(value).map(|ms| ("within", duration(ms, span))),
                "everyMs" => self.duration_ms(value).map(|ms| ("every", duration(ms, span))),
                "limit" => self.duration_ms(value).map(|n| ("limit", int(n, span))),
                "max" => self.duration_ms(value).map(|n| ("max", int(n, span))),
                "on" => {
                    let ExprKind::Array(kinds) = &value.kind else {
                        self.report(diag("ts-bound-not-literal", span, "The bound must be a literal"));
                        continue;
                    };
                    let mut names = Vec::new();
                    for kind in kinds {
                        match kind {
                            ArrayItem::Item(Expr { kind: ExprKind::Str(text), span }) if is_kind_name(text) => names.push(sym(text, *span)),
                            _ => self.report(diag("ts-bound-not-literal", span, "The retried kinds must be literal CallError kind names")),
                        }
                    }
                    Some(("on", list(names, span)))
                }
                other => {
                    self.report(diag("ts-bound-not-literal", *key_span, format!("{other} is not an option of this builtin")));
                    None
                }
            };
            found.extend(entry);
        }
        let order = ["delays", "backoff", "within", "every", "on", "limit", "max"];
        let mut ordered: Vec<(String, Node)> = Vec::new();
        for name in order {
            if let Some(position) = found.iter().position(|(n, _)| *n == name) {
                ordered.push((name.to_string(), found.remove(position).1));
            }
        }
        ordered
    }

    fn bounded(&mut self, name: &str, args: &[Expr], span: Span, awaited: bool) -> Node {
        if !awaited {
            return self.reject_fix("ts-promise-value", span, "Await calls where they are made", "await the call");
        }
        match (name, args) {
            ("sleep", [ms]) => match self.duration_ms(ms) {
                Some(ms) => app("sleep", vec![duration(ms, span)], span),
                None => unit(span),
            },
            ("retry", [opts, f]) => {
                let options = self.options(opts);
                let body = self.thunk_body(f);
                app("retry", vec![record(options, opts.span), body], span)
            }
            ("waitUntil", [opts, poll, done]) => {
                let options = self.options(opts);
                let body = self.thunk_body(poll);
                let (predicate, _) = self.callback(done, false, false);
                app("wait-until", vec![record(options, opts.span), predicate, body], span)
            }
            ("parMap", [opts, items, f]) => {
                let options = self.options(opts);
                let source = self.expr_t(items, false, false);
                self.parallel += 1;
                let (function, _) = self.callback(f, false, true);
                self.parallel -= 1;
                app("par-map", vec![record(options, opts.span), function, source], span)
            }
            ("loop", [opts, state, f]) => self.loop_builtin(opts, state, f, span),
            _ => self.reject("ts-unsupported", span, format!("{name} is called with the wrong arguments")),
        }
    }

    fn thunk_body(&mut self, f: &Expr) -> Node {
        let ExprKind::Arrow(function) = &f.kind else {
            return self.reject("ts-function-value", f.span, "retry and waitUntil take an arrow function");
        };
        if !function.params.is_empty() {
            return self.reject("ts-callback-arity", f.span, "This function takes no parameters");
        }
        let (_, body) = self.function_parts(function, true, false);
        body
    }

    fn loop_builtin(&mut self, opts: &Expr, state: &Expr, f: &Expr, span: Span) -> Node {
        let options = self.options(opts);
        let ExprKind::Object(items) = &state.kind else {
            return self.reject("ts-bound-not-literal", state.span, "The loop state must be an object literal");
        };
        let ExprKind::Arrow(function) = &f.kind else {
            return self.reject("ts-function-value", f.span, "loop takes an arrow function");
        };
        let keys: Vec<String> = items
            .iter()
            .filter_map(|item| match item {
                ObjectItem::Field { key, .. } => Some(key.clone()),
                ObjectItem::Spread(_) => None,
            })
            .collect();
        let mut initial: Vec<Node> = Vec::new();
        let mut values: Vec<Node> = Vec::new();
        for item in items {
            if let ObjectItem::Field { value, .. } = item {
                values.push(self.expr_t(value, false, false));
            }
        }
        let [Pattern { kind: PatternKind::Object(props), span: pattern_span }] = function.params.as_slice() else {
            return self.reject("ts-callback-arity", f.span, "The loop function takes the state as one object pattern");
        };
        self.fn_depth += 1;
        self.frames.push(Frame::LoopBuiltin(keys.clone()));
        self.scopes.push(Scope::new());
        let saved_mutated = std::mem::take(&mut self.mutated);
        self.mutated = vec![match &function.body {
            FunctionBody::Block(body) => all_mutated(body),
            FunctionBody::Expr(_) => Default::default(),
        }];
        let mut names: Vec<(String, Node)> = Vec::new();
        for key in &keys {
            let local = props.iter().find(|p| &p.key == key).and_then(|p| match &p.value.kind {
                PatternKind::Ident(local) => Some(local.clone()),
                _ => None,
            });
            let local = local.unwrap_or_else(|| key.clone());
            let sx = self.declare(&local, self.plain_var());
            names.push((sx.clone(), sym(&sx, *pattern_span)));
        }
        let mut bindings: Vec<Node> = Vec::new();
        for ((sx, _), value) in names.iter().zip(values) {
            bindings.push(sym(sx, span));
            initial.push(value);
        }
        let body = match &function.body {
            FunctionBody::Expr(e) => self.loop_step(e, &keys),
            FunctionBody::Block(stmts) => {
                let (_, node) = self.body(stmts, &|l: &mut Lowerer<'a>| l.reject("ts-unsupported", l.end_span(), "A loop function must return next(…) or done(…)"), false);
                node
            }
        };
        self.mutated = saved_mutated;
        self.scopes.pop();
        self.frames.pop();
        self.fn_depth -= 1;
        let mut state_list: Vec<Node> = Vec::new();
        for (binding, value) in bindings.into_iter().zip(initial) {
            state_list.push(binding);
            state_list.push(value);
        }
        app("loop", vec![record(options, opts.span), list(state_list, state.span), body], span)
    }
}

fn item_span(item: &ArrayItem) -> Span {
    match item {
        ArrayItem::Item(e) | ArrayItem::Spread(e) => e.span,
    }
}

fn known_methods() -> Vec<&'static str> {
    vec![
        "map", "filter", "flatMap", "find", "some", "every", "forEach", "reduce", "includes", "indexOf", "join", "slice", "concat", "sort",
        "reverse", "startsWith", "endsWith", "split", "trim", "toLowerCase", "toUpperCase", "padStart", "padEnd", "replace", "replaceAll",
        "toString",
    ]
}
