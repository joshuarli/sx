use std::collections::HashSet;

use sx_kernel::syntax::{Node, Span};

use crate::analysis::*;
use crate::ast::*;
use crate::build::*;
use crate::diag;
use crate::lower::*;

struct FnItem<'s> {
    index: usize,
    name: String,
    function: &'s Function,
    name_span: Span,
}

fn fn_items(list: &[Stmt]) -> Vec<FnItem<'_>> {
    let mut items = Vec::new();
    for (index, stmt) in list.iter().enumerate() {
        match &stmt.kind {
            StmtKind::Function(function) => {
                if let Some(name) = &function.name {
                    items.push(FnItem { index, name: name.name.clone(), function, name_span: name.span });
                }
            }
            StmtKind::Decl(DeclKind::Const, decls) if decls.len() == 1 => {
                let decl = &decls[0];
                if let (PatternKind::Ident(name), Some(Expr { kind: ExprKind::Arrow(function), .. })) = (&decl.pattern.kind, &decl.init) {
                    items.push(FnItem { index, name: name.clone(), function, name_span: decl.pattern.span });
                }
            }
            _ => {}
        }
    }
    items
}

fn declared_names(function: &Function) -> HashSet<String> {
    let mut names = Vec::new();
    function.params.iter().for_each(|p| pattern_names(p, &mut names));
    let mut declare_stmts = |stmts: &[Stmt]| {
        fn visit(stmt: &Stmt, names: &mut Vec<String>) {
            match &stmt.kind {
                StmtKind::Decl(_, decls) => decls.iter().for_each(|d| pattern_names(&d.pattern, names)),
                StmtKind::If(_, a, b) => {
                    visit(a, names);
                    if let Some(b) = b {
                        visit(b, names);
                    }
                }
                StmtKind::ForOf { pattern, body, .. } => {
                    pattern_names(pattern, names);
                    visit(body, names);
                }
                StmtKind::For { init, body, .. } => {
                    if let Some(init) = init {
                        visit(init, names);
                    }
                    visit(body, names);
                }
                StmtKind::Try { block, param, handler } => {
                    if let Some(param) = param {
                        pattern_names(param, names);
                    }
                    block.iter().chain(handler).for_each(|s| visit(s, names));
                }
                StmtKind::Block(body) => body.iter().for_each(|s| visit(s, names)),
                StmtKind::Function(f) => {
                    if let Some(n) = &f.name {
                        names.push(n.name.clone());
                    }
                }
                _ => {}
            }
        }
        stmts.iter().for_each(|s| visit(s, &mut names));
    };
    declare_stmts(function_body_stmts(function));
    names.into_iter().collect()
}

fn free_names(function: &Function) -> HashSet<String> {
    let declared = declared_names(function);
    let mut found = HashSet::new();
    let mut visit = |e: &Expr| {
        if let ExprKind::Ident(name) = &e.kind {
            found.insert(name.clone());
        }
        if let ExprKind::Arrow(inner) = &e.kind {
            for p in &inner.params {
                let mut names = Vec::new();
                pattern_names(p, &mut names);
                let _ = names;
            }
        }
    };
    match &function.body {
        FunctionBody::Expr(e) => walk_expr(e, &mut visit),
        FunctionBody::Block(b) => walk_stmts(b, &mut visit),
    }
    found.retain(|n| !declared.contains(n));
    found
}

impl<'a> Lowerer<'a> {
    pub fn compute_throwing(&mut self, body: &[Stmt]) {
        let items = fn_items(body);
        let mut throwing: HashSet<String> = HashSet::new();
        loop {
            let before = throwing.len();
            for item in &items {
                if throwing.contains(&item.name) {
                    continue;
                }
                if can_throw_plain(function_body_stmts(item.function), &throwing) {
                    throwing.insert(item.name.clone());
                }
            }
            if throwing.len() == before {
                break;
            }
        }
        self.throwing = throwing;
    }

    pub fn body(&mut self, list: &[Stmt], k: Cont<'_, 'a>, top: bool) -> (Vec<Node>, Node) {
        let items = fn_items(list);
        let item_names: HashSet<&str> = items.iter().map(|i| i.name.as_str()).collect();
        let local_decls: HashSet<String> = {
            let mut names = Vec::new();
            for (index, stmt) in list.iter().enumerate() {
                if let StmtKind::Decl(_, decls) = &stmt.kind
                    && !items.iter().any(|i| i.index == index)
                {
                    decls.iter().for_each(|d| pattern_names(&d.pattern, &mut names));
                }
            }
            names.into_iter().collect()
        };
        let mutated = self.mutated.last().cloned().unwrap_or_default();
        for (name, at) in alias_violations(list, &item_names) {
            self.report(diag(
                "ts-mutation-escape",
                at,
                format!("{name} is stored here while a later change to it can still run, so the stored copy would differ from JavaScript's alias"),
            ));
        }
        let mut hoisted: Vec<usize> = Vec::new();
        for (position, item) in items.iter().enumerate() {
            let free = free_names(item.function);
            let calls_self_or_later = free.iter().any(|n| item_names.contains(n.as_str()));
            let reaches = self.reaches_self(&items, position, &free);
            if reaches {
                self.report(
                    diag("ts-recursion", item.name_span, "A function may not call itself").fix("loop({ max: N }, state, fn)"),
                );
            }
            let _ = calls_self_or_later;
            if free.iter().any(|n| mutated.contains(n) && !item_names.contains(n.as_str())) {
                let name = free.iter().find(|n| mutated.contains(*n)).cloned().unwrap_or_default();
                self.report(diag(
                    "ts-mutation-escape",
                    item.function.span,
                    format!("The function mentions {name}, which the program assigns"),
                ));
            }
            if !free.iter().any(|n| local_decls.contains(n)) {
                hoisted.push(position);
            }
        }
        let order = self.dependency_order(&items, &hoisted);
        for &position in &order {
            let item = &items[position];
            self.declare_function(&item.name, item.function);
        }
        let mut defns = Vec::new();
        let mut bindings: Vec<(Node, Node)> = Vec::new();
        for &position in &order {
            let item = &items[position];
            let (params, body) = self.function_parts(item.function, false, false);
            let sx = self.sx_name(&item.name);
            let span = item.function.span;
            if top {
                defns.push(form(vec![sym("defn", span), sym(&sx, span), params, body], span));
                self.defn_names.push(sx);
            } else {
                bindings.push((sym(&sx, span), form(vec![sym("fn", span), params, body], span)));
            }
        }
        let skip: HashSet<usize> = order.iter().map(|&p| items[p].index).collect();
        let remaining: Vec<Stmt> = list.iter().enumerate().filter(|(i, _)| !skip.contains(i)).map(|(_, s)| s.clone()).collect();
        let main = self.stmts(&remaining, k);
        let span = main.span;
        (defns, let_(bindings, main, span))
    }

