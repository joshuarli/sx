use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use sx_kernel::checker::closest;
use sx_kernel::diagnostics::Diagnostic;
use sx_kernel::registry::Registry;
use sx_kernel::syntax::{Node, Span};

use crate::analysis::*;
use crate::ast::*;
use crate::build::*;
use crate::diag;
use crate::parser::Program;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hint {
    Unknown,
    List,
    Tuple,
    Text,
    Map,
    Settled,
    SettledList,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatchKind {
    ToolOnly,
    Mixed { json_parse: bool },
}

#[derive(Clone, Debug)]
pub struct Var {
    pub sx: String,
    pub depth: usize,
    pub is_const: bool,
    pub owned: bool,
    pub uninit: bool,
    pub hint: Hint,
    pub catch: Option<CatchKind>,
    pub function: Option<bool>,
}

#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub done: Option<String>,
    pub brk: Option<String>,
    pub cont: Option<String>,
    pub ret: Option<String>,
    pub vars: Vec<String>,
}

#[derive(Clone, Debug)]
pub enum Frame {
    Function,
    LoopBuiltin(Vec<String>),
    Loop(Layout),
    Try(Layout),
}

pub enum Exit<'e> {
    Return(Option<&'e Expr>),
    ReturnValue(Node),
    Break,
    Continue,
}

pub type Scope = HashMap<String, Var>;
pub type Cont<'k, 'a> = &'k dyn Fn(&mut Lowerer<'a>) -> Node;

pub struct Lowerer<'a> {
    pub types: &'a BTreeMap<String, TypeAnn>,
    pub lines: Vec<&'a str>,
    pub tools: BTreeMap<String, String>,
    pub namespaces: BTreeSet<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub scopes: Vec<Scope>,
    pub frames: Vec<Frame>,
    pub fn_depth: usize,
    pub counter: usize,
    pub parallel: usize,
    pub promise_ok: bool,
    pub mutated: Vec<HashSet<String>>,
    pub throwing: HashSet<String>,
    pub defn_names: Vec<String>,
    pub param_hint: Option<Hint>,
}

