use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use crate::budgets::Budgets;
use crate::ir::{self, Binder, Conversion, HigherOrder, Lambda, MatchPattern, Node, NodeKind, PreludeOp, Program, WrapKind};
use crate::json::{Json, repr_str};
use crate::syntax::Span;
use crate::types::{CallErrorKind, Type};
use crate::values::{
    MapValue, RecordValue, Value, call_error_value, decode, encode, float_text, show_value, size,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    CallFailed,
    InvalidHostResponse,
    BudgetExhausted,
    Arithmetic,
    OutOfRange,
    LoopLimit,
    Internal,
}

impl FailureKind {
    pub fn name(self) -> &'static str {
        match self {
            FailureKind::CallFailed => "call-failed",
            FailureKind::InvalidHostResponse => "invalid-host-response",
            FailureKind::BudgetExhausted => "budget-exhausted",
            FailureKind::Arithmetic => "arithmetic",
            FailureKind::OutOfRange => "out-of-range",
            FailureKind::LoopLimit => "loop-limit",
            FailureKind::Internal => "internal",
        }
    }
}

impl fmt::Display for FailureKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub kind: FailureKind,
    pub message: String,
    pub span: Option<Span>,
    pub request: Option<String>,
}

impl Failure {
    fn new(kind: FailureKind, message: impl Into<String>, span: Option<Span>) -> Failure {
        Failure { kind, message: message.into(), span, request: None }
    }
}

fn internal(message: impl Into<String>, span: Option<Span>) -> Failure {
    Failure::new(FailureKind::Internal, message, span)
}

fn invalid(message: impl Into<String>, request: &str) -> Failure {
    Failure { kind: FailureKind::InvalidHostResponse, message: message.into(), span: None, request: Some(request.to_string()) }
}

fn budget(message: impl Into<String>, span: Option<Span>) -> Failure {
    Failure::new(FailureKind::BudgetExhausted, message, span)
}

type Step<T> = Result<T, Failure>;

#[derive(Clone, Debug)]
pub enum Request {
    Call { id: String, capability: String, input: Json },
    Delay { id: String, milliseconds: i64 },
}

impl Request {
    pub fn id(&self) -> &str {
        match self {
            Request::Call { id, .. } | Request::Delay { id, .. } => id,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Response {
    Output(Json),
    Refusal { kind: String, message: String },
}

#[derive(Clone, Debug)]
pub enum Outcome {
    Completed { value: Value, json: Json },
    Suspended(Vec<Request>),
    Failed(Failure),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoreError {
    Unimplemented,
    Failed(String),
    Arithmetic(String),
}

pub trait CoreImplementations {
    fn apply(&self, function: &str, args: &[Value]) -> Result<Value, CoreError>;
}

pub struct NoCores;

impl CoreImplementations for NoCores {
    fn apply(&self, _function: &str, _args: &[Value]) -> Result<Value, CoreError> {
        Err(CoreError::Unimplemented)
    }
}

#[derive(Clone)]
enum Slot<'p> {
    Value(Value),
    Local(Env<'p>),
}

struct EnvNode<'p> {
    name: &'p str,
    slot: Slot<'p>,
    parent: Env<'p>,
}

type Env<'p> = Option<Rc<EnvNode<'p>>>;

fn extend<'p>(env: Env<'p>, name: &'p str, slot: Slot<'p>) -> Env<'p> {
    Some(Rc::new(EnvNode { name, slot, parent: env }))
}

fn lookup<'p>(env: &Env<'p>, name: &str) -> Step<Slot<'p>> {
    let mut current = env;
    while let Some(node) = current {
        if node.name == name {
            return Ok(node.slot.clone());
        }
        current = &node.parent;
    }
    Err(internal(format!("{name} is not bound."), None))
}

#[derive(Clone)]
struct Closure<'p> {
    params: &'p [Binder],
    body: &'p Node,
    env: Env<'p>,
}

enum Returned<'p> {
    Value(Value),
    Closure(Closure<'p>),
    Recurring(Vec<Value>),
}

fn plain(returned: Returned<'_>) -> Step<Value> {
    match returned {
        Returned::Value(value) => Ok(value),
        _ => Err(internal("A function or recur reached a value position.", None)),
    }
}

#[derive(Clone)]
struct Raised {
    error: RecordValue,
    span: Span,
}

#[derive(Clone, Copy)]
enum Child<'p> {
    Node(&'p Node),
    Lambda(&'p Lambda),
}

struct WaitState<'p> {
    node: &'p ir::WaitUntil,
    span: Span,
    env: Env<'p>,
    round: usize,
}

struct EachFrame<'p> {
    op: HigherOrder,
    closure: Closure<'p>,
    items: Arc<[Value]>,
    span: Span,
    accumulator: Option<Value>,
    index: usize,
    results: Vec<Value>,
}

type JoinId = u64;

struct JoinFrame<'p> {
    id: JoinId,
    span: Span,
    count: usize,
    as_tuple: bool,
    closure: Option<Closure<'p>>,
    items: Arc<[Value]>,
    limit: usize,
    started: usize,
    results: BTreeMap<usize, Value>,
    failures: BTreeMap<usize, RecordValue>,
    children: Vec<FiberId>,
}

enum Frame<'p> {
    Seq { origin: &'p Node, children: Vec<Child<'p>>, env: Env<'p>, values: Vec<Returned<'p>> },
    Let { node: &'p ir::Let, env: Env<'p> },
    If { node: &'p ir::If, env: Env<'p> },
    Match { node: &'p ir::Match, span: Span, env: Env<'p> },
    Attempt,
    Retry { node: &'p ir::Retry, span: Span, env: Env<'p>, attempt: usize },
    Wait(WaitState<'p>),
    WaitCheck { wait: WaitState<'p>, value: Value },
    Loop { node: &'p ir::Loop, span: Span, env: Env<'p>, iteration: i64 },
    Each(EachFrame<'p>),
    ThenEval { node: &'p Node, env: Env<'p> },
    Join(JoinFrame<'p>),
}

type FiberId = usize;
type RequestOrder = (Vec<usize>, u64);

struct Waiting<'p> {
    request: Request,
    order: RequestOrder,
    output: Option<&'p Type>,
}

struct Fiber<'p> {
    path: Vec<usize>,
    parent: Option<(FiberId, JoinId, usize)>,
    stack: Vec<Frame<'p>>,
    node: Option<(Child<'p>, Env<'p>)>,
    returning: Option<Returned<'p>>,
    raising: Option<Raised>,
    waiting: Option<Waiting<'p>>,
    call_span: Option<Span>,
    joining: bool,
    issued: bool,
    done: bool,
    cancelled: bool,
    result: Option<Value>,
    failure: Option<Raised>,
}

impl<'p> Fiber<'p> {
    fn new(path: Vec<usize>, parent: Option<(FiberId, JoinId, usize)>) -> Fiber<'p> {
        Fiber {
            path,
            parent,
            stack: Vec::new(),
            node: None,
            returning: None,
            raising: None,
            waiting: None,
            call_span: None,
            joining: false,
            issued: false,
            done: false,
            cancelled: false,
            result: None,
            failure: None,
        }
    }

    fn runnable(&self) -> bool {
        if self.done || self.cancelled || self.waiting.is_some() || self.joining {
            return false;
        }
        self.node.is_some() || self.returning.is_some() || self.raising.is_some()
    }
}

pub struct Machine<'p> {
    program: &'p Program,
    budgets: Budgets,
    cores: &'p dyn CoreImplementations,
    fibers: Vec<Fiber<'p>>,
    live: Vec<FiberId>,
    issued: HashMap<String, FiberId>,
    sequences: HashMap<Vec<usize>, u64>,
    trace: Vec<Json>,
    steps: u64,
    calls: u64,
    applications: u64,
    delay: u64,
    joins: JoinId,
    finished: Option<Outcome>,
}

const ROOT: FiberId = 0;