    fn reaches_self(&self, items: &[FnItem<'_>], start: usize, free: &HashSet<String>) -> bool {
        let mut seen: HashSet<usize> = HashSet::new();
        let mut pending: Vec<usize> = Vec::new();
        let successors = |names: &HashSet<String>| -> Vec<usize> {
            items.iter().enumerate().filter(|(_, i)| names.contains(&i.name)).map(|(p, _)| p).collect()
        };
        pending.extend(successors(free));
        while let Some(at) = pending.pop() {
            if at == start {
                return true;
            }
            if seen.insert(at) {
                let names = free_names(items[at].function);
                pending.extend(successors(&names));
            }
        }
        false
    }

    fn dependency_order(&self, items: &[FnItem<'_>], hoisted: &[usize]) -> Vec<usize> {
        let mut order: Vec<usize> = Vec::new();
        fn visit(items: &[FnItem<'_>], hoisted: &[usize], at: usize, order: &mut Vec<usize>, stack: &mut Vec<usize>) {
            if order.contains(&at) || stack.contains(&at) {
                return;
            }
            stack.push(at);
            let names = free_names(items[at].function);
            for &other in hoisted {
                if names.contains(&items[other].name) {
                    visit(items, hoisted, other, order, stack);
                }
            }
            stack.pop();
            order.push(at);
        }
        for &at in hoisted {
            visit(items, hoisted, at, &mut order, &mut Vec::new());
        }
        order
    }

    pub fn declare_function(&mut self, name: &str, function: &Function) -> String {
        let mut template = self.plain_var();
        template.function = Some(function.is_async);
        template.is_const = true;
        self.declare(name, template)
    }

    pub fn function_parts(&mut self, function: &Function, promise_ok: bool, tuple_index: bool) -> (Node, Node) {
        let span = function.span;
        self.fn_depth += 1;
        self.frames.push(Frame::Function);
        self.scopes.push(Scope::new());
        let saved_promise = std::mem::replace(&mut self.promise_ok, promise_ok);
        let saved_parallel = self.parallel;
        let mutated = match &function.body {
            FunctionBody::Block(body) => all_mutated(body),
            FunctionBody::Expr(_) => HashSet::new(),
        };
        self.mutated.push(mutated);
        let mut params: Vec<Node> = Vec::new();
        let mut wrap: Vec<(Node, Node)> = Vec::new();
        for param in &function.params {
            match &param.kind {
                PatternKind::Ident(name) => {
                    let mut template = self.plain_var();
                    template.hint = self.param_hint.take().unwrap_or(Hint::Unknown);
                    let sx = self.declare(name, template);
                    params.push(sym(&sx, param.span));
                }
                _ => {
                    self.param_hint = None;
                    let temp = self.fresh("arg");
                    params.push(sym(&temp, param.span));
                    let value = sym(&temp, param.span);
                    let template = self.plain_var();
                    wrap.extend(self.bind_pattern(param, value, false, &template));
                }
            }
        }
        if tuple_index && params.len() == 2 {
            let element = params.remove(0);
            let index = params.remove(0);
            params = vec![list(vec![index, element], span)];
        }
        let body = match &function.body {
            FunctionBody::Expr(e) => self.value_of(e, true),
            FunctionBody::Block(stmts) => {
                let (_, node) = self.body(stmts, &|l: &mut Lowerer<'a>| unit(l.end_span()), false);
                node
            }
        };
        let body = let_(wrap, body, span);
        self.mutated.pop();
        self.promise_ok = saved_promise;
        self.parallel = saved_parallel;
        self.scopes.pop();
        self.frames.pop();
        self.fn_depth -= 1;
        (list(params, span), body)
    }

    pub fn end_span(&self) -> Span {
        let line = self.lines.len().max(1);
        let column = self.lines.last().map_or(1, |l| l.chars().count() + 1);
        let at = sx_kernel::syntax::Position { line, column };
        Span::new(at, at)
    }

    pub fn bind_pattern(&mut self, pattern: &Pattern, value: Node, tuple_ok: bool, template: &Var) -> Vec<(Node, Node)> {
        let span = pattern.span;
        match &pattern.kind {
            PatternKind::Ident(name) => {
                let sx = self.declare(name, template.clone());
                vec![(sym(&sx, span), value)]
            }
            PatternKind::Object(props) => {
                let temp = self.fresh("obj");
                let mut bindings = vec![(sym(&temp, span), value)];
                for prop in props {
                    let field = app("get", vec![sym(&temp, span), kw(&prop.key, prop.value.span)], prop.value.span);
                    bindings.extend(self.bind_pattern(&prop.value, field, false, template));
                }
                bindings
            }
            PatternKind::Array(items) => {
                let all_idents = items.iter().all(|p| matches!(p.kind, PatternKind::Ident(_)));
                if tuple_ok && all_idents {
                    let mut names = Vec::new();
                    for item in items {
                        if let PatternKind::Ident(name) = &item.kind {
                            let sx = self.declare(name, template.clone());
                            names.push(sym(&sx, item.span));
                        }
                    }
                    return vec![(list(names, span), value)];
                }
                let temp = self.fresh("arr");
                let mut bindings = vec![(sym(&temp, span), value)];
                for (index, item) in items.iter().enumerate() {
                    let element = app("nth", vec![sym(&temp, span), int(index as i64, span)], item.span);
                    bindings.extend(self.bind_pattern(item, element, false, template));
                }
                bindings
            }
        }
    }

    pub fn stmts(&mut self, list: &[Stmt], k: Cont<'_, 'a>) -> Node {
        let Some((first, rest)) = list.split_first() else {
            return k(self);
        };
        self.stmt(first, rest, k)
    }

    fn leave(&mut self, saved: &[Scope], rest: &[Stmt], k: Cont<'_, 'a>) -> Node {
        self.with_scopes(saved.to_vec(), |l| l.stmts(rest, k))
    }

    pub fn branch(&mut self, stmt: &Stmt, k: Cont<'_, 'a>) -> Node {
        let saved = self.scopes.clone();
        self.scopes.push(Scope::new());
        let node = self.stmts(std::slice::from_ref(stmt), k);
        self.scopes = saved;
        node
    }

    pub fn branch_list(&mut self, list: &[Stmt], k: Cont<'_, 'a>) -> Node {
        let saved = self.scopes.clone();
        self.scopes.push(Scope::new());
        let node = self.stmts(list, k);
        self.scopes = saved;
        node
    }

    fn existing(&self, names: Vec<String>) -> Vec<String> {
        names.into_iter().filter(|n| self.is_bound(n)).collect()
    }

    fn stmt(&mut self, first: &Stmt, rest: &[Stmt], k: Cont<'_, 'a>) -> Node {
        let span = first.span;
        match &first.kind {
            StmtKind::Empty | StmtKind::TypeDecl(..) => self.stmts(rest, k),
            StmtKind::Rejected(children) => {
                self.frames.push(Frame::Loop(Layout::default()));
                let saved = self.scopes.clone();
                let _ = self.stmts(children, &|l: &mut Lowerer<'a>| unit(l.end_span()));
                self.scopes = saved;
                self.frames.pop();
                self.stmts(rest, k)
            }
            StmtKind::Decl(kind, decls) => self.decl(*kind, decls, span, rest, k),
            StmtKind::Expr(expr) => self.expr_stmt(expr, span, rest, k),
            StmtKind::Function(function) => {
                let name = function.name.as_ref().map(|n| n.name.as_str()).unwrap_or("");
                let sx = self.declare_function(name, function);
                let (params, body) = self.function_parts(function, false, false);
                let value = form(vec![sym("fn", span), params, body], span);
                let rest_node = self.stmts(rest, k);
                let_(vec![(sym(&sx, span), value)], rest_node, span)
            }
            StmtKind::Return(value) => self.exit(Exit::Return(value.as_ref()), span),
            StmtKind::Break => self.exit(Exit::Break, span),
            StmtKind::Continue => self.exit(Exit::Continue, span),
            StmtKind::Throw(value) => self.throw(value, span),
            StmtKind::Block(body) => {
                let saved = self.scopes.clone();
                self.scopes.push(Scope::new());
                let kk = |l: &mut Lowerer<'a>| l.leave(&saved, rest, k);
                let node = self.stmts(body, &kk);
                self.scopes = saved.clone();
                node
            }
            StmtKind::If(condition, then, otherwise) => self.if_stmt(condition, then, otherwise.as_deref(), span, rest, k),
            StmtKind::ForOf { pattern, source, body } => {
                let source_node = self.value_of(source, false);
                let element_hint = if self.hint_of(source) == Hint::SettledList { Hint::Settled } else { Hint::Unknown };
                self.for_of(pattern, source_node, element_hint, body, span, rest, k)
            }
            StmtKind::For { init, test, update, body } => self.counted_for(init.as_deref(), test.as_ref(), update.as_ref(), body, span, rest, k),
            StmtKind::Try { block, param, handler } => self.try_stmt(block, param.as_ref(), handler, span, rest, k),
        }
    }

    fn throw(&mut self, value: &Expr, span: Span) -> Node {
        match &value.kind {
            ExprKind::New(name, args) if name == "Error" && args.len() == 1 => {
                let message = self.value_of(&args[0], false);
                app("fail", vec![message], span)
            }
            ExprKind::Ident(name) if self.lookup(name).is_some_and(|v| v.catch.is_some()) => {
                let sx = self.sx_name(name);
                app("fail", vec![sym(&sx, value.span)], span)
            }
            _ => self.reject_fix(
                "ts-throw",
                span,
                "Only throw new Error(text) and rethrowing the catch parameter are supported",
                "throw new Error(message)",
            ),
        }
    }

    fn if_stmt(&mut self, condition: &Expr, then: &Stmt, otherwise: Option<&Stmt>, span: Span, rest: &[Stmt], k: Cont<'_, 'a>) -> Node {
        let mut exit = stmt_exits_of(then);
        if let Some(otherwise) = otherwise {
            let more = stmt_exits_of(otherwise);
            exit = Exits { ret: exit.ret || more.ret, brk: exit.brk || more.brk, cont: exit.cont || more.cont };
        }
        let cond = self.condition(condition);
        if exit.any() {
            let snapshot = self.scopes.clone();
            let kk = |l: &mut Lowerer<'a>| l.leave(&snapshot, rest, k);
            let a = self.branch(then, &kk);
            let b = match otherwise {
                Some(otherwise) => self.branch(otherwise, &kk),
                None => kk(self),
            };
            return app("if", vec![cond, a, b], span);
        }
        let mut vars = assigned_outer_stmt(then);
        if let Some(otherwise) = otherwise {
            for name in assigned_outer_stmt(otherwise) {
                if !vars.contains(&name) {
                    vars.push(name);
                }
            }
        }
        let vars = self.existing(vars);
        self.require_assigned(&vars, then, otherwise, span);
        let state = |l: &mut Lowerer<'a>| l.state_node(&vars, span);
        let a = self.branch(then, &state);
        let b = match otherwise {
            Some(otherwise) => self.branch(otherwise, &state),
            None => state(self),
        };
        let join = app("if", vec![cond, a, b], span);
        for name in &vars {
            if let Some(var) = self.lookup_mut(name) {
                var.uninit = false;
            }
        }
        let rest_node = self.stmts(rest, k);
        let pattern = self.state_pattern(&vars, span);
        let_(vec![(pattern, join)], rest_node, span)
    }

    fn require_assigned(&mut self, vars: &[String], then: &Stmt, otherwise: Option<&Stmt>, span: Span) {
        for name in vars {
            let uninit = self.lookup(name).is_some_and(|v| v.uninit);
            if !uninit {
                continue;
            }
            let in_then = stmt_assigns_all(then, name);
            let in_else = otherwise.is_some_and(|o| stmt_assigns_all(o, name));
            if !(in_then && in_else) {
                self.report(diag("ts-uninitialized", span, "This name may be read before it is assigned").fix("give it a value"));
            }
        }
    }

    fn decl(&mut self, kind: DeclKind, decls: &[Declarator], span: Span, rest: &[Stmt], k: Cont<'_, 'a>) -> Node {
        let mut bindings: Vec<(Node, Node)> = Vec::new();
        for decl in decls {
            let dspan = decl.pattern.span;
            if let (PatternKind::Ident(name), Some(Expr { kind: ExprKind::Arrow(function), .. })) = (&decl.pattern.kind, &decl.init) {
                let sx = self.declare_function(name, function);
                let (params, body) = self.function_parts(function, false, false);
                bindings.push((sym(&sx, dspan), form(vec![sym("fn", function.span), params, body], function.span)));
                continue;
            }
            let Some(init) = &decl.init else {
                match (&decl.pattern.kind, kind) {
                    (PatternKind::Ident(name), DeclKind::Let) => {
                        let mut template = self.plain_var();
                        template.uninit = true;
                        template.owned = true;
                        self.declare(name, template);
                    }
                    _ => {
                        self.report(diag("ts-uninitialized", dspan, "This name may be read before it is assigned").fix("give it a value"));
                    }
                }
                continue;
            };
            let mut template = self.plain_var();
            template.is_const = kind == DeclKind::Const;
            template.owned = is_fresh(init);
            template.hint = self.hint_of(init);
            let tolerant = matches!(init.kind, ExprKind::Chain(..)) && matches!(decl.pattern.kind, PatternKind::Ident(_));
            if tolerant {
                let PatternKind::Ident(name) = &decl.pattern.kind else { unreachable!("tolerant implies an identifier pattern") };
                if !chain_uses_ok(name, rest) {
                    self.report(diag("ts-optional-chain", init.span, "The result of ?. is undefined, not null").fix("x == null ? null : x.f"));
                }
            }
            let value = match (&decl.annotation, &init.kind) {
                (Some(annotation), ExprKind::Call(..)) if is_json_parse(init) => self.typed_parse(init, annotation),
                _ => self.expr_t(init, tolerant, false),
            };
            let tuple_ok = is_tuple_source(init)
                && match &decl.pattern.kind {
                    PatternKind::Array(items) => tuple_source_len(init) == Some(items.len()),
                    _ => true,
                };
            bindings.extend(self.bind_pattern(&decl.pattern, value, tuple_ok, &template));
            if let PatternKind::Array(items) = &decl.pattern.kind
                && matches!(self.hint_of(init), Hint::SettledList)
            {
                for item in items {
                    if let PatternKind::Ident(n) = &item.kind
                        && let Some(var) = self.lookup_mut(n)
                    {
                        var.hint = Hint::Settled;
                    }
                }
            }
        }
        let rest_node = self.stmts(rest, k);
        let_(bindings, rest_node, span)
    }

    fn expr_stmt(&mut self, expr: &Expr, span: Span, rest: &[Stmt], k: Cont<'_, 'a>) -> Node {
        if let Some(m) = mutation(expr) {
            return self.mutate(m, expr, span, rest, k);
        }
        if matches!(&expr.kind, ExprKind::Str(_)) {
            return self.stmts(rest, k);
        }
        let allowed = match &expr.kind {
            ExprKind::Call(..) | ExprKind::Chain(..) => true,
            ExprKind::Await(inner) => matches!(inner.kind, ExprKind::Call(..)),
            ExprKind::Rejected => true,
            _ => false,
        };
        if !allowed {
            self.report(diag("ts-unsupported", expr.span, "This expression statement is not supported in checkable TypeScript"));
        }
        let awaited = matches!(expr.kind, ExprKind::Await(_));
        let value = if awaited { self.value_of(expr, false) } else { self.statement_call(expr) };
        let rest_node = self.stmts(rest, k);
        seq(value, rest_node, span)
    }

    fn mutate(&mut self, m: Mutation<'_>, expr: &Expr, span: Span, rest: &[Stmt], k: Cont<'_, 'a>) -> Node {
        let name = mutated_name(expr).unwrap_or("").to_string();
        let Some(var) = self.lookup(&name).cloned() else {
            self.report(diag("ts-mutation-pattern", span, format!("{name} is not a variable of this program")));
            return self.stmts(rest, k);
        };
        if var.depth < self.fn_depth {
            self.report(diag("ts-mutation-escape", span, format!("A function may not assign {name}, which belongs to an outer function")));
            return self.stmts(rest, k);
        }
        if self.parallel > 0 {
            self.report(diag("ts-mutation-escape", span, "A parallel branch may not assign a variable outside itself"));
        }
        let current = sym(&var.sx, span);
        let new_value = match m {
            Mutation::Assign { op, value, .. } => {
                if var.is_const {
                    self.report(diag("ts-mutation-pattern", span, format!("{name} is a const, so it cannot be assigned")));
                    return self.stmts(rest, k);
                }
                let rhs = self.value_of(value, false);
                if let Some(v) = self.lookup_mut(&name) {
                    v.uninit = false;
                    if op == AssignOp::Set {
                        v.owned = is_fresh(value);
                        v.hint = Hint::Unknown;
                    }
                }
                match op {
                    AssignOp::Set => rhs,
                    AssignOp::Add if var.hint == Hint::Text => app("str", vec![current, rhs], span),
                    other => self.arithmetic(other, current, rhs, span),
                }
            }
            Mutation::Update { increment, .. } => {
                if var.is_const {
                    self.report(diag("ts-mutation-pattern", span, format!("{name} is a const, so it cannot be assigned")));
                    return self.stmts(rest, k);
                }
                let one = int(1, span);
                self.arithmetic(if increment { AssignOp::Add } else { AssignOp::Sub }, current, one, span)
            }
            Mutation::Push { args, .. } => {
                if !var.owned {
                    self.report(diag("ts-mutation-pattern", span, format!("push on {name}, which is not an array this function owns")));
                    return self.stmts(rest, k);
                }
                let items: Vec<Node> = args.iter().map(|a| self.value_of(a, false)).collect();
                app("concat", vec![current, list(items, span)], span)
            }
            Mutation::Put { path, op, value, .. } => {
                if !var.owned {
                    self.report(diag("ts-mutation-pattern", span, format!("The write to {name} is not to an object this function owns")));
                    return self.stmts(rest, k);
                }
                let rhs = self.value_of(value, false);
                self.put_path(current, &path, op, rhs, span)
            }
            Mutation::Reorder { method, args, .. } => {
                if !var.owned {
                    self.report(diag("ts-mutation-pattern", span, format!("{method} on {name}, which is not an array this function owns")));
                    return self.stmts(rest, k);
                }
                self.reorder(current, method, args, span)
            }
        };
        let rest_node = self.stmts(rest, k);
        let_(vec![(sym(&var.sx, span), new_value)], rest_node, span)
    }

    fn put_path(&mut self, base: Node, path: &[PathStep<'_>], op: AssignOp, rhs: Node, span: Span) -> Node {
        let Some((step, more)) = path.split_first() else {
            return rhs;
        };
        let key = match step {
            PathStep::Field(name) => string(name, span),
            PathStep::Key(e) => self.value_of(e, false),
        };
        let read = match step {
            PathStep::Field(name) => app("get", vec![dup(&base), kw(name, span)], span),
            PathStep::Key(_) => app("get", vec![dup(&base), dup(&key)], span),
        };
        let new = if more.is_empty() {
            match op {
                AssignOp::Set => rhs,
                other => self.arithmetic(other, read, rhs, span),
            }
        } else {
            self.put_path(read, more, op, rhs, span)
        };
        app("put", vec![base, key, new], span)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn for_of(&mut self, pattern: &Pattern, source: Node, element_hint: Hint, body: &Stmt, span: Span, rest: &[Stmt], k: Cont<'_, 'a>) -> Node {
        let vars = self.existing(assigned_outer_stmt(body));
        let exit = stmt_exits_of(body);
        let layout = Layout {
            done: (exit.brk || exit.ret).then(|| self.fresh("done")),
            ret: exit.ret.then(|| self.fresh("ret")),
            brk: None,
            cont: None,
            vars: vars.clone(),
        };
        self.frames.push(Frame::Loop(layout.clone()));
        self.scopes.push(Scope::new());
        let (param, wrap) = match &pattern.kind {
            PatternKind::Ident(name) => {
                let mut template = self.plain_var();
                template.hint = element_hint;
                let sx = self.declare(name, template);
                (sym(&sx, pattern.span), Vec::new())
            }
            _ => {
                let temp = self.fresh("item");
                let template = self.plain_var();
                let bindings = self.bind_pattern(pattern, sym(&temp, pattern.span), false, &template);
                (sym(&temp, pattern.span), bindings)
            }
        };
        let fall = |l: &mut Lowerer<'a>| l.fall_off(&layout, span);
        let body_node = self.branch(body, &fall);
        let body_node = let_(wrap, body_node, span);
        self.scopes.pop();
        self.frames.pop();
        let slots = self.layout_slots(&layout);
        if slots == 0 {
            let lambda = form(vec![sym("fn", span), list(vec![param], span), body_node], span);
            let mapped = app("map", vec![lambda, source], span);
            let rest_node = self.stmts(rest, k);
            return seq(mapped, rest_node, span);
        }
        let state_param = self.layout_pattern(&layout, span);
        let guarded = match &layout.done {
            Some(done) => {
                let again = self.layout_pattern(&layout, span);
                app("if", vec![sym(done, span), again, body_node], span)
            }
            None => body_node,
        };
        let lambda = form(vec![sym("fn", span), list(vec![state_param, param], span), guarded], span);
        let initial = self.layout_values(&layout, (false, false, false), None, span);
        let reduced = app("reduce", vec![lambda, initial, source], span);
        let after_pattern = {
            let mut layout_after = layout.clone();
            let ret_name = layout_after.ret.clone();
            layout_after.done = None;
            layout_after.ret = None;
            let mut nodes: Vec<Node> = Vec::new();
            if layout.done.is_some() {
                nodes.push(sym("_", span));
            }
            if let Some(ret) = &ret_name {
                nodes.push(sym(ret, span));
            }
            nodes.extend(layout.vars.iter().map(|n| sym(&self.sx_name(n), span)));
            if nodes.len() == 1 { nodes.remove(0) } else { list(nodes, span) }
        };
        for name in &layout.vars {
            if let Some(var) = self.lookup_mut(name) {
                var.uninit = false;
            }
        }
        let rest_node = self.stmts(rest, k);
        let after = match &layout.ret {
            Some(ret) => self.after_ret(ret, rest_node, span),
            None => rest_node,
        };
        let_(vec![(after_pattern, reduced)], after, span)
    }

    fn after_ret(&mut self, ret: &str, rest_node: Node, span: Span) -> Node {
        let bound = self.fresh("r");
        let exit = self.exit(Exit::ReturnValue(sym(&bound, span)), span);
        form(
            vec![sym("match", span), sym(ret, span), form(vec![sym("some", span), sym(&bound, span)], span), exit, sym("none", span), rest_node],
            span,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn counted_for(
        &mut self,
        init: Option<&Stmt>,
        test: Option<&Expr>,
        update: Option<&Expr>,
        body: &Stmt,
        span: Span,
        rest: &[Stmt],
        k: Cont<'_, 'a>,
    ) -> Node {
        let (Some(init), Some(test), Some(update)) = (init, test, update) else {
            self.report(diag("ts-while", span, "Loops must be bounded").fix("retry, waitUntil, loop({ max: N }, state, fn), or for (const x of xs)"));
            return self.stmts(rest, k);
        };
        let reject = |l: &mut Self, why: &str| {
            l.report(diag("ts-counted-loop", span, format!("A counted for loop must {why}")));
        };
        let StmtKind::Decl(DeclKind::Let, decls) = &init.kind else {
            reject(self, "declare one let counter");
            return self.stmts(rest, k);
        };
        let [decl] = decls.as_slice() else {
            reject(self, "declare one let counter");
            return self.stmts(rest, k);
        };
        let (PatternKind::Ident(counter), Some(start)) = (&decl.pattern.kind, &decl.init) else {
            reject(self, "declare one let counter with an initial value");
            return self.stmts(rest, k);
        };
        if matches!(start.kind, ExprKind::Number(Number::Float(_))) {
            reject(self, "count with an integer");
            return self.stmts(rest, k);
        }
        let ExprKind::Binary(op @ (BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge), left, bound) = &test.kind else {
            reject(self, "test the counter with <, <=, > or >=");
            return self.stmts(rest, k);
        };
        if !matches!(&left.kind, ExprKind::Ident(n) if n == counter) {
            reject(self, "test the counter on the left");
            return self.stmts(rest, k);
        }
        let ascending = matches!(op, BinaryOp::Lt | BinaryOp::Le);
        let step = match &update.kind {
            ExprKind::Update { increment, target } if matches!(&target.kind, ExprKind::Ident(n) if n == counter) && *increment == ascending => Some(1i64),
            ExprKind::Assign(aop, target, amount)
                if matches!(&target.kind, ExprKind::Ident(n) if n == counter)
                    && *aop == if ascending { AssignOp::Add } else { AssignOp::Sub } =>
            {
                match amount.kind {
                    ExprKind::Number(Number::Int(n)) if n > 0 => Some(n),
                    _ => None,
                }
            }
            _ => None,
        };
        let Some(step) = step else {
            reject(self, "step the counter in the direction its test implies, by a positive integer literal");
            return self.stmts(rest, k);
        };
        let assigned = assigned_outer_stmt(body);
        if assigned.contains(counter) {
            reject(self, "never assign the counter in its body");
            return self.stmts(rest, k);
        }
        if !self.invariant_bound(bound, &assigned, counter) {
            reject(self, "have a bound the body does not change and that calls nothing");
            return self.stmts(rest, k);
        }
        let from = self.value_of(start, false);
        let to = self.value_of(bound, false);
        let to = match (op, &to.kind) {
            (BinaryOp::Le, sx_kernel::syntax::NodeKind::Int(n)) => int(n + 1, span),
            (BinaryOp::Ge, sx_kernel::syntax::NodeKind::Int(n)) => int(n - 1, span),
            (BinaryOp::Le, _) => app("+", vec![to, int(1, span)], span),
            (BinaryOp::Ge, _) => app("-", vec![to, int(1, span)], span),
            _ => to,
        };
        let signed = if ascending { step } else { -step };
        let mut args = vec![from, to];
        if signed != 1 {
            args.push(int(signed, span));
        }
        let source = app("range", args, span);
        let pattern = Pattern { kind: PatternKind::Ident(counter.clone()), span: decl.pattern.span };
        self.for_of(&pattern, source, Hint::Unknown, body, span, rest, k)
    }

    fn invariant_bound(&self, bound: &Expr, assigned: &[String], counter: &str) -> bool {
        let mut ok = true;
        walk_expr(bound, &mut |e| match &e.kind {
            ExprKind::Ident(n) => {
                if assigned.contains(n) || n == counter {
                    ok = false;
                }
            }
            ExprKind::Call(..) | ExprKind::Chain(..) | ExprKind::Await(_) | ExprKind::Arrow(_) | ExprKind::Assign(..) | ExprKind::Update { .. } => ok = false,
            _ => {}
        });
        ok
    }

    #[allow(clippy::too_many_arguments)]
    fn try_stmt(&mut self, block: &[Stmt], param: Option<&Pattern>, handler: &[Stmt], span: Span, rest: &[Stmt], k: Cont<'_, 'a>) -> Node {
        let mut vars = assigned_outer(block);
        for name in assigned_outer(handler) {
            if !vars.contains(&name) {
                vars.push(name);
            }
        }
        let vars = self.existing(vars);
        let mut ex = exits(block);
        let more = exits(handler);
        ex = Exits { ret: ex.ret || more.ret, brk: ex.brk || more.brk, cont: ex.cont || more.cont };
        let events = try_events(block, &self.throwing);
        let mut seen_assignment: Option<(String, Span)> = None;
        for event in events {
            match event {
                Event::Assignment(name, at) => {
                    if seen_assignment.is_none() {
                        seen_assignment = Some((name, at));
                    }
                }
                Event::Throwing => {
                    if let Some((name, at)) = seen_assignment.take() {
                        self.report(diag(
                            "ts-mutation-in-try",
                            at,
                            format!("The assignment to {name} comes before an operation in the try block that can throw"),
                        ));
                    }
                }
            }
        }
        let handler_exits = diverges(handler);
        let json_parse = {
            let mut found = false;
            walk_stmts(block, &mut |e| {
                if let ExprKind::Call(callee, _) = &e.kind
                    && let ExprKind::Field(receiver, method) = &callee.kind
                    && method == "parse"
                    && matches!(&receiver.kind, ExprKind::Ident(n) if n == "JSON")
                {
                    found = true;
                }
            });
            found
        };
        let mixed = contains_throw(block) || json_parse || calls_user_function(block, &self.throwing);
        let catch_kind = if mixed { CatchKind::Mixed { json_parse } } else { CatchKind::ToolOnly };
        if handler_exits && !exits(block).any() {
            let vars_t = self.existing(assigned_outer(block));
            let fall = |l: &mut Lowerer<'a>| l.state_node(&vars_t, span);
            let attempted = self.branch_list(block, &fall);
            self.scopes.push(Scope::new());
            let error_sx = match param {
                Some(Pattern { kind: PatternKind::Ident(name), .. }) => {
                    let mut template = self.plain_var();
                    template.catch = Some(catch_kind);
                    template.is_const = true;
                    self.declare(name, template)
                }
                Some(other) => {
                    self.report(diag("ts-error-use", other.span, "A catch parameter may not be destructured").fix("e.message"));
                    "_".to_string()
                }
                None => "_".to_string(),
            };
            let caught = self.branch_list(handler, &|l: &mut Lowerer<'a>| unit(l.end_span()));
            self.scopes.pop();
            for name in &vars_t {
                if let Some(var) = self.lookup_mut(name) {
                    var.uninit = false;
                }
            }
            let rest_node = self.stmts(rest, k);
            let pattern = self.state_pattern(&vars_t, span);
            let ok_pattern = if vars_t.is_empty() { sym("_", span) } else { pattern };
            return form(
                vec![
                    sym("match", span),
                    app("attempt", vec![attempted], span),
                    app("ok", vec![ok_pattern], span),
                    rest_node,
                    app("err", vec![sym(&error_sx, span)], span),
                    caught,
                ],
                span,
            );
        }
        let layout = Layout {
            done: None,
            brk: ex.brk.then(|| self.fresh("brk")),
            cont: ex.cont.then(|| self.fresh("cont")),
            ret: ex.ret.then(|| self.fresh("ret")),
            vars: vars.clone(),
        };
        self.frames.push(Frame::Try(layout.clone()));
        let fall = |l: &mut Lowerer<'a>| l.fall_off(&layout, span);
        let attempted = self.branch_list(block, &fall);
        self.scopes.push(Scope::new());
        let error_sx = match param {
            Some(Pattern { kind: PatternKind::Ident(name), .. }) => {
                let mut template = self.plain_var();
                template.catch = Some(catch_kind);
                template.is_const = true;
                self.declare(name, template)
            }
            Some(other) => {
                self.report(diag("ts-error-use", other.span, "A catch parameter may not be destructured").fix("e.message"));
                "_".to_string()
            }
            None => "_".to_string(),
        };
        let caught = self.branch_list(handler, &fall);
        self.scopes.pop();
        self.frames.pop();
        for name in &layout.vars {
            if let Some(var) = self.lookup_mut(name) {
                var.uninit = false;
            }
        }
        let ok_value = self.fresh("v");
        let slots = self.layout_slots(&layout);
        let ok_arm = if slots == 0 { unit(span) } else { sym(&ok_value, span) };
        let ok_pattern = if slots == 0 { sym("_", span) } else { sym(&ok_value, span) };
        let joined = form(
            vec![
                sym("match", span),
                app("attempt", vec![attempted], span),
                app("ok", vec![ok_pattern], span),
                ok_arm,
                app("err", vec![sym(&error_sx, span)], span),
                caught,
            ],
            span,
        );
        let rest_node = self.stmts(rest, k);
        let mut after = if diverges(block) && diverges(handler) {
            app("fail", vec![string("unreachable", span)], span)
        } else {
            rest_node
        };
        if let Some(ret) = &layout.ret {
            after = self.after_ret(ret, after, span);
        }
        if let Some(cont) = &layout.cont {
            let exit = self.exit(Exit::Continue, span);
            after = app("if", vec![sym(cont, span), exit, after], span);
        }
        if let Some(brk) = &layout.brk {
            let exit = self.exit(Exit::Break, span);
            after = app("if", vec![sym(brk, span), exit, after], span);
        }
        let pattern = {
            let mut nodes: Vec<Node> = Vec::new();
            for flag in [&layout.done, &layout.brk, &layout.cont, &layout.ret].into_iter().flatten() {
                nodes.push(sym(flag, span));
            }
            nodes.extend(layout.vars.iter().map(|n| sym(&self.sx_name(n), span)));
            match nodes.len() {
                0 => sym("_", span),
                1 => nodes.remove(0),
                _ => list(nodes, span),
            }
        };
        let_(vec![(pattern, joined)], after, span)
    }
}

pub fn diverges(stmts: &[Stmt]) -> bool {
    match stmts.iter().rev().find(|s| !matches!(s.kind, StmtKind::Empty)).map(|s| &s.kind) {
        Some(StmtKind::Return(_) | StmtKind::Break | StmtKind::Continue | StmtKind::Throw(_)) => true,
        Some(StmtKind::Block(body)) => diverges(body),
        Some(StmtKind::If(_, a, Some(b))) => diverges(std::slice::from_ref(a)) && diverges(std::slice::from_ref(b)),
        _ => false,
    }
}

fn stmt_assigns_all(stmt: &Stmt, name: &str) -> bool {
    definitely_assigns(std::slice::from_ref(stmt), name)
}

pub fn is_fresh(init: &Expr) -> bool {
    match &init.kind {
        ExprKind::Array(_) | ExprKind::Object(_) => true,
        ExprKind::Await(inner) => is_fresh(inner),
        ExprKind::Call(callee, _) => match &callee.kind {
            ExprKind::Field(receiver, method) => match (&receiver.kind, method.as_str()) {
                (ExprKind::Ident(object), "fromEntries") if object == "Object" => true,
                (ExprKind::Ident(object), "parse") if object == "JSON" => true,
                (ExprKind::Ident(object), "all" | "allSettled") if object == "Promise" => true,
                (ExprKind::Field(inner, _), _) if matches!(&inner.kind, ExprKind::Ident(t) if t == "tools") => true,
                (_, "map" | "filter" | "slice" | "flatMap" | "concat" | "split") => true,
                _ => false,
            },
            ExprKind::Ident(name) => matches!(name.as_str(), "parMap" | "retry" | "loop" | "waitUntil"),
            _ => false,
        },
        _ => false,
    }
}

pub fn is_json_parse(e: &Expr) -> bool {
    matches!(&e.kind, ExprKind::Call(callee, _)
        if matches!(&callee.kind, ExprKind::Field(r, m) if m == "parse" && matches!(&r.kind, ExprKind::Ident(o) if o == "JSON")))
}

fn tuple_source_len(e: &Expr) -> Option<usize> {
    match &e.kind {
        ExprKind::Await(inner) => tuple_source_len(inner),
        ExprKind::Array(items) => Some(items.len()),
        ExprKind::Call(_, args) => match args.first().map(|a| &a.kind) {
            Some(ExprKind::Array(items)) => Some(items.len()),
            _ => None,
        },
        _ => None,
    }
}

pub fn is_tuple_source(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Await(inner) => is_tuple_source(inner),
        ExprKind::Array(_) => true,
        ExprKind::Call(callee, args) => {
            matches!(&callee.kind, ExprKind::Field(r, m) if matches!(&r.kind, ExprKind::Ident(o) if o == "Promise") && (m == "all" || m == "allSettled"))
                && matches!(args.first().map(|a| &a.kind), Some(ExprKind::Array(_)))
        }
        _ => false,
    }
}

pub fn chain_uses_ok(name: &str, rest: &[Stmt]) -> bool {
    let mut ok = true;
    for stmt in rest {
        scan_stmt_uses(stmt, name, &mut ok);
    }
    ok
}

fn scan_stmt_uses(stmt: &Stmt, name: &str, ok: &mut bool) {
    match &stmt.kind {
        StmtKind::Decl(_, decls) => decls.iter().filter_map(|d| d.init.as_ref()).for_each(|e| scan_uses(e, name, false, false, ok)),
        StmtKind::Expr(e) | StmtKind::Throw(e) | StmtKind::Return(Some(e)) => scan_uses(e, name, false, false, ok),
        StmtKind::If(c, a, b) => {
            scan_uses(c, name, true, true, ok);
            scan_stmt_uses(a, name, ok);
            if let Some(b) = b {
                scan_stmt_uses(b, name, ok);
            }
        }
        StmtKind::ForOf { source, body, .. } => {
            scan_uses(source, name, false, false, ok);
            scan_stmt_uses(body, name, ok);
        }
        StmtKind::For { init, test, update, body } => {
            if let Some(init) = init {
                scan_stmt_uses(init, name, ok);
            }
            if let Some(test) = test {
                scan_uses(test, name, false, false, ok);
            }
            if let Some(update) = update {
                scan_uses(update, name, false, false, ok);
            }
            scan_stmt_uses(body, name, ok);
        }
        StmtKind::Try { block, handler, .. } => block.iter().chain(handler).for_each(|s| scan_stmt_uses(s, name, ok)),
        StmtKind::Block(body) => body.iter().for_each(|s| scan_stmt_uses(s, name, ok)),
        StmtKind::Function(f) => match &f.body {
            FunctionBody::Block(body) => body.iter().for_each(|s| scan_stmt_uses(s, name, ok)),
            FunctionBody::Expr(e) => scan_uses(e, name, false, false, ok),
        },
        _ => {}
    }
}

fn literal_non_null(e: &Expr) -> bool {
    matches!(e.kind, ExprKind::Str(_) | ExprKind::Number(_) | ExprKind::Bool(_))
}

fn scan_uses(e: &Expr, name: &str, tolerant: bool, in_test: bool, ok: &mut bool) {
    match &e.kind {
        ExprKind::Ident(n) => {
            if n == name && !tolerant {
                *ok = false;
            }
        }
        ExprKind::Binary(op, l, r) => match op {
            BinaryOp::Coalesce | BinaryOp::Or => {
                scan_uses(l, name, true, in_test, ok);
                scan_uses(r, name, false, false, ok);
            }
            BinaryOp::And => {
                scan_uses(l, name, in_test, in_test, ok);
                scan_uses(r, name, false, in_test, ok);
            }
            BinaryOp::LooseEqNull | BinaryOp::LooseNeNull => scan_uses(l, name, true, false, ok),
            BinaryOp::StrictEq | BinaryOp::StrictNe => {
                scan_uses(l, name, literal_non_null(r), false, ok);
                scan_uses(r, name, literal_non_null(l), false, ok);
            }
            _ => {
                scan_uses(l, name, false, false, ok);
                scan_uses(r, name, false, false, ok);
            }
        },
        ExprKind::Unary(UnaryOp::Not, inner) => scan_uses(inner, name, true, true, ok),
        ExprKind::Conditional(c, a, b) => {
            scan_uses(c, name, true, true, ok);
            scan_uses(a, name, false, false, ok);
            scan_uses(b, name, false, false, ok);
        }
        ExprKind::Call(callee, args) => {
            let tool = matches!(&callee.kind, ExprKind::Field(r, _) if matches!(&r.kind, ExprKind::Field(t, _) if matches!(&t.kind, ExprKind::Ident(n) if n == "tools")));
            scan_uses(callee, name, false, false, ok);
            for (index, arg) in args.iter().enumerate() {
                match (&arg.kind, tool && index == 0) {
                    (ExprKind::Object(items), true) => {
                        for item in items {
                            match item {
                                ObjectItem::Field { value, .. } => scan_uses(value, name, true, false, ok),
                                ObjectItem::Spread(value) => scan_uses(value, name, false, false, ok),
                            }
                        }
                    }
                    _ => scan_uses(arg, name, false, false, ok),
                }
            }
        }
        ExprKind::Await(inner) => scan_uses(inner, name, false, false, ok),
        _ => {
            let mut children: Vec<&Expr> = Vec::new();
            let mut first = true;
            walk_expr(e, &mut |child| {
                if first {
                    first = false;
                } else {
                    children.push(child);
                }
            });
            let direct: Vec<&Expr> = direct_children(e);
            let _ = children;
            for child in direct {
                scan_uses(child, name, false, false, ok);
            }
            if let ExprKind::Arrow(function) = &e.kind
                && let FunctionBody::Block(body) = &function.body
            {
                body.iter().for_each(|s| scan_stmt_uses(s, name, ok));
            }
        }
    }
}

fn direct_children(e: &Expr) -> Vec<&Expr> {
    match &e.kind {
        ExprKind::Template(_, parts) => parts.iter().collect(),
        ExprKind::Array(items) => items.iter().map(|i| match i { ArrayItem::Item(e) | ArrayItem::Spread(e) => e }).collect(),
        ExprKind::Object(items) => items.iter().map(|i| match i { ObjectItem::Field { value, .. } | ObjectItem::Spread(value) => value }).collect(),
        ExprKind::Field(base, _) => vec![base],
        ExprKind::Index(base, index) => vec![base, index],
        ExprKind::Chain(head, links) => {
            let mut out: Vec<&Expr> = vec![head];
            for link in links {
                match link {
                    Link::Access { access: Access::Index(index), .. } => out.push(index),
                    Link::Call { args, .. } => out.extend(args.iter()),
                    _ => {}
                }
            }
            out
        }
        ExprKind::New(_, args) => args.iter().collect(),
        ExprKind::Cast(inner, _) | ExprKind::Unary(_, inner) | ExprKind::Await(inner) => vec![inner],
        ExprKind::Arrow(function) => match &function.body {
            FunctionBody::Expr(body) => vec![body],
            FunctionBody::Block(_) => vec![],
        },
        ExprKind::Assign(_, l, r) => vec![l, r],
        ExprKind::Update { target, .. } => vec![target],
        _ => vec![],
    }
}