pub fn camel(name: &str) -> String {
    let mut out = String::new();
    let mut upper = false;
    for c in name.trim_end_matches('?').chars() {
        if c == '-' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

const RESERVED: [&str; 62] = [
    "let", "if", "cond", "match", "when", "do", "fn", "defn", "def", "get", "call", "attempt", "retry", "wait-until", "sleep",
    "loop", "recur", "par", "par-map", "some", "none", "ok", "err", "unit", "true", "false", "map", "filter", "reduce",
    "flatten", "concat", "count", "nth", "first", "rest", "take", "sort-by", "identity", "str", "float", "floor", "ceil",
    "round", "rem", "quot", "any?", "all?", "contains?", "into-map", "keys", "vals", "entries", "enumerate", "range", "put",
    "fail", "log", "truthy", "typeof", "not", "and", "or",
];

impl<'a> Lowerer<'a> {
    pub fn new(source: &'a str, types: &'a BTreeMap<String, TypeAnn>, registry: &Registry) -> Lowerer<'a> {
        let mut tools = BTreeMap::new();
        let mut namespaces = BTreeSet::new();
        for capability in registry.capabilities() {
            let Some((namespace, name)) = capability.id.split_once('.') else {
                continue;
            };
            namespaces.insert(camel(namespace));
            tools.insert(format!("{}.{}", camel(namespace), camel(name)), capability.id.clone());
        }
        Lowerer {
            types,
            lines: source.split('\n').collect(),
            tools,
            namespaces,
            diagnostics: Vec::new(),
            scopes: vec![Scope::new()],
            frames: vec![Frame::Function],
            fn_depth: 0,
            counter: 0,
            parallel: 0,
            promise_ok: false,
            mutated: vec![HashSet::new()],
            throwing: HashSet::new(),
            defn_names: Vec::new(),
            param_hint: None,
        }
    }

    pub fn report(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub fn reject(&mut self, code: &'static str, span: Span, message: impl Into<String>) -> Node {
        self.report(diag(code, span, message));
        unit(span)
    }

    pub fn reject_fix(&mut self, code: &'static str, span: Span, message: impl Into<String>, fix: &str) -> Node {
        self.report(diag(code, span, message).fix(fix));
        unit(span)
    }

    pub fn source_text(&self, span: Span) -> String {
        let line = self.lines.get(span.start.line.saturating_sub(1)).copied().unwrap_or("");
        let end = if span.end.line == span.start.line { span.end.column - 1 } else { line.chars().count() };
        line.chars().skip(span.start.column - 1).take(end.saturating_sub(span.start.column - 1)).collect()
    }

    pub fn fresh(&mut self, base: &str) -> String {
        self.counter += 1;
        format!("{base}'{}", self.counter)
    }

    pub fn lookup(&self, name: &str) -> Option<&Var> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    pub fn lookup_mut(&mut self, name: &str) -> Option<&mut Var> {
        self.scopes.iter_mut().rev().find_map(|scope| scope.get_mut(name))
    }

    pub fn is_bound(&self, name: &str) -> bool {
        self.lookup(name).is_some()
    }

    pub fn declare(&mut self, name: &str, template: Var) -> String {
        let mut sx = if name == "_" { "_ignored".to_string() } else if RESERVED.contains(&name) { format!("{name}'") } else { name.to_string() };
        if self.is_bound(name) {
            sx = self.fresh(&sx);
        }
        let var = Var { sx: sx.clone(), ..template };
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), var);
        }
        sx
    }

    pub fn plain_var(&self) -> Var {
        Var {
            sx: String::new(),
            depth: self.fn_depth,
            is_const: false,
            owned: false,
            uninit: false,
            hint: Hint::Unknown,
            catch: None,
            function: None,
        }
    }

    pub fn with_scopes<T>(&mut self, scopes: Vec<Scope>, run: impl FnOnce(&mut Self) -> T) -> T {
        let inner = std::mem::replace(&mut self.scopes, scopes);
        let result = run(self);
        self.scopes = inner;
        result
    }

    pub fn sx_name(&self, name: &str) -> String {
        self.lookup(name).map(|v| v.sx.clone()).unwrap_or_else(|| name.to_string())
    }

    pub fn state_node(&mut self, vars: &[String], span: Span) -> Node {
        let mut nodes = Vec::new();
        for name in vars {
            let (sx, uninit) = match self.lookup(name) {
                Some(v) => (v.sx.clone(), v.uninit),
                None => (name.clone(), false),
            };
            if uninit {
                self.report(
                    diag("ts-uninitialized", span, "This name may be read before it is assigned").fix("give it a value"),
                );
            }
            nodes.push(sym(&sx, span));
        }
        match nodes.len() {
            0 => unit(span),
            1 => nodes.remove(0),
            _ => list(nodes, span),
        }
    }

    pub fn state_pattern(&self, vars: &[String], span: Span) -> Node {
        let mut nodes: Vec<Node> = vars.iter().map(|n| sym(&self.sx_name(n), span)).collect();
        match nodes.len() {
            0 => sym("_", span),
            1 => nodes.remove(0),
            _ => list(nodes, span),
        }
    }

    pub fn layout_pattern(&self, layout: &Layout, span: Span) -> Node {
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
    }

    pub fn layout_slots(&self, layout: &Layout) -> usize {
        [&layout.done, &layout.brk, &layout.cont, &layout.ret].iter().filter(|f| f.is_some()).count() + layout.vars.len()
    }

    pub fn layout_values(
        &mut self,
        layout: &Layout,
        flags: (bool, bool, bool),
        ret: Option<Node>,
        span: Span,
    ) -> Node {
        let mut nodes: Vec<Node> = Vec::new();
        if layout.done.is_some() {
            nodes.push(boolean(flags.0, span));
        }
        if layout.brk.is_some() {
            nodes.push(boolean(flags.1, span));
        }
        if layout.cont.is_some() {
            nodes.push(boolean(flags.2, span));
        }
        if layout.ret.is_some() {
            nodes.push(match ret {
                Some(value) => some(value, span),
                None => none(span),
            });
        }
        let vars = layout.vars.clone();
        let state = self.state_node(&vars, span);
        if layout.vars.len() == 1 {
            nodes.push(state);
        } else if !layout.vars.is_empty()
            && let sx_kernel::syntax::NodeKind::List(items) = state.kind
        {
            nodes.extend(items.into_vec());
        }
        match nodes.len() {
            0 => unit(span),
            1 => nodes.remove(0),
            _ => list(nodes, span),
        }
    }

    pub fn fall_off(&mut self, layout: &Layout, span: Span) -> Node {
        self.layout_values(layout, (false, false, false), None, span)
    }

    pub fn exit(&mut self, exit: Exit<'_>, span: Span) -> Node {
        let frame = self.frames.last().cloned().unwrap_or(Frame::Function);
        match frame {
            Frame::Function => match exit {
                Exit::Return(None) => unit(span),
                Exit::Return(Some(e)) => self.return_value(e),
                Exit::ReturnValue(node) => node,
                Exit::Break | Exit::Continue => {
                    self.reject("ts-unsupported", span, "break and continue are only supported inside for…of and counted for loops")
                }
            },
            Frame::LoopBuiltin(keys) => match exit {
                Exit::Return(Some(e)) => self.loop_step(e, &keys),
                Exit::Return(None) => self.reject("ts-unsupported", span, "A loop body must end in next(…) or done(…)"),
                Exit::ReturnValue(_) => self.reject("ts-unsupported", span, "next and done must be returned directly from the loop function"),
                Exit::Break | Exit::Continue => self.reject("ts-unsupported", span, "break and continue are not supported in a loop function"),
            },
            Frame::Loop(layout) | Frame::Try(layout) => {
                let in_loop = matches!(self.frames.last(), Some(Frame::Loop(_)));
                match exit {
                    Exit::Return(value) => {
                        let value = match value {
                            Some(e) => self.return_value(e),
                            None => unit(span),
                        };
                        self.layout_values(&layout, (true, false, false), Some(value), span)
                    }
                    Exit::ReturnValue(value) => self.layout_values(&layout, (true, false, false), Some(value), span),
                    Exit::Break => self.layout_values(&layout, (true, true, false), None, span),
                    Exit::Continue => {
                        let flags = if in_loop { (false, false, false) } else { (false, false, true) };
                        self.layout_values(&layout, flags, None, span)
                    }
                }
            }
        }
    }

    pub fn return_value(&mut self, e: &Expr) -> Node {
        self.value_of(e, true)
    }

    pub fn closest_tools(&self, name: &str) -> Option<String> {
        let candidates: Vec<&String> = self.tools.keys().collect();
        closest(name, &candidates)
    }
}