impl<'p> Machine<'p> {
    pub fn new(
        program: &'p Program,
        inputs: &[(String, Value)],
        budgets: Budgets,
        cores: &'p dyn CoreImplementations,
    ) -> Machine<'p> {
        let mut env: Env<'p> = None;
        for (name, value) in inputs {
            if let Some((declared, _)) = program.inputs.get_key_value(name) {
                env = extend(env, declared.as_str(), Slot::Value(value.clone()));
            }
        }
        let mut root = Fiber::new(Vec::new(), None);
        root.node = Some((Child::Node(&program.body), env));
        Machine {
            program,
            budgets,
            cores,
            fibers: vec![root],
            live: vec![ROOT],
            issued: HashMap::new(),
            sequences: HashMap::new(),
            trace: Vec::new(),
            steps: 0,
            calls: 0,
            applications: 0,
            delay: 0,
            joins: 0,
            finished: None,
        }
    }

    pub fn trace(&self) -> &[Json] {
        &self.trace
    }

    pub fn start(&mut self) -> Outcome {
        self.run()
    }

    fn next_id(&mut self, id: FiberId) -> (String, RequestOrder) {
        let path = self.fibers[id].path.clone();
        let sequence = self.sequences.entry(path.clone()).or_insert(0);
        *sequence += 1;
        let text: String = path.iter().map(|index| format!(".{index}")).collect();
        (format!("r{text}:{sequence}"), (path, *sequence))
    }

    pub fn resume(&mut self, responses: Vec<(String, Response)>) -> Outcome {
        if let Some(finished) = &self.finished {
            return finished.clone();
        }
        let mut seen: HashSet<&str> = HashSet::new();
        for (request_id, _) in &responses {
            if seen.contains(request_id.as_str()) {
                return self.fail(invalid(format!("The host answered request {request_id} twice."), request_id));
            }
            if !self.issued.contains_key(request_id) {
                let message = format!("The host answered {request_id}, which is not an outstanding request.");
                return self.fail(invalid(message, request_id));
            }
            seen.insert(request_id);
        }
        let mut missing: Vec<(RequestOrder, String)> = self
            .issued
            .iter()
            .filter(|(request_id, _)| !seen.contains(request_id.as_str()))
            .map(|(request_id, fiber)| (self.order_of(*fiber), request_id.clone()))
            .collect();
        missing.sort();
        if let Some((_, first)) = missing.first() {
            let message = format!("The host did not answer request {first}.");
            let first = first.clone();
            return self.fail(invalid(message, &first));
        }
        let mut ordered: Vec<(RequestOrder, String, Response)> = responses
            .into_iter()
            .map(|(request_id, response)| {
                let order = self.issued.get(&request_id).map(|fiber| self.order_of(*fiber)).unwrap_or_default();
                (order, request_id, response)
            })
            .collect();
        ordered.sort_by(|a, b| a.0.cmp(&b.0));
        for (_, request_id, response) in ordered {
            let Some(fiber) = self.issued.remove(&request_id) else {
                return self.fail(internal(format!("{request_id} vanished from the issued requests."), None));
            };
            self.trace.push(response_trace(&request_id, &response));
            if !self.fibers[fiber].cancelled
                && let Err(failure) = self.deliver(fiber, response)
            {
                return self.fail(failure);
            }
        }
        self.run()
    }

    fn order_of(&self, fiber: FiberId) -> RequestOrder {
        self.fibers[fiber].waiting.as_ref().map(|w| w.order.clone()).unwrap_or_default()
    }

    fn fail(&mut self, failure: Failure) -> Outcome {
        let outcome = Outcome::Failed(failure);
        self.finished = Some(outcome.clone());
        outcome
    }

    fn deliver(&mut self, id: FiberId, response: Response) -> Step<()> {
        let fiber = &mut self.fibers[id];
        let Some(waiting) = fiber.waiting.take() else {
            return Err(internal("A response reached a fiber that waits for nothing.", None));
        };
        fiber.issued = false;
        match waiting.request {
            Request::Delay { id: request_id, .. } => {
                if !matches!(response, Response::Output(Json::Null)) {
                    return Err(invalid(format!("Delay request {request_id} must be answered with null."), &request_id));
                }
                fiber.returning = Some(Returned::Value(Value::Unit));
                Ok(())
            }
            Request::Call { id: request_id, capability, .. } => {
                let span = fiber.call_span.unwrap_or(self.program.body.span);
                match response {
                    Response::Refusal { kind, message } => {
                        let Some(kind) = CallErrorKind::from_name(&kind) else {
                            let message = format!("{request_id}: {} is not a call error kind.", repr_str(&kind));
                            return Err(invalid(message, &request_id));
                        };
                        fiber.raising = Some(Raised { error: call_error_value(kind, &message), span });
                        Ok(())
                    }
                    Response::Output(output) => {
                        let Some(output_type) = waiting.output else {
                            return Err(internal("A call request has no output type.", None));
                        };
                        let value = decode(&output, output_type, "$")
                            .map_err(|error| invalid(format!("{request_id} ({capability}): {error}"), &request_id))?;
                        self.check_size(&value, Some(span))?;
                        self.fibers[id].returning = Some(Returned::Value(value));
                        Ok(())
                    }
                }
            }
        }
    }

    fn run(&mut self) -> Outcome {
        if let Some(finished) = &self.finished {
            return finished.clone();
        }
        match self.run_rounds() {
            Ok(outcome) => {
                if !matches!(outcome, Outcome::Suspended(_)) {
                    self.finished = Some(outcome.clone());
                }
                outcome
            }
            Err(failure) => self.fail(failure),
        }
    }

    fn run_rounds(&mut self) -> Step<Outcome> {
        loop {
            let mut order = self.live.clone();
            order.sort_by(|a, b| self.fibers[*a].path.cmp(&self.fibers[*b].path));
            for id in order {
                while self.fibers[id].runnable() {
                    self.step(id)?;
                }
            }
            self.settle_joins()?;
            if self.fibers[ROOT].done {
                return self.complete();
            }
            if self.live.iter().any(|id| self.fibers[*id].runnable()) {
                continue;
            }
            let requests = self.issue();
            if !requests.is_empty() {
                return Ok(Outcome::Suspended(requests));
            }
            return Err(internal("No fiber can run and no request is outstanding.", None));
        }
    }

    fn complete(&mut self) -> Step<Outcome> {
        let root = &self.fibers[ROOT];
        if let Some(failure) = &root.failure {
            let kind = match failure.error.get("kind") {
                Some(Value::Variant(variant)) => variant.variant.to_string(),
                _ => return Err(internal("A failure carries no call error kind.", None)),
            };
            let message = match failure.error.get("message") {
                Some(Value::String(text)) => text.to_string(),
                _ => return Err(internal("A failure carries no message.", None)),
            };
            let text = format!("A tool call failed with {kind}: {message}");
            return Err(Failure::new(FailureKind::CallFailed, text, Some(failure.span)));
        }
        let Some(result) = root.result.clone() else {
            return Err(internal("The program finished without a result.", None));
        };
        self.check_size(&result, Some(self.program.body.span))?;
        let json = encode(&result, &self.program.result);
        Ok(Outcome::Completed { value: result, json })
    }

    fn issue(&mut self) -> Vec<Request> {
        let mut waiting: Vec<FiberId> = self
            .live
            .iter()
            .copied()
            .filter(|id| {
                let fiber = &self.fibers[*id];
                fiber.waiting.is_some() && !fiber.issued && !fiber.cancelled
            })
            .collect();
        waiting.sort_by_key(|id| self.order_of(*id));
        let limit = usize::try_from(self.budgets.concurrency).unwrap_or(usize::MAX);
        let mut requests = Vec::new();
        for id in waiting.into_iter().take(limit) {
            let fiber = &mut self.fibers[id];
            fiber.issued = true;
            let Some(waiting) = &fiber.waiting else {
                continue;
            };
            let request = waiting.request.clone();
            self.issued.insert(request.id().to_string(), id);
            self.trace.push(match &request {
                Request::Call { id, capability, input } => Json::object([
                    ("request", Json::string(id.as_str())),
                    ("capability", Json::string(capability.as_str())),
                    ("input", input.clone()),
                ]),
                Request::Delay { id, milliseconds } => {
                    Json::object([("delay", Json::string(id.as_str())), ("milliseconds", Json::Int(*milliseconds))])
                }
            });
            requests.push(request);
        }
        requests
    }

    fn top_join(&mut self, id: FiberId) -> Step<&mut JoinFrame<'p>> {
        match self.fibers[id].stack.last_mut() {
            Some(Frame::Join(join)) => Ok(join),
            _ => Err(internal("A joining fiber has no join frame.", None)),
        }
    }

    fn settle_joins(&mut self) -> Step<()> {
        let snapshot = self.live.clone();
        for id in snapshot {
            if !self.fibers[id].joining {
                continue;
            }
            let join = self.top_join(id)?;
            if let Some((_, first)) = join.failures.iter().next() {
                let first = first.clone();
                let span = join.span;
                let children = join.children.clone();
                for child in children {
                    self.cancel(child);
                }
                self.close_join(id);
                self.fibers[id].raising = Some(Raised { error: first, span });
                continue;
            }
            if join.closure.is_some() {
                self.start_children(id)?;
            }
            let join = self.top_join(id)?;
            if join.results.len() == join.count {
                let ordered: Vec<Value> = join.results.values().cloned().collect();
                let as_tuple = join.as_tuple;
                self.close_join(id);
                let value = if as_tuple { Value::tuple(ordered) } else { Value::list(ordered) };
                self.fibers[id].returning = Some(Returned::Value(value));
            }
        }
        Ok(())
    }

    fn close_join(&mut self, id: FiberId) {
        let fiber = &mut self.fibers[id];
        fiber.joining = false;
        let Some(Frame::Join(join)) = fiber.stack.pop() else {
            return;
        };
        let finished: HashSet<FiberId> = join.children.into_iter().collect();
        let fibers = &self.fibers;
        self.live.retain(|f| !finished.contains(f) || fibers[*f].issued);
    }

    fn cancel(&mut self, id: FiberId) {
        let mut pending = vec![id];
        while let Some(current) = pending.pop() {
            let fiber = &mut self.fibers[current];
            fiber.cancelled = true;
            for frame in &fiber.stack {
                if let Frame::Join(join) = frame {
                    pending.extend(join.children.iter().rev());
                }
            }
        }
    }

    fn start_children(&mut self, id: FiberId) -> Step<()> {
        let join = self.top_join(id)?;
        let mut active = 0;
        let children = join.children.clone();
        for child in &children {
            if !self.fibers[*child].done {
                active += 1;
            }
        }
        loop {
            let join = self.top_join(id)?;
            if !(join.started < join.count && active < join.limit) {
                return Ok(());
            }
            let index = join.started;
            let join_id = join.id;
            let span = join.span;
            let item = join.items[index].clone();
            let Some(closure) = join.closure.clone() else {
                return Err(internal("A par-map join has no function.", None));
            };
            let mut path = self.fibers[id].path.clone();
            path.push(index);
            let child = self.fibers.len();
            self.fibers.push(Fiber::new(path, Some((id, join_id, index))));
            self.apply(child, &closure, vec![item], span)?;
            let join = self.top_join(id)?;
            join.children.push(child);
            join.started += 1;
            self.live.push(child);
            active += 1;
        }
    }

    fn parent_join(&mut self, id: FiberId) -> Option<(&mut JoinFrame<'p>, usize)> {
        let (parent, join_id, index) = self.fibers[id].parent?;
        match self.fibers[parent].stack.last_mut() {
            Some(Frame::Join(join)) if join.id == join_id => Some((join, index)),
            _ => None,
        }
    }

    fn finish(&mut self, id: FiberId, value: Value) {
        let fiber = &mut self.fibers[id];
        fiber.done = true;
        fiber.result = Some(value.clone());
        if let Some((join, index)) = self.parent_join(id) {
            join.results.insert(index, value);
        }
    }

    fn fail_fiber(&mut self, id: FiberId, raised: Raised) {
        let error = raised.error.clone();
        let fiber = &mut self.fibers[id];
        fiber.done = true;
        fiber.failure = Some(raised);
        if let Some((join, index)) = self.parent_join(id) {
            join.failures.insert(index, error);
        }
    }

    fn step(&mut self, id: FiberId) -> Step<()> {
        self.steps += 1;
        if self.steps > self.budgets.steps {
            return Err(budget(format!("The program ran more than {} steps (steps budget).", self.budgets.steps), None));
        }
        let fiber = &mut self.fibers[id];
        if fiber.raising.is_some() {
            return self.unwind(id);
        }
        if let Some((child, env)) = fiber.node.take() {
            return self.evaluate(id, child, env);
        }
        let Some(value) = fiber.returning.take() else {
            return Err(internal("A runnable fiber has nothing to do.", None));
        };
        let Some(frame) = fiber.stack.pop() else {
            return match value {
                Returned::Value(value) => {
                    self.finish(id, value);
                    Ok(())
                }
                _ => Err(internal("A fiber ended with a function or a recur.", None)),
            };
        };
        self.resume_frame(id, frame, value)
    }

    fn check_size(&self, value: &Value, span: Option<Span>) -> Step<()> {
        if size(value) > self.budgets.value_size {
            let message = format!("A value grew past the value-size budget of {}.", self.budgets.value_size);
            return Err(budget(message, span));
        }
        Ok(())
    }

    fn set_node(&mut self, id: FiberId, node: &'p Node, env: Env<'p>) {
        self.fibers[id].node = Some((Child::Node(node), env));
    }

    fn set_returning(&mut self, id: FiberId, value: Value) {
        self.fibers[id].returning = Some(Returned::Value(value));
    }

    fn push_frame(&mut self, id: FiberId, frame: Frame<'p>) {
        self.fibers[id].stack.push(frame);
    }

    fn evaluate(&mut self, id: FiberId, child: Child<'p>, env: Env<'p>) -> Step<()> {
        let node = match child {
            Child::Lambda(lambda) => {
                let closure = closure(lambda, &env)?;
                self.fibers[id].returning = Some(Returned::Closure(closure));
                return Ok(());
            }
            Child::Node(node) => node,
        };
        match &node.kind {
            NodeKind::Literal { value, .. } => self.set_returning(id, value.clone()),
            NodeKind::Variable { name } => match lookup(&env, name)? {
                Slot::Local(_) => {
                    return Err(internal(format!("{name} is a local function used as a value."), None));
                }
                Slot::Value(value) => self.set_returning(id, value),
            },
            NodeKind::Lambda(lambda) => {
                let closure = closure(lambda, &env)?;
                self.fibers[id].returning = Some(Returned::Closure(closure));
            }
            NodeKind::Let(let_node) => {
                self.push_frame(id, Frame::Let { node: let_node, env: env.clone() });
                self.set_node(id, &let_node.value, env);
            }
            NodeKind::LocalFunction { name, body } => {
                let scope = extend(env.clone(), name, Slot::Local(env));
                self.set_node(id, body, scope);
            }
            NodeKind::If(if_node) => {
                self.push_frame(id, Frame::If { node: if_node, env: env.clone() });
                self.set_node(id, &if_node.condition, env);
            }
            NodeKind::Match(match_node) => {
                self.push_frame(id, Frame::Match { node: match_node, span: node.span, env: env.clone() });
                self.set_node(id, &match_node.scrutinee, env);
            }
            NodeKind::Attempt { body } => {
                self.push_frame(id, Frame::Attempt);
                self.set_node(id, body, env);
            }
            NodeKind::Retry(retry) => {
                self.push_frame(id, Frame::Retry { node: retry, span: node.span, env: env.clone(), attempt: 0 });
                self.set_node(id, &retry.body, env);
            }
            NodeKind::WaitUntil(wait) => {
                self.push_frame(id, Frame::Wait(WaitState { node: wait, span: node.span, env: env.clone(), round: 0 }));
                self.set_node(id, &wait.body, env);
            }
            NodeKind::Sleep { milliseconds } => {
                if *milliseconds == 0 {
                    self.set_returning(id, Value::Unit);
                } else {
                    self.request_delay(id, *milliseconds, node.span)?;
                }
            }
            NodeKind::Par { branches } => {
                self.joins += 1;
                let join_id = self.joins;
                let mut children = Vec::with_capacity(branches.len());
                for (index, branch) in branches.iter().enumerate() {
                    let mut path = self.fibers[id].path.clone();
                    path.push(index);
                    let child = self.fibers.len();
                    let mut fiber = Fiber::new(path, Some((id, join_id, index)));
                    fiber.node = Some((Child::Node(branch), env.clone()));
                    self.fibers.push(fiber);
                    children.push(child);
                }
                self.live.extend(children.iter().copied());
                self.push_frame(
                    id,
                    Frame::Join(JoinFrame {
                        id: join_id,
                        span: node.span,
                        count: branches.len(),
                        as_tuple: true,
                        closure: None,
                        items: Arc::from(Vec::new()),
                        limit: 0,
                        started: 0,
                        results: BTreeMap::new(),
                        failures: BTreeMap::new(),
                        children,
                    }),
                );
                self.fibers[id].joining = true;
            }
            _ => {
                let children = sequence_children(node)?;
                if children.is_empty() {
                    return self.finish_sequence(id, node, env, Vec::new());
                }
                let first = children[0];
                self.push_frame(id, Frame::Seq { origin: node, children, env: env.clone(), values: Vec::new() });
                self.fibers[id].node = Some((first, env));
            }
        }
        Ok(())
    }

    fn resume_frame(&mut self, id: FiberId, frame: Frame<'p>, value: Returned<'p>) -> Step<()> {
        if let Returned::Recurring(values) = value {
            return match frame {
                Frame::Loop { node, span, env, iteration } => self.recur(id, node, span, env, iteration, values),
                _ => {
                    self.fibers[id].returning = Some(Returned::Recurring(values));
                    Ok(())
                }
            };
        }
        match frame {
            Frame::Seq { origin, children, env, mut values } => {
                values.push(value);
                if values.len() < children.len() {
                    let next = children[values.len()];
                    self.fibers[id].node = Some((next, env.clone()));
                    self.push_frame(id, Frame::Seq { origin, children, env, values });
                    Ok(())
                } else {
                    self.finish_sequence(id, origin, env, values)
                }
            }
            Frame::Let { node, env } => {
                let bound = bind(env, &node.binder, plain(value)?)?;
                self.set_node(id, &node.body, bound);
                Ok(())
            }
            Frame::If { node, env } => {
                let chosen = if matches!(value, Returned::Value(Value::Bool(true))) { &node.then } else { &node.otherwise };
                self.set_node(id, chosen, env);
                Ok(())
            }
            Frame::Match { node, span, env } => {
                let value = plain(value)?;
                for (pattern, body) in &node.clauses {
                    if let Some(bound) = match_value(pattern, &value, env.clone())? {
                        self.set_node(id, body, bound);
                        return Ok(());
                    }
                }
                Err(internal("No match clause matched.", Some(span)))
            }
            Frame::Attempt => {
                let value = plain(value)?;
                self.set_returning(id, Value::Ok(Arc::new(value)));
                Ok(())
            }
            Frame::Retry { .. } | Frame::Loop { .. } => {
                self.fibers[id].returning = Some(value);
                Ok(())
            }
            Frame::Wait(wait) => {
                let value = plain(value)?;
                let predicate = closure(&wait.node.predicate, &wait.env)?;
                let span = wait.span;
                self.push_frame(id, Frame::WaitCheck { wait, value: value.clone() });
                self.apply(id, &predicate, vec![value], span)
            }
            Frame::WaitCheck { wait, value: checked } => {
                let satisfied = matches!(value, Returned::Value(Value::Bool(true)));
                self.wait_again(id, wait, checked, satisfied)
            }
            Frame::Each(each) => {
                let value = plain(value)?;
                self.each(id, each, value)
            }
            Frame::ThenEval { node, env } => {
                self.set_node(id, node, env);
                Ok(())
            }
            Frame::Join(_) => Err(internal("A join frame received a value directly.", None)),
        }
    }

    fn recur(
        &mut self,
        id: FiberId,
        node: &'p ir::Loop,
        span: Span,
        env: Env<'p>,
        iteration: i64,
        values: Vec<Value>,
    ) -> Step<()> {
        if iteration >= node.maximum {
            let message = format!("This loop asked to recur after {} iterations, its :max.", node.maximum);
            return Err(Failure::new(FailureKind::LoopLimit, message, Some(span)));
        }
        if values.len() != node.binders.len() {
            return Err(internal("recur gave the wrong number of values.", Some(span)));
        }
        let mut bound = env.clone();
        for (binder, item) in node.binders.iter().zip(values) {
            bound = bind(bound, binder, item)?;
        }
        self.push_frame(id, Frame::Loop { node, span, env, iteration: iteration + 1 });
        self.set_node(id, &node.body, bound);
        Ok(())
    }

    fn wait_again(&mut self, id: FiberId, mut wait: WaitState<'p>, value: Value, satisfied: bool) -> Step<()> {
        let node = wait.node;
        if satisfied {
            self.set_returning(id, value);
            return Ok(());
        }
        if wait.round >= node.intervals.len() {
            let total: i64 = node.intervals.iter().sum();
            let message = format!("wait-until gave up after {total} ms.");
            self.fibers[id].raising =
                Some(Raised { error: call_error_value(CallErrorKind::Timeout, &message), span: wait.span });
            return Ok(());
        }
        let delay = node.intervals[wait.round];
        wait.round += 1;
        let span = wait.span;
        let env = wait.env.clone();
        self.push_frame(id, Frame::Wait(wait));
        self.push_frame(id, Frame::ThenEval { node: &node.body, env });
        self.request_delay(id, delay, span)
    }

    fn unwind(&mut self, id: FiberId) -> Step<()> {
        let Some(raised) = self.fibers[id].raising.take() else {
            return Ok(());
        };
        while let Some(frame) = self.fibers[id].stack.pop() {
            match frame {
                Frame::Attempt => {
                    self.set_returning(id, Value::Err(Arc::new(Value::Record(raised.error))));
                    return Ok(());
                }
                Frame::Retry { node, span, env, attempt } => {
                    let kind = match raised.error.get("kind") {
                        Some(Value::Variant(variant)) => CallErrorKind::from_name(&variant.variant),
                        _ => return Err(internal("A failure carries no call error kind.", None)),
                    };
                    if kind.is_some_and(|k| node.kinds.contains(&k)) && attempt < node.delays.len() {
                        let delay = node.delays[attempt];
                        let body_env = env.clone();
                        self.push_frame(id, Frame::Retry { node, span, env, attempt: attempt + 1 });
                        self.push_frame(id, Frame::ThenEval { node: &node.body, env: body_env });
                        return self.request_delay(id, delay, span);
                    }
                }
                Frame::Join(_) => return Err(internal("A failure reached a join frame.", None)),
                _ => {}
            }
        }
        self.fail_fiber(id, raised);
        Ok(())
    }

    fn request_delay(&mut self, id: FiberId, milliseconds: i64, span: Span) -> Step<()> {
        self.delay = self.delay.saturating_add(u64::try_from(milliseconds).unwrap_or(0));
        if self.delay > self.budgets.delay_ms {
            let message = format!("The program asked for more than {} ms of delays (delay budget).", self.budgets.delay_ms);
            return Err(budget(message, Some(span)));
        }
        let (request_id, order) = self.next_id(id);
        self.fibers[id].waiting = Some(Waiting { request: Request::Delay { id: request_id, milliseconds }, order, output: None });
        Ok(())
    }

    fn each(&mut self, id: FiberId, mut frame: EachFrame<'p>, value: Value) -> Step<()> {
        match frame.op {
            HigherOrder::Reduce => frame.accumulator = Some(value),
            HigherOrder::Any | HigherOrder::All => {
                let deciding = frame.op == HigherOrder::Any;
                if value == Value::Bool(deciding) {
                    self.set_returning(id, Value::Bool(deciding));
                    return Ok(());
                }
            }
            _ => frame.results.push(value),
        }
        frame.index += 1;
        if frame.index < frame.items.len() {
            let args = each_args(&frame)?;
            let closure = frame.closure.clone();
            let span = frame.span;
            self.push_frame(id, Frame::Each(frame));
            return self.apply(id, &closure, args, span);
        }
        let result = finish_each(frame)?;
        self.set_returning(id, result);
        Ok(())
    }

    fn apply(&mut self, id: FiberId, closure: &Closure<'p>, args: Vec<Value>, span: Span) -> Step<()> {
        self.applications += 1;
        if self.applications > self.budgets.applications {
            let message = format!(
                "The program applied functions more than {} times (applications budget).",
                self.budgets.applications
            );
            return Err(budget(message, Some(span)));
        }
        if args.len() != closure.params.len() {
            return Err(internal("A function was applied to the wrong number of arguments.", Some(span)));
        }
        let mut env = closure.env.clone();
        for (binder, arg) in closure.params.iter().zip(args) {
            env = bind(env, binder, arg)?;
        }
        self.set_node(id, closure.body, env);
        Ok(())
    }

    fn finish_sequence(&mut self, id: FiberId, node: &'p Node, env: Env<'p>, values: Vec<Returned<'p>>) -> Step<()> {
        let span = node.span;
        match &node.kind {
            NodeKind::Prelude { op, .. } => self.prelude(id, *op, values, span),
            NodeKind::CoreCall { function, .. } => {
                let args = plain_all(values)?;
                let result = match self.cores.apply(function, &args) {
                    Ok(result) => result,
                    Err(CoreError::Unimplemented) => {
                        return Err(internal(format!("{function} has a signature but no implementation."), Some(span)));
                    }
                    Err(CoreError::Failed(message)) => return Err(internal(format!("{function}: {message}"), Some(span))),
                    Err(CoreError::Arithmetic(message)) => {
                        return Err(Failure::new(FailureKind::Arithmetic, format!("{function}: {message}"), Some(span)));
                    }
                };
                self.check_size(&result, Some(span))?;
                self.set_returning(id, result);
                Ok(())
            }
            NodeKind::Call { capability, .. } => {
                self.calls += 1;
                if self.calls > self.budgets.calls {
                    let message = format!("The program made more than {} tool calls (calls budget).", self.budgets.calls);
                    return Err(budget(message, Some(span)));
                }
                let Some(signature) = self.program.signatures.capabilities.get(capability) else {
                    return Err(internal(format!("{capability} has no signature."), Some(span)));
                };
                let argument = plain(single(values)?)?;
                let input = encode(&argument, &signature.input);
                let (request_id, order) = self.next_id(id);
                let fiber = &mut self.fibers[id];
                fiber.waiting = Some(Waiting {
                    request: Request::Call { id: request_id, capability: capability.clone(), input },
                    order,
                    output: Some(&signature.output),
                });
                fiber.call_span = Some(span);
                Ok(())
            }
            NodeKind::Get { field, .. } => {
                let record = plain(single(values)?)?;
                let Value::Record(record) = record else {
                    return Err(internal("get of a value that is not a record.", Some(span)));
                };
                let Some(found) = record.get(field) else {
                    return Err(internal(format!("The record has no field {field}."), Some(span)));
                };
                let found = found.clone();
                self.set_returning(id, found);
                Ok(())
            }
            NodeKind::MakeRecord { fields } => {
                let values = plain_all(values)?;
                let built = Value::Record(RecordValue::new(
                    fields.iter().zip(values).map(|((name, _), v)| (Arc::from(name.as_str()), v)).collect(),
                ));
                self.check_size(&built, Some(span))?;
                self.set_returning(id, built);
                Ok(())
            }
            NodeKind::MakeList { .. } => {
                let built = Value::list(plain_all(values)?);
                self.check_size(&built, Some(span))?;
                self.set_returning(id, built);
                Ok(())
            }
            NodeKind::MakeTuple { .. } => {
                let built = Value::tuple(plain_all(values)?);
                self.set_returning(id, built);
                Ok(())
            }
            NodeKind::Wrap { kind, .. } => {
                let inner = Arc::new(plain(single(values)?)?);
                let wrapped = match kind {
                    WrapKind::Some => Value::Some(inner),
                    WrapKind::Ok => Value::Ok(inner),
                    WrapKind::Err => Value::Err(inner),
                };
                self.set_returning(id, wrapped);
                Ok(())
            }
            NodeKind::MakeVariant { enum_name, variant, .. } => {
                let payload = plain_all(values)?.into_iter().next();
                self.set_returning(id, Value::variant(enum_name, variant, payload));
                Ok(())
            }
            NodeKind::Coerce { conversion, .. } => {
                let value = plain(single(values)?)?;
                self.set_returning(id, convert(&value, conversion)?);
                Ok(())
            }
            NodeKind::ApplyLocal { function, name, .. } => {
                let Slot::Local(scope) = lookup(&env, name)? else {
                    return Err(internal(format!("{name} is not a local function."), Some(span)));
                };
                let closure = Closure { params: &function.params, body: &function.body, env: scope };
                self.apply(id, &closure, plain_all(values)?, span)
            }
            NodeKind::Recur { .. } => {
                self.fibers[id].returning = Some(Returned::Recurring(plain_all(values)?));
                Ok(())
            }
            NodeKind::Loop(loop_node) => {
                let values = plain_all(values)?;
                if values.len() != loop_node.binders.len() {
                    return Err(internal("loop has the wrong number of initial values.", Some(span)));
                }
                let mut bound = env.clone();
                for (binder, value) in loop_node.binders.iter().zip(values) {
                    bound = bind(bound, binder, value)?;
                }
                self.push_frame(id, Frame::Loop { node: loop_node, span, env, iteration: 1 });
                self.set_node(id, &loop_node.body, bound);
                Ok(())
            }
            NodeKind::ParMap { limit, .. } => {
                let mut values = values.into_iter();
                let (Some(Returned::Closure(closure)), Some(Returned::Value(Value::List(items))), None) =
                    (values.next(), values.next(), values.next())
                else {
                    return Err(internal("par-map needs a function and a list.", Some(span)));
                };
                self.joins += 1;
                let join = JoinFrame {
                    id: self.joins,
                    span,
                    count: items.len(),
                    as_tuple: false,
                    closure: Some(closure),
                    items,
                    limit: usize::try_from(*limit).unwrap_or(0),
                    started: 0,
                    results: BTreeMap::new(),
                    failures: BTreeMap::new(),
                    children: Vec::new(),
                };
                self.push_frame(id, Frame::Join(join));
                self.fibers[id].joining = true;
                self.start_children(id)
            }
            other => Err(internal(format!("No evaluation rule for {}.", other.name()), None)),
        }
    }

    fn prelude(&mut self, id: FiberId, op: PreludeOp, values: Vec<Returned<'p>>, span: Span) -> Step<()> {
        let PreludeOp::HigherOrder(order) = op else {
            let args = plain_all(values)?;
            let result = self.pure_prelude(op, &args, span)?;
            self.set_returning(id, result);
            return Ok(());
        };
        let mut values = values.into_iter();
        let Some(Returned::Closure(closure)) = values.next() else {
            return Err(internal("A higher-order prelude function needs a function.", Some(span)));
        };
        let rest: Vec<Returned<'p>> = values.collect();
        let mut rest = rest.into_iter();
        let accumulator = if order == HigherOrder::Reduce {
            match rest.next() {
                Some(Returned::Value(value)) => Some(value),
                _ => return Err(internal("reduce needs an initial value.", Some(span))),
            }
        } else {
            None
        };
        let Some(Returned::Value(Value::List(items))) = rest.next_back() else {
            return Err(internal("A higher-order prelude function needs a list.", Some(span)));
        };
        let frame = EachFrame { op: order, closure, items, span, accumulator, index: 0, results: Vec::new() };
        if frame.items.is_empty() {
            let result = finish_each(frame)?;
            self.set_returning(id, result);
            return Ok(());
        }
        let args = each_args(&frame)?;
        let closure = frame.closure.clone();
        self.push_frame(id, Frame::Each(frame));
        self.apply(id, &closure, args, span)
    }

    fn pure_prelude(&self, op: PreludeOp, args: &[Value], span: Span) -> Step<Value> {
        let arg = |index: usize| args.get(index).ok_or_else(|| internal("A prelude function is missing an argument.", Some(span)));
        if matches!(op, PreludeOp::Add | PreludeOp::Subtract | PreludeOp::Multiply | PreludeOp::Divide)
            && let Value::Float(_) = arg(0)?
        {
            return float_arithmetic(op, args, span);
        }
        let int = |index: usize| match arg(index)? {
            Value::Int(n) => Ok(*n),
            _ => Err(internal("A prelude function expected an Int.", Some(span))),
        };
        let float = |index: usize| match arg(index)? {
            Value::Float(f) => Ok(f.0),
            _ => Err(internal("A prelude function expected a Float.", Some(span))),
        };
        let list = |index: usize| match arg(index)? {
            Value::List(items) => Ok(items.clone()),
            _ => Err(internal("A prelude function expected a list.", Some(span))),
        };
        let map = |index: usize| match arg(index)? {
            Value::Map(map) => Ok(map.clone()),
            _ => Err(internal("A prelude function expected a map.", Some(span))),
        };
        let text = |index: usize| match arg(index)? {
            Value::String(s) => Ok(s.clone()),
            _ => Err(internal("A prelude function expected a String.", Some(span))),
        };
        Ok(match op {
            PreludeOp::Add => Value::Int(checked_int(i128::from(int(0)?) + i128::from(int(1)?), span)?),
            PreludeOp::Subtract => {
                if args.len() == 1 {
                    Value::Int(checked_int(-i128::from(int(0)?), span)?)
                } else {
                    Value::Int(checked_int(i128::from(int(0)?) - i128::from(int(1)?), span)?)
                }
            }
            PreludeOp::Multiply => Value::Int(checked_int(i128::from(int(0)?) * i128::from(int(1)?), span)?),
            PreludeOp::Quot | PreludeOp::Rem => {
                let (a, b) = (i128::from(int(0)?), i128::from(int(1)?));
                if b == 0 {
                    return Err(Failure::new(FailureKind::Arithmetic, format!("{} by zero.", op.name()), Some(span)));
                }
                let sign = if (a >= 0) == (b >= 0) { 1 } else { -1 };
                let quotient = a.abs() / b.abs() * sign;
                let result = if op == PreludeOp::Quot { quotient } else { a - b * quotient };
                Value::Int(checked_int(result, span)?)
            }
            PreludeOp::Float => Value::float(int(0)? as f64),
            PreludeOp::Round => {
                let original = float(0)?;
                Value::Int(int_from_float(op, round_half_away(original), original, span)?)
            }
            PreludeOp::Floor => {
                let original = float(0)?;
                Value::Int(int_from_float(op, original.floor(), original, span)?)
            }
            PreludeOp::Ceil => {
                let original = float(0)?;
                Value::Int(int_from_float(op, original.ceil(), original, span)?)
            }
            PreludeOp::Equal => Value::Bool(arg(0)? == arg(1)?),
            PreludeOp::NotEqual => Value::Bool(arg(0)? != arg(1)?),
            PreludeOp::Less | PreludeOp::LessEqual | PreludeOp::Greater | PreludeOp::GreaterEqual => {
                let invalid = Cell::new(false);
                let order = compare_keys(arg(0)?, arg(1)?, &invalid);
                if invalid.get() {
                    return Err(sort_key_failure());
                }
                Value::Bool(match op {
                    PreludeOp::Less => order == Ordering::Less,
                    PreludeOp::LessEqual => order != Ordering::Greater,
                    PreludeOp::Greater => order == Ordering::Greater,
                    _ => order != Ordering::Less,
                })
            }
            PreludeOp::Not => Value::Bool(*arg(0)? == Value::Bool(false)),
            PreludeOp::And => Value::Bool(*arg(0)? == Value::Bool(true) && *arg(1)? == Value::Bool(true)),
            PreludeOp::Or => Value::Bool(*arg(0)? == Value::Bool(true) || *arg(1)? == Value::Bool(true)),
            PreludeOp::Str => {
                let mut joined = String::new();
                for value in args {
                    joined.push_str(&text_of(value, span)?);
                }
                let joined = Value::string(&joined);
                self.check_size(&joined, Some(span))?;
                joined
            }
            PreludeOp::Count => Value::Int(match arg(0)? {
                Value::Map(map) => map.entries().len() as i64,
                Value::Bytes(bytes) => bytes.len() as i64,
                _ => list(0)?.len() as i64,
            }),
            PreludeOp::First => match list(0)?.first() {
                Some(first) => first.clone(),
                None => return Err(Failure::new(FailureKind::OutOfRange, "first of an empty list.", Some(span))),
            },
            PreludeOp::Rest => {
                let items = list(0)?;
                Value::list(items.iter().skip(1).cloned().collect())
            }
            PreludeOp::TupleItem => {
                let index = int(1)?;
                let Value::Tuple(items) = arg(0)? else {
                    return Err(internal("tuple-item of a non-tuple.", Some(span)));
                };
                usize::try_from(index)
                    .ok()
                    .and_then(|at| items.get(at))
                    .cloned()
                    .ok_or_else(|| internal("tuple-item index is outside the tuple.", Some(span)))?
            }
            PreludeOp::Nth => {
                let (items, index) = (list(0)?, int(1)?);
                match usize::try_from(index).ok().and_then(|at| items.get(at)) {
                    Some(found) => found.clone(),
                    None => {
                        let message = format!("nth {index} of a list of {}.", items.len());
                        return Err(Failure::new(FailureKind::OutOfRange, message, Some(span)));
                    }
                }
            }
            PreludeOp::Take => {
                let (count, items) = (int(0)?, list(1)?);
                if count < 0 {
                    let message = format!("take with a negative count, {count}.");
                    return Err(Failure::new(FailureKind::Arithmetic, message, Some(span)));
                }
                let count = usize::try_from(count).unwrap_or(usize::MAX);
                Value::list(items.iter().take(count).cloned().collect())
            }
            PreludeOp::Concat => {
                let (a, b) = (list(0)?, list(1)?);
                let joined = Value::list(a.iter().chain(b.iter()).cloned().collect());
                self.check_size(&joined, Some(span))?;
                joined
            }
            PreludeOp::Flatten => {
                let mut flat = Vec::new();
                for inner in list(0)?.iter() {
                    let Value::List(items) = inner else {
                        return Err(internal("flatten of a list that holds something other than lists.", Some(span)));
                    };
                    flat.extend(items.iter().cloned());
                }
                Value::list(flat)
            }
            PreludeOp::Contains => match arg(0)? {
                Value::Map(map) => Value::Bool(map.has(&text(1)?)),
                _ => {
                    let needle = arg(1)?;
                    Value::Bool(list(0)?.iter().any(|item| item == needle))
                }
            },
            PreludeOp::MapGet => match map(0)?.get(&text(1)?) {
                Some(found) => Value::some(found.clone()),
                None => Value::None,
            },
            PreludeOp::Assoc => {
                let assigned = Value::Map(map(0)?.assoc(text(1)?, arg(2)?.clone()));
                self.check_size(&assigned, Some(span))?;
                assigned
            }
            PreludeOp::Dissoc => Value::Map(map(0)?.dissoc(&text(1)?)),
            PreludeOp::Keys => Value::list(map(0)?.entries().iter().map(|(k, _)| Value::String(k.clone())).collect()),
            PreludeOp::Vals => Value::list(map(0)?.entries().iter().map(|(_, v)| v.clone()).collect()),
            PreludeOp::Entries => Value::list(
                map(0)?.entries().iter().map(|(k, v)| Value::tuple(vec![Value::String(k.clone()), v.clone()])).collect(),
            ),
            PreludeOp::IntoMap => {
                let mut pairs = Vec::new();
                for pair in list(0)?.iter() {
                    match pair {
                        Value::Tuple(items) if items.len() == 2 => match &items[0] {
                            Value::String(key) => pairs.push((key.clone(), items[1].clone())),
                            _ => return Err(internal("into-map needs String keys.", Some(span))),
                        },
                        _ => return Err(internal("into-map needs pairs.", Some(span))),
                    }
                }
                let built = Value::Map(MapValue::from_pairs(pairs));
                self.check_size(&built, Some(span))?;
                built
            }
            PreludeOp::Divide => return Err(internal("No prelude rule for /.", Some(span))),
            PreludeOp::HigherOrder(_) => return Err(internal(format!("No prelude rule for {}.", op.name()), Some(span))),
        })
    }
}

