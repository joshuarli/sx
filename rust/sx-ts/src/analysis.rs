use std::collections::HashSet;

use crate::ast::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Exits {
    pub ret: bool,
    pub brk: bool,
    pub cont: bool,
}

impl Exits {
    pub fn any(self) -> bool {
        self.ret || self.brk || self.cont
    }

    fn merge(&mut self, other: Exits) {
        self.ret |= other.ret;
        self.brk |= other.brk;
        self.cont |= other.cont;
    }
}

pub fn exits(stmts: &[Stmt]) -> Exits {
    let mut found = Exits::default();
    for stmt in stmts {
        found.merge(stmt_exits(stmt, false));
    }
    found
}

pub fn stmt_exits_of(stmt: &Stmt) -> Exits {
    stmt_exits(stmt, false)
}

fn stmt_exits(stmt: &Stmt, in_loop: bool) -> Exits {
    let mut found = Exits::default();
    match &stmt.kind {
        StmtKind::Return(_) => found.ret = true,
        StmtKind::Break => found.brk = !in_loop,
        StmtKind::Continue => found.cont = !in_loop,
        StmtKind::If(_, then, otherwise) => {
            found.merge(stmt_exits(then, in_loop));
            if let Some(otherwise) = otherwise {
                found.merge(stmt_exits(otherwise, in_loop));
            }
        }
        StmtKind::Block(body) => body.iter().for_each(|s| found.merge(stmt_exits(s, in_loop))),
        StmtKind::Try { block, handler, .. } => {
            block.iter().chain(handler).for_each(|s| found.merge(stmt_exits(s, in_loop)));
        }
        StmtKind::ForOf { body, .. } | StmtKind::For { body, .. } => found.merge(stmt_exits(body, true)),
        _ => {}
    }
    found
}

pub fn pattern_names(pattern: &Pattern, out: &mut Vec<String>) {
    match &pattern.kind {
        PatternKind::Ident(name) => out.push(name.clone()),
        PatternKind::Object(props) => props.iter().for_each(|p| pattern_names(&p.value, out)),
        PatternKind::Array(items) => items.iter().for_each(|p| pattern_names(p, out)),
    }
}

