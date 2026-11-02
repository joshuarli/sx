use crate::checker::{is_plain_name, is_special_form};
use crate::diagnostics::Diagnostic;
use crate::syntax::{Node, NodeKind, Span};

pub struct Definition {
    pub is_function: bool,
    pub name: Node,
    pub value: Node,
    defined: Vec<String>,
    uses: Vec<Use>,
}

struct Use {
    name: String,
    span: Span,
    form_name: bool,
}

#[derive(Clone)]
enum Reason {
    Uses(Span),
    RunsAfter,
    Redefines(String),
}

#[derive(Clone)]
struct Dependency {
    on: usize,
    reason: Reason,
}

#[derive(Clone, Copy, PartialEq)]
enum State {
    Unvisited,
    Visiting,
    Done,
}

impl Definition {
    pub fn new(is_function: bool, name: Node, value: Node) -> Definition {
        let mut defined = Vec::new();
        binder_names(&name, false, &mut defined);
        let mut collector = Collector::default();
        collector.node(&value);
        Definition { is_function, name, value, defined, uses: collector.found }
    }

    fn label(&self) -> String {
        match &self.name.kind {
            NodeKind::Symbol(name) if name != "_" => name.clone(),
            _ => format!("the definition at line {}", self.name.span.start.line),
        }
    }
}

fn binder_names(pattern: &Node, in_match: bool, out: &mut Vec<String>) {
    match &pattern.kind {
        NodeKind::Symbol(name) if name == "_" || name == "none" => {}
        NodeKind::Symbol(name) if !in_match || is_plain_name(name) => out.push(name.clone()),
        NodeKind::Form(items) => items.iter().skip(1).for_each(|item| binder_names(item, in_match, out)),
        NodeKind::List(items) => items.iter().for_each(|item| binder_names(item, in_match, out)),
        _ => {}
    }
}

#[derive(Default)]
struct Collector {
    bound: Vec<String>,
    found: Vec<Use>,
}

impl Collector {
    fn node(&mut self, node: &Node) {
        match &node.kind {
            NodeKind::Symbol(name) => self.read(name, node.span, false),
            NodeKind::List(items) => items.iter().for_each(|item| self.node(item)),
            NodeKind::Record(fields) => fields.iter().for_each(|field| self.node(&field.value)),
            NodeKind::Form(items) => self.form(items),
            _ => {}
        }
    }

    fn read(&mut self, name: &str, span: Span, form_name: bool) {
        if !self.bound.iter().any(|bound| bound == name) {
            self.found.push(Use { name: name.to_string(), span, form_name });
        }
    }

    fn bind(&mut self, pattern: &Node, in_match: bool) {
        binder_names(pattern, in_match, &mut self.bound);
    }

    fn form(&mut self, items: &[Node]) {
        let mark = self.bound.len();
        let name = items.first().and_then(|head| match &head.kind {
            NodeKind::Symbol(name) => Some(name.as_str()),
            _ => None,
        });
        match name {
            Some("let") => {
                if let Some(NodeKind::List(bindings)) = items.get(1).map(|n| &n.kind) {
                    for pair in bindings.chunks(2) {
                        if let Some(value) = pair.get(1) {
                            self.node(value);
                        }
                        self.bind(&pair[0], false);
                    }
                }
                items.iter().skip(2).for_each(|item| self.node(item));
            }
            Some("fn") => {
                if let Some(params) = items.get(1) {
                    self.bind(params, false);
                }
                items.iter().skip(2).for_each(|item| self.node(item));
            }
            Some("loop") => {
                if let Some(options) = items.get(1) {
                    self.node(options);
                }
                if let Some(NodeKind::List(bindings)) = items.get(2).map(|n| &n.kind) {
                    let mut names = Vec::new();
                    for pair in bindings.chunks(2) {
                        if let Some(value) = pair.get(1) {
                            self.node(value);
                        }
                        binder_names(&pair[0], false, &mut names);
                    }
                    self.bound.extend(names);
                }
                items.iter().skip(3).for_each(|item| self.node(item));
            }
            Some("match") => {
                if let Some(subject) = items.get(1) {
                    self.node(subject);
                }
                for clause in items.iter().skip(2).collect::<Vec<_>>().chunks(2) {
                    let inner = self.bound.len();
                    self.bind(clause[0], true);
                    if let Some(body) = clause.get(1) {
                        self.node(body);
                    }
                    self.bound.truncate(inner);
                }
            }
            Some(name) => {
                if matches!(name, "when" | "do") {
                    self.read(name, items[0].span, true);
                } else if !is_special_form(name) {
                    self.read(name, items[0].span, false);
                }
                items.iter().skip(1).for_each(|item| self.node(item));
            }
            None => items.iter().for_each(|item| self.node(item)),
        }
        self.bound.truncate(mark);
    }
}

fn resolve(definitions: &[Definition], user: usize, used: &Use) -> Option<usize> {
    let names = |at: &usize| {
        let target = &definitions[*at];
        target.defined.contains(&used.name) && (!used.form_name || target.is_function)
    };
    (0..user).rev().find(names).or_else(|| {
        (user + 1..definitions.len())
            .find(names)
            .filter(|at| definitions[user].is_function || definitions[*at].is_function)
    })
}