fn single(values: Vec<Returned<'_>>) -> Step<Returned<'_>> {
    let mut values = values.into_iter();
    match (values.next(), values.next()) {
        (Some(value), None) => Ok(value),
        _ => Err(internal("A form expected exactly one operand.", None)),
    }
}

fn plain_all(values: Vec<Returned<'_>>) -> Step<Vec<Value>> {
    values.into_iter().map(plain).collect()
}

fn closure<'p>(function: &'p Lambda, env: &Env<'p>) -> Step<Closure<'p>> {
    let Some(captured) = &function.captured else {
        return Ok(Closure { params: &function.params, body: &function.body, env: env.clone() });
    };
    match lookup(env, captured)? {
        Slot::Local(scope) => Ok(Closure { params: &function.params, body: &function.body, env: scope }),
        Slot::Value(_) => Err(internal(format!("{captured} is not a local function."), None)),
    }
}

fn nodes(items: &[Node]) -> Vec<Child<'_>> {
    items.iter().map(Child::Node).collect()
}

fn sequence_children(node: &Node) -> Step<Vec<Child<'_>>> {
    Ok(match &node.kind {
        NodeKind::Prelude { args, .. }
        | NodeKind::CoreCall { args, .. }
        | NodeKind::ApplyLocal { args, .. }
        | NodeKind::Recur { args } => nodes(args),
        NodeKind::Call { argument, .. } => vec![Child::Node(argument)],
        NodeKind::Get { record, .. } => vec![Child::Node(record)],
        NodeKind::MakeRecord { fields } => fields.iter().map(|(_, value)| Child::Node(value)).collect(),
        NodeKind::MakeList { items, .. } | NodeKind::MakeTuple { items } => nodes(items),
        NodeKind::Wrap { value, .. } | NodeKind::Coerce { value, .. } => vec![Child::Node(value)],
        NodeKind::MakeVariant { payload, .. } => payload.iter().map(|p| Child::Node(p)).collect(),
        NodeKind::Loop(loop_node) => nodes(&loop_node.initial),
        NodeKind::ParMap { function, items, .. } => vec![Child::Lambda(function), Child::Node(items)],
        other => return Err(internal(format!("No evaluation rule for {}.", other.name()), None)),
    })
}