pub enum PathStep<'a> {
    Field(&'a str),
    Key(&'a Expr),
}

pub enum Mutation<'a> {
    Assign { name: &'a str, op: AssignOp, value: &'a Expr },
    Update { name: &'a str, increment: bool },
    Push { name: &'a str, args: &'a [Expr] },
    Put { name: &'a str, path: Vec<PathStep<'a>>, op: AssignOp, value: &'a Expr },
    Reorder { name: &'a str, method: &'a str, args: &'a [Expr] },
}

fn path_of<'a>(target: &'a Expr, steps: &mut Vec<PathStep<'a>>) -> Option<&'a str> {
    match &target.kind {
        ExprKind::Ident(name) => Some(name),
        ExprKind::Field(base, field) => {
            let root = path_of(base, steps)?;
            steps.push(PathStep::Field(field));
            Some(root)
        }
        ExprKind::Index(base, key) => {
            let root = path_of(base, steps)?;
            match &key.kind {
                ExprKind::Str(text) => steps.push(PathStep::Field(text)),
                _ => steps.push(PathStep::Key(key)),
            }
            Some(root)
        }
        _ => None,
    }
}

pub fn mutation(expr: &Expr) -> Option<Mutation<'_>> {
    match &expr.kind {
        ExprKind::Assign(op, target, value) => match &target.kind {
            ExprKind::Ident(name) => Some(Mutation::Assign { name, op: *op, value }),
            ExprKind::Field(..) | ExprKind::Index(..) => {
                let mut path = Vec::new();
                let name = path_of(target, &mut path)?;
                Some(Mutation::Put { name, path, op: *op, value })
            }
            _ => None,
        },
        ExprKind::Update { increment, target } => match &target.kind {
            ExprKind::Ident(name) => Some(Mutation::Update { name, increment: *increment }),
            _ => None,
        },
        ExprKind::Call(callee, args) => {
            let ExprKind::Field(receiver, method) = &callee.kind else {
                return None;
            };
            let ExprKind::Ident(name) = &receiver.kind else {
                return None;
            };
            match method.as_str() {
                "push" => Some(Mutation::Push { name, args }),
                "sort" | "reverse" => Some(Mutation::Reorder { name, method, args }),
                _ => None,
            }
        }
        _ => None,
    }
}

pub fn mutated_name(expr: &Expr) -> Option<&str> {
    match mutation(expr)? {
        Mutation::Assign { name, .. }
        | Mutation::Update { name, .. }
        | Mutation::Push { name, .. }
        | Mutation::Put { name, .. }
        | Mutation::Reorder { name, .. } => Some(name),
    }
}

struct Scopes {
    stack: Vec<HashSet<String>>,
    found: Vec<String>,
}

impl Scopes {
    fn declared(&self, name: &str) -> bool {
        self.stack.iter().any(|scope| scope.contains(name))
    }

    fn declare(&mut self, pattern: &Pattern) {
        let mut names = Vec::new();
        pattern_names(pattern, &mut names);
        if let Some(top) = self.stack.last_mut() {
            top.extend(names);
        }
    }

    fn assign(&mut self, name: &str) {
        if !self.declared(name) && !self.found.iter().any(|n| n == name) {
            self.found.push(name.to_string());
        }
    }

    fn stmts(&mut self, list: &[Stmt]) {
        self.stack.push(HashSet::new());
        for stmt in list {
            self.stmt(stmt);
        }
        self.stack.pop();
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Decl(_, decls) => {
                for decl in decls {
                    if let Some(init) = &decl.init {
                        self.expr(init);
                    }
                    self.declare(&decl.pattern);
                }
            }
            StmtKind::Expr(expr) => {
                match mutation(expr) {
                    Some(Mutation::Assign { name, value, .. }) => {
                        self.expr(value);
                        self.assign(name);
                    }
                    Some(Mutation::Update { name, .. }) => self.assign(name),
                    Some(Mutation::Push { name, args }) | Some(Mutation::Reorder { name, args, .. }) => {
                        args.iter().for_each(|a| self.expr(a));
                        self.assign(name);
                    }
                    Some(Mutation::Put { name, value, .. }) => {
                        self.expr(value);
                        self.assign(name);
                    }
                    None => self.expr(expr),
                }
            }
            StmtKind::If(condition, then, otherwise) => {
                self.expr(condition);
                self.stmts(std::slice::from_ref(then));
                if let Some(otherwise) = otherwise {
                    self.stmts(std::slice::from_ref(otherwise));
                }
            }
            StmtKind::ForOf { pattern, source, body } => {
                self.expr(source);
                self.stack.push(HashSet::new());
                self.declare(pattern);
                self.stmts(std::slice::from_ref(body));
                self.stack.pop();
            }
            StmtKind::For { init, test, update, body } => {
                self.stack.push(HashSet::new());
                if let Some(init) = init {
                    self.stmt(init);
                }
                if let Some(test) = test {
                    self.expr(test);
                }
                if let Some(update) = update {
                    self.expr(update);
                }
                self.stmts(std::slice::from_ref(body));
                self.stack.pop();
            }
            StmtKind::Try { block, param, handler } => {
                self.stmts(block);
                self.stack.push(HashSet::new());
                if let Some(param) = param {
                    self.declare(param);
                }
                self.stmts(handler);
                self.stack.pop();
            }
            StmtKind::Throw(expr) => self.expr(expr),
            StmtKind::Return(Some(expr)) => self.expr(expr),
            StmtKind::Block(body) => self.stmts(body),
            StmtKind::Function(function) => {
                if let Some(name) = &function.name
                    && let Some(top) = self.stack.last_mut()
                {
                    top.insert(name.name.clone());
                }
            }
            _ => {}
        }
    }

    fn expr(&mut self, _expr: &Expr) {}
}

pub fn assigned_outer(stmts: &[Stmt]) -> Vec<String> {
    let mut scopes = Scopes { stack: Vec::new(), found: Vec::new() };
    scopes.stmts(stmts);
    scopes.found
}

pub fn assigned_outer_stmt(stmt: &Stmt) -> Vec<String> {
    assigned_outer(std::slice::from_ref(stmt))
}

pub fn walk_expr<'a>(expr: &'a Expr, visit: &mut dyn FnMut(&'a Expr)) {
    visit(expr);
    match &expr.kind {
        ExprKind::Template(_, parts) => parts.iter().for_each(|e| walk_expr(e, visit)),
        ExprKind::Array(items) => items.iter().for_each(|item| match item {
            ArrayItem::Item(e) | ArrayItem::Spread(e) => walk_expr(e, visit),
        }),
        ExprKind::Object(items) => items.iter().for_each(|item| match item {
            ObjectItem::Field { value, .. } | ObjectItem::Spread(value) => walk_expr(value, visit),
        }),
        ExprKind::Field(base, _) => walk_expr(base, visit),
        ExprKind::Index(base, index) => {
            walk_expr(base, visit);
            walk_expr(index, visit);
        }
        ExprKind::Chain(head, links) => {
            walk_expr(head, visit);
            for link in links {
                match link {
                    Link::Access { access: Access::Index(index), .. } => walk_expr(index, visit),
                    Link::Call { args, .. } => args.iter().for_each(|e| walk_expr(e, visit)),
                    _ => {}
                }
            }
        }
        ExprKind::Call(callee, args) => {
            walk_expr(callee, visit);
            args.iter().for_each(|e| walk_expr(e, visit));
        }
        ExprKind::New(_, args) => args.iter().for_each(|e| walk_expr(e, visit)),
        ExprKind::Await(inner) | ExprKind::Unary(_, inner) | ExprKind::Cast(inner, _) => walk_expr(inner, visit),
        ExprKind::Arrow(function) => match &function.body {
            FunctionBody::Expr(body) => walk_expr(body, visit),
            FunctionBody::Block(body) => walk_stmts(body, visit),
        },
        ExprKind::Binary(_, left, right) | ExprKind::Assign(_, left, right) => {
            walk_expr(left, visit);
            walk_expr(right, visit);
        }
        ExprKind::Conditional(c, a, b) => {
            walk_expr(c, visit);
            walk_expr(a, visit);
            walk_expr(b, visit);
        }
        ExprKind::Update { target, .. } => walk_expr(target, visit),
        _ => {}
    }
}

pub fn walk_stmts<'a>(stmts: &'a [Stmt], visit: &mut dyn FnMut(&'a Expr)) {
    for stmt in stmts {
        walk_stmt(stmt, visit);
    }
}

pub fn walk_stmt<'a>(stmt: &'a Stmt, visit: &mut dyn FnMut(&'a Expr)) {
    match &stmt.kind {
        StmtKind::Decl(_, decls) => decls.iter().filter_map(|d| d.init.as_ref()).for_each(|e| walk_expr(e, visit)),
        StmtKind::Expr(e) | StmtKind::Throw(e) | StmtKind::Return(Some(e)) => walk_expr(e, visit),
        StmtKind::If(c, a, b) => {
            walk_expr(c, visit);
            walk_stmt(a, visit);
            if let Some(b) = b {
                walk_stmt(b, visit);
            }
        }
        StmtKind::ForOf { source, body, .. } => {
            walk_expr(source, visit);
            walk_stmt(body, visit);
        }
        StmtKind::For { init, test, update, body } => {
            if let Some(init) = init {
                walk_stmt(init, visit);
            }
            if let Some(test) = test {
                walk_expr(test, visit);
            }
            if let Some(update) = update {
                walk_expr(update, visit);
            }
            walk_stmt(body, visit);
        }
        StmtKind::Try { block, handler, .. } => {
            walk_stmts(block, visit);
            walk_stmts(handler, visit);
        }
        StmtKind::Block(body) => walk_stmts(body, visit),
        StmtKind::Function(function) => match &function.body {
            FunctionBody::Block(body) => walk_stmts(body, visit),
            FunctionBody::Expr(body) => walk_expr(body, visit),
        },
        _ => {}
    }
}

pub fn mentions(stmts: &[Stmt], name: &str) -> bool {
    let mut found = false;
    walk_stmts(stmts, &mut |e| {
        if matches!(&e.kind, ExprKind::Ident(n) if n == name) {
            found = true;
        }
    });
    found
}

pub fn expr_mentions(expr: &Expr, name: &str) -> bool {
    let mut found = false;
    walk_expr(expr, &mut |e| {
        if matches!(&e.kind, ExprKind::Ident(n) if n == name) {
            found = true;
        }
    });
    found
}

pub fn calls_user_function(stmts: &[Stmt], names: &HashSet<String>) -> bool {
    let mut found = false;
    walk_stmts(stmts, &mut |e| {
        if let ExprKind::Call(callee, _) = &e.kind
            && matches!(&callee.kind, ExprKind::Ident(n) if names.contains(n))
        {
            found = true;
        }
    });
    found
}

pub fn can_throw_plain(stmts: &[Stmt], throwing: &HashSet<String>) -> bool {
    let mut found = false;
    let mut visit = |e: &Expr| match &e.kind {
        ExprKind::Call(callee, _) => match &callee.kind {
            ExprKind::Ident(n) if throwing.contains(n) => found = true,
            ExprKind::Field(receiver, method)
                if method == "parse" && matches!(&receiver.kind, ExprKind::Ident(n) if n == "JSON") =>
            {
                found = true
            }
            _ => {}
        },
        _ => {}
    };
    walk_stmts(stmts, &mut visit);
    found || contains_throw(stmts)
}

pub fn contains_throw(stmts: &[Stmt]) -> bool {
    fn in_stmt(stmt: &Stmt) -> bool {
        match &stmt.kind {
            StmtKind::Throw(_) => true,
            StmtKind::If(_, a, b) => in_stmt(a) || b.as_deref().is_some_and(in_stmt),
            StmtKind::Block(body) => body.iter().any(in_stmt),
            StmtKind::ForOf { body, .. } | StmtKind::For { body, .. } => in_stmt(body),
            StmtKind::Try { block, handler, .. } => block.iter().chain(handler).any(in_stmt),
            _ => false,
        }
    }
    stmts.iter().any(in_stmt)
}

pub fn function_body_stmts(function: &Function) -> &[Stmt] {
    match &function.body {
        FunctionBody::Block(body) => body,
        FunctionBody::Expr(_) => &[],
    }
}

#[derive(Debug, PartialEq)]
pub enum Event {
    Throwing,
    Assignment(String, sx_kernel::syntax::Span),
}

pub fn try_events(stmts: &[Stmt], throwing: &HashSet<String>) -> Vec<Event> {
    let mut events = Vec::new();
    let mut scopes = Scopes { stack: vec![HashSet::new()], found: Vec::new() };
    events_in(stmts, throwing, &mut scopes, &mut events);
    events
}

fn expr_throws(expr: &Expr, throwing: &HashSet<String>) -> bool {
    let mut found = false;
    walk_expr(expr, &mut |e| match &e.kind {
        ExprKind::Call(callee, _) => match &callee.kind {
            ExprKind::Field(receiver, method) => {
                if matches!(&receiver.kind, ExprKind::Field(inner, _) if matches!(&inner.kind, ExprKind::Ident(n) if n == "tools")) {
                    found = true;
                }
                if method == "parse" && matches!(&receiver.kind, ExprKind::Ident(n) if n == "JSON") {
                    found = true;
                }
            }
            ExprKind::Ident(n) if throwing.contains(n) || matches!(n.as_str(), "retry" | "waitUntil" | "loop" | "parMap") => found = true,
            _ => {}
        },
        _ => {}
    });
    found
}

fn events_in(stmts: &[Stmt], throwing: &HashSet<String>, scopes: &mut Scopes, events: &mut Vec<Event>) {
    scopes.stack.push(HashSet::new());
    for stmt in stmts {
        match &stmt.kind {
            StmtKind::Decl(_, decls) => {
                for decl in decls {
                    if let Some(init) = &decl.init
                        && expr_throws(init, throwing)
                    {
                        events.push(Event::Throwing);
                    }
                    scopes.declare(&decl.pattern);
                }
            }
            StmtKind::Expr(expr) => {
                if expr_throws(expr, throwing) {
                    events.push(Event::Throwing);
                }
                if let Some(name) = mutated_name(expr)
                    && !scopes.declared(name)
                {
                    events.push(Event::Assignment(name.to_string(), expr.span));
                }
            }
            StmtKind::Throw(_) => events.push(Event::Throwing),
            StmtKind::Return(Some(expr)) if expr_throws(expr, throwing) => events.push(Event::Throwing),
            StmtKind::If(condition, then, otherwise) => {
                if expr_throws(condition, throwing) {
                    events.push(Event::Throwing);
                }
                events_in(std::slice::from_ref(then), throwing, scopes, events);
                if let Some(otherwise) = otherwise {
                    events_in(std::slice::from_ref(otherwise), throwing, scopes, events);
                }
            }
            StmtKind::Block(body) => events_in(body, throwing, scopes, events),
            StmtKind::ForOf { source, body, .. } => {
                if expr_throws(source, throwing) {
                    events.push(Event::Throwing);
                }
                events_in(std::slice::from_ref(body), throwing, scopes, events);
            }
            StmtKind::Try { block, handler, .. } => {
                events_in(block, throwing, scopes, events);
                events_in(handler, throwing, scopes, events);
            }
            _ => {}
        }
    }
    scopes.stack.pop();
}