fn dependencies(definitions: &[Definition]) -> Vec<Vec<Dependency>> {
    let mut dependencies = vec![Vec::new(); definitions.len()];
    let mut previous_value: Option<usize> = None;
    for (user, definition) in definitions.iter().enumerate() {
        if !definition.is_function {
            if let Some(before) = previous_value {
                dependencies[user].push(Dependency { on: before, reason: Reason::RunsAfter });
            }
            previous_value = Some(user);
        }
        for used in &definition.uses {
            let Some(target) = resolve(definitions, user, used) else {
                continue;
            };
            if target != user {
                dependencies[user].push(Dependency { on: target, reason: Reason::Uses(used.span) });
            }
            for (other, candidate) in definitions.iter().enumerate() {
                if other > target && other != user && candidate.defined.contains(&used.name) {
                    dependencies[other].push(Dependency { on: user, reason: Reason::Redefines(used.name.clone()) });
                }
            }
        }
    }
    for list in &mut dependencies {
        list.sort_by_key(|dependency| dependency.on);
    }
    dependencies
}

struct Walk<'a> {
    definitions: &'a [Definition],
    dependencies: Vec<Vec<Dependency>>,
    state: Vec<State>,
    path: Vec<(usize, Option<Reason>)>,
    order: Vec<usize>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl Walk<'_> {
    fn visit(&mut self, at: usize) {
        self.state[at] = State::Visiting;
        self.path.push((at, None));
        for dependency in self.dependencies[at].clone() {
            match self.state[dependency.on] {
                State::Done => {}
                State::Visiting => self.cycle(at, &dependency),
                State::Unvisited => {
                    if let Some(last) = self.path.last_mut() {
                        last.1 = Some(dependency.reason.clone());
                    }
                    self.visit(dependency.on);
                }
            }
        }
        self.path.pop();
        self.state[at] = State::Done;
        self.order.push(at);
    }

    fn cycle(&mut self, from: usize, closing: &Dependency) {
        let Some(start) = self.path.iter().position(|(at, _)| *at == closing.on) else {
            return;
        };
        let mut steps: Vec<(usize, Reason)> = self.path[start..]
            .iter()
            .filter_map(|(at, reason)| reason.clone().map(|reason| (*at, reason)))
            .collect();
        steps.push((from, closing.reason.clone()));
        let nodes: Vec<usize> = self.path[start..].iter().map(|(at, _)| *at).collect();
        let label = |at: usize| self.definitions[at].label();
        let described: Vec<String> = steps
            .iter()
            .enumerate()
            .map(|(index, (at, reason))| {
                let next = nodes[(index + 1) % nodes.len()];
                match reason {
                    Reason::Uses(_) => format!("{} uses {}", label(*at), label(next)),
                    Reason::RunsAfter => {
                        format!("{} runs after {}, because definitions that are not functions run in source order", label(*at), label(next))
                    }
                    Reason::Redefines(name) => format!(
                        "{} defines {name} again, so it must come after {}, which uses the first {name}",
                        label(*at),
                        label(next)
                    ),
                }
            })
            .collect();
        let names: Vec<String> = nodes.iter().map(|at| label(*at)).collect();
        let only_calls = nodes.iter().all(|at| self.definitions[*at].is_function)
            && steps.iter().all(|(_, reason)| matches!(reason, Reason::Uses(_)));
        let span = match &closing.reason {
            Reason::Uses(span) => *span,
            _ => self.definitions[from].name.span,
        };
        let diagnostic = if only_calls {
            Diagnostic::new(
                "unknown-function",
                span,
                format!("{} call each other, and Sx has no recursion.", join(&names)),
            )
            .detail(format!(
                "{}.\nRepeat with loop and :max, or map over a list that already exists.",
                described.join(", ")
            ))
            .help("loop")
        } else {
            Diagnostic::new(
                "unknown-name",
                span,
                format!("{} need each other, so none of them can come first.", join(&names)),
            )
            .detail(format!(
                "{}.\nDefine the value before the code that runs the function, or pass it to the function as an argument.",
                described.join(", ")
            ))
            .help("defn")
        };
        self.diagnostics.push(diagnostic);
    }
}

fn join(names: &[String]) -> String {
    match names {
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [init @ .., last] => format!("{}, and {last}", init.join(", ")),
        [] => String::new(),
    }
}

pub fn order(definitions: Vec<Definition>, diagnostics: &mut Vec<Diagnostic>) -> Vec<Definition> {
    let dependencies = dependencies(&definitions);
    let count = definitions.len();
    let mut walk = Walk {
        definitions: &definitions,
        dependencies,
        state: vec![State::Unvisited; count],
        path: Vec::new(),
        order: Vec::with_capacity(count),
        diagnostics,
    };
    for at in 0..count {
        if walk.state[at] == State::Unvisited {
            walk.visit(at);
        }
    }
    let order = walk.order;
    let mut slots: Vec<Option<Definition>> = definitions.into_iter().map(Some).collect();
    order.into_iter().filter_map(|at| slots[at].take()).collect()
}