fn response_trace(request_id: &str, response: &Response) -> Json {
    match response {
        Response::Output(value) => Json::object([("response", Json::string(request_id)), ("output", value.clone())]),
        Response::Refusal { kind, message } => Json::object([
            ("response", Json::string(request_id)),
            ("error", Json::object([("kind", Json::string(kind.as_str())), ("message", Json::string(message.as_str()))])),
        ]),
    }
}

fn bind<'p>(env: Env<'p>, binder: &'p Binder, value: Value) -> Step<Env<'p>> {
    match binder {
        Binder::Name(name) => Ok(extend(env, name, Slot::Value(value))),
        Binder::Discard => Ok(env),
        Binder::Tuple(items) => {
            let Value::Tuple(parts) = value else {
                return Err(internal("A tuple pattern met a value that is not a tuple.", None));
            };
            if parts.len() != items.len() {
                return Err(internal("A tuple pattern met a tuple of another length.", None));
            }
            let mut env = env;
            for (item, part) in items.iter().zip(parts.iter()) {
                env = bind(env, item, part.clone())?;
            }
            Ok(env)
        }
    }
}

fn match_value<'p>(pattern: &'p MatchPattern, value: &Value, env: Env<'p>) -> Step<Option<Env<'p>>> {
    match (pattern, value) {
        (MatchPattern::Any(binder), _) => bind(env, binder, value.clone()).map(Some),
        (MatchPattern::None, Value::None) => Ok(Some(env)),
        (MatchPattern::Literal(literal), _) => Ok((value == literal).then_some(env)),
        (MatchPattern::Some(inner), Value::Some(v))
        | (MatchPattern::Ok(inner), Value::Ok(v))
        | (MatchPattern::Err(inner), Value::Err(v)) => match_value(inner, v, env),
        (MatchPattern::Variant { variant, inner }, Value::Variant(found)) => {
            if *found.variant != **variant {
                return Ok(None);
            }
            match (inner, &found.payload) {
                (None, _) => Ok(Some(env)),
                (Some(inner), Some(payload)) => match_value(inner, payload, env),
                (Some(_), None) => Err(internal("A variant pattern expected a payload.", None)),
            }
        }
        _ => Ok(None),
    }
}