pub fn lower(program: &Program, source: &str, registry: &Registry) -> Result<Node, Vec<Diagnostic>> {
    let mut lowerer = Lowerer::new(source, &program.types, registry);
    lowerer.diagnostics.extend(program.diagnostics.iter().cloned());
    lowerer.compute_throwing(&program.body);
    lowerer.mutated = vec![all_mutated(&program.body)];
    let (defns, main) = lowerer.body(&program.body, &|l: &mut Lowerer<'_>| unit(l.end_span()), true);
    if !lowerer.diagnostics.is_empty() {
        let mut diagnostics = lowerer.diagnostics;
        diagnostics.sort_by_key(|d| (d.span.start.line, d.span.start.column));
        diagnostics.dedup_by(|a, b| a.code == b.code && a.span == b.span && a.message == b.message);
        return Err(diagnostics);
    }
    let mut forms = defns;
    forms.push(main);
    sx_kernel::definitions::assemble(forms).map_err(|mut diagnostics| {
        diagnostics.sort_by_key(|d| (d.span.start.line, d.span.start.column));
        diagnostics
    })
}

pub fn all_mutated(stmts: &[Stmt]) -> HashSet<String> {
    fn visit(stmt: &Stmt, out: &mut HashSet<String>) {
        match &stmt.kind {
            StmtKind::Expr(e) => {
                if let Some(name) = mutated_name(e) {
                    out.insert(name.to_string());
                }
            }
            StmtKind::If(_, a, b) => {
                visit(a, out);
                if let Some(b) = b {
                    visit(b, out);
                }
            }
            StmtKind::ForOf { body, .. } | StmtKind::For { body, .. } => visit(body, out),
            StmtKind::Try { block, handler, .. } => block.iter().chain(handler).for_each(|s| visit(s, out)),
            StmtKind::Block(body) => body.iter().for_each(|s| visit(s, out)),
            _ => {}
        }
    }
    let mut out = HashSet::new();
    stmts.iter().for_each(|s| visit(s, &mut out));
    out
}

pub fn definitely_assigns(stmts: &[Stmt], name: &str) -> bool {
    stmts.iter().any(|stmt| stmt_assigns(stmt, name))
}

fn stmt_assigns(stmt: &Stmt, name: &str) -> bool {
    match &stmt.kind {
        StmtKind::Expr(e) => matches!(mutation(e), Some(Mutation::Assign { name: n, op: AssignOp::Set, .. }) if n == name),
        StmtKind::If(_, a, Some(b)) => stmt_assigns(a, name) && stmt_assigns(b, name),
        StmtKind::Block(body) => definitely_assigns(body, name),
        StmtKind::Try { block, handler, .. } => definitely_assigns(block, name) && definitely_assigns(handler, name),
        _ => false,
    }
}