fn convert(value: &Value, conversion: &Conversion) -> Step<Value> {
    match conversion {
        Conversion::VariantName => match value {
            Value::Variant(variant) => Ok(Value::String(variant.variant.clone())),
            _ => Err(internal("variant-name of a value that is not a variant.", None)),
        },
        Conversion::ToSome(inner) => Ok(Value::some(match inner {
            None => value.clone(),
            Some(inner) => convert(value, inner)?,
        })),
        Conversion::EachField(fields) => {
            let Value::Record(record) = value else {
                return Err(internal("each-field of a value that is not a record.", None));
            };
            let mut converted = Vec::with_capacity(record.fields().len());
            for (name, item) in record.fields() {
                let field_conversion = fields.iter().find(|(n, _)| **n == **name).and_then(|(_, c)| c.as_ref());
                converted.push((
                    name.clone(),
                    match field_conversion {
                        None => item.clone(),
                        Some(c) => convert(item, c)?,
                    },
                ));
            }
            Ok(Value::Record(RecordValue::new(converted)))
        }
        Conversion::WidenRecord(widened) => {
            let Value::Record(record) = value else {
                return Err(internal("widen-record of a value that is not a record.", None));
            };
            let mut fields = Vec::with_capacity(widened.len());
            for field in widened {
                let item = if !field.present {
                    Value::None
                } else {
                    let Some(item) = record.get(&field.name) else {
                        return Err(internal(format!("widen-record found no field {}.", field.name), None));
                    };
                    match &field.conversion {
                        None => item.clone(),
                        Some(c) => convert(item, c)?,
                    }
                };
                fields.push((Arc::from(field.name.as_str()), item));
            }
            Ok(Value::Record(RecordValue::new(fields)))
        }
        Conversion::EachElement(element) => {
            let Value::List(items) = value else {
                return Err(internal("each-element of a value that is not a list.", None));
            };
            Ok(Value::list(items.iter().map(|item| convert(item, element)).collect::<Step<_>>()?))
        }
        Conversion::EachItem(conversions) => {
            let Value::Tuple(items) = value else {
                return Err(internal("each-item of a value that is not a tuple.", None));
            };
            if items.len() != conversions.len() {
                return Err(internal("each-item of a tuple of another length.", None));
            }
            let converted = items
                .iter()
                .zip(conversions)
                .map(|(item, c)| match c {
                    None => Ok(item.clone()),
                    Some(c) => convert(item, c),
                })
                .collect::<Step<_>>()?;
            Ok(Value::tuple(converted))
        }
    }
}

fn sort_key_failure() -> Failure {
    internal("A sort key is not an Int, Float, String, Bool, or tuple.", None)
}

fn rank(value: &Value) -> u8 {
    match value {
        Value::List(_) | Value::Tuple(_) => 0,
        Value::Bool(_) => 1,
        Value::Int(_) | Value::Float(_) => 2,
        Value::String(_) => 3,
        _ => 4,
    }
}

fn compare_int_float(i: i64, f: f64) -> Ordering {
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    if f >= LIMIT {
        return Ordering::Less;
    }
    if f < -LIMIT {
        return Ordering::Greater;
    }
    let whole = f.trunc();
    match i.cmp(&(whole as i64)) {
        Ordering::Equal => 0.0f64.partial_cmp(&(f - whole)).unwrap_or(Ordering::Equal),
        other => other,
    }
}

fn compare_keys(a: &Value, b: &Value, invalid: &Cell<bool>) -> Ordering {
    match (a, b) {
        (Value::List(x) | Value::Tuple(x), Value::List(y) | Value::Tuple(y)) => {
            for (p, q) in x.iter().zip(y.iter()) {
                let order = compare_keys(p, q, invalid);
                if order != Ordering::Equal {
                    return order;
                }
            }
            x.len().cmp(&y.len())
        }
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Float(x), Value::Float(y)) => x.0.partial_cmp(&y.0).unwrap_or(Ordering::Equal),
        (Value::Int(x), Value::Float(y)) => compare_int_float(*x, y.0),
        (Value::Float(x), Value::Int(y)) => compare_int_float(*y, x.0).reverse(),
        (Value::String(x), Value::String(y)) => x.cmp(y),
        _ => {
            invalid.set(true);
            rank(a).cmp(&rank(b))
        }
    }
}

fn each_args(frame: &EachFrame<'_>) -> Step<Vec<Value>> {
    let item = frame.items[frame.index].clone();
    if frame.op == HigherOrder::Reduce {
        let Some(accumulator) = frame.accumulator.clone() else {
            return Err(internal("reduce lost its accumulator.", None));
        };
        return Ok(vec![accumulator, item]);
    }
    Ok(vec![item])
}

fn finish_each(frame: EachFrame<'_>) -> Step<Value> {
    let EachFrame { op, items, results, accumulator, .. } = frame;
    Ok(match op {
        HigherOrder::Map => Value::list(results),
        HigherOrder::Filter => Value::list(
            items.iter().zip(&results).filter(|(_, keep)| **keep == Value::Bool(true)).map(|(item, _)| item.clone()).collect(),
        ),
        HigherOrder::Keep => Value::list(
            results
                .into_iter()
                .filter_map(|r| match r {
                    Value::Some(inner) => Some((*inner).clone()),
                    _ => None,
                })
                .collect(),
        ),
        HigherOrder::Any => Value::Bool(false),
        HigherOrder::All => Value::Bool(true),
        HigherOrder::Reduce => accumulator.ok_or_else(|| internal("reduce lost its accumulator.", None))?,
        HigherOrder::SortBy => {
            let invalid = Cell::new(false);
            let mut order: Vec<usize> = (0..items.len()).collect();
            order.sort_by(|i, j| compare_keys(&results[*i], &results[*j], &invalid));
            if invalid.get() {
                return Err(sort_key_failure());
            }
            Value::list(order.into_iter().map(|i| items[i].clone()).collect())
        }
        HigherOrder::DistinctBy => {
            let mut seen: HashSet<&Value> = HashSet::new();
            let mut kept = Vec::new();
            for (item, key) in items.iter().zip(&results) {
                if seen.insert(key) {
                    kept.push(item.clone());
                }
            }
            Value::list(kept)
        }
        HigherOrder::GroupBy => {
            let mut index: HashMap<&Value, usize> = HashMap::new();
            let mut groups: Vec<(Value, Vec<Value>)> = Vec::new();
            for (item, key) in items.iter().zip(&results) {
                match index.get(key) {
                    Some(at) => groups[*at].1.push(item.clone()),
                    None => {
                        index.insert(key, groups.len());
                        groups.push((key.clone(), vec![item.clone()]));
                    }
                }
            }
            Value::list(groups.into_iter().map(|(key, group)| Value::tuple(vec![key, Value::list(group)])).collect())
        }
    })
}

const INT_MIN_F: f64 = -9_223_372_036_854_775_808.0;
const INT_LIMIT_F: f64 = 9_223_372_036_854_775_808.0;

fn checked_int(value: i128, span: Span) -> Step<i64> {
    i64::try_from(value).map_err(|_| {
        Failure::new(
            FailureKind::Arithmetic,
            "Integer overflow: the result does not fit in a signed 64-bit Int.",
            Some(span),
        )
    })
}

fn checked_float(value: f64, span: Span) -> Step<Value> {
    if !value.is_finite() {
        let message = "Float overflow: the result is not a finite number, and JSON cannot carry one.";
        return Err(Failure::new(FailureKind::Arithmetic, message, Some(span)));
    }
    Ok(Value::float(value))
}

fn int_from_float(op: PreludeOp, value: f64, original: f64, span: Span) -> Step<i64> {
    if !(INT_MIN_F..INT_LIMIT_F).contains(&value) {
        let message = format!("{} of {} does not fit in a signed 64-bit Int.", op.name(), float_text(original));
        return Err(Failure::new(FailureKind::OutOfRange, message, Some(span)));
    }
    Ok(value as i64)
}

fn round_half_away(value: f64) -> f64 {
    let magnitude = value.abs();
    let whole = magnitude.floor();
    let rounded = if magnitude - whole >= 0.5 { whole + 1.0 } else { whole };
    if value >= 0.0 { rounded } else { -rounded }
}

fn float_arithmetic(op: PreludeOp, args: &[Value], span: Span) -> Step<Value> {
    let float = |index: usize| match args.get(index) {
        Some(Value::Float(f)) => Ok(f.0),
        _ => Err(internal("A Float operation expected a Float.", Some(span))),
    };
    let a = float(0)?;
    if op == PreludeOp::Subtract && args.len() == 1 {
        return Ok(Value::float(-a));
    }
    let b = float(1)?;
    match op {
        PreludeOp::Add => checked_float(a + b, span),
        PreludeOp::Subtract => checked_float(a - b, span),
        PreludeOp::Multiply => checked_float(a * b, span),
        _ => {
            if b == 0.0 {
                return Err(Failure::new(FailureKind::Arithmetic, "/ by zero.", Some(span)));
            }
            checked_float(a / b, span)
        }
    }
}

fn text_of(value: &Value, span: Span) -> Step<String> {
    match value {
        Value::Bool(b) => Ok(if *b { "true" } else { "false" }.to_string()),
        Value::Int(n) => Ok(n.to_string()),
        Value::String(s) => Ok(s.to_string()),
        Value::Float(f) => Ok(float_text(f.0)),
        Value::Variant(variant) if variant.payload.is_none() => Ok(variant.variant.to_string()),
        _ => Err(internal(format!("str cannot write {}.", show_value(value)), Some(span))),
    }
}
