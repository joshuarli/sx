from __future__ import annotations

from collections.abc import Mapping, Sequence
from dataclasses import dataclass, field
from enum import StrEnum
from functools import cmp_to_key

from sx import ir
from sx.checker import DEFAULT_BUDGETS, Budgets
from sx.syntax import Span
from sx.types import CALL_ERROR_KINDS, Type
from sx.values import (
    INT_MAX,
    INT_MIN,
    NONE_VALUE,
    UNIT_VALUE,
    DecodeError,
    ErrValue,
    ListValue,
    MapValue,
    NoneValue,
    OkValue,
    RecordValue,
    SomeValue,
    TupleValue,
    Value,
    VariantValue,
    call_error,
    decode,
    encode,
    make_map,
    show_value,
    size,
)

HIGHER_ORDER = frozenset({"map", "filter", "keep", "any?", "all?", "sort-by", "distinct-by", "group-by", "reduce"})


class FailureKind(StrEnum):
    CALL_FAILED = "call-failed"
    INVALID_HOST_RESPONSE = "invalid-host-response"
    BUDGET_EXHAUSTED = "budget-exhausted"
    ARITHMETIC = "arithmetic"
    OUT_OF_RANGE = "out-of-range"
    LOOP_LIMIT = "loop-limit"
    INTERNAL = "internal"


@dataclass(frozen=True, slots=True)
class Failure:
    kind: FailureKind
    message: str
    span: Span | None = None
    request: str | None = None


@dataclass(frozen=True, slots=True)
class CallRequest:
    id: str
    capability: str
    input: object


@dataclass(frozen=True, slots=True)
class DelayRequest:
    id: str
    milliseconds: int


type Request = CallRequest | DelayRequest


@dataclass(frozen=True, slots=True)
class Output:
    value: object


@dataclass(frozen=True, slots=True)
class Refusal:
    kind: str
    message: str


type Response = Output | Refusal


@dataclass(frozen=True, slots=True)
class Completed:
    value: Value
    json: object


@dataclass(frozen=True, slots=True)
class Suspended:
    requests: tuple[Request, ...]


@dataclass(frozen=True, slots=True)
class Failed:
    failure: Failure


type Outcome = Completed | Suspended | Failed


@dataclass(frozen=True, slots=True)
class LocalScope:
    env: Env | None


@dataclass(frozen=True, slots=True)
class Closure:
    params: tuple[ir.Binder, ...]
    body: ir.Node
    env: Env | None


type Slot = Value | LocalScope | Closure


@dataclass(frozen=True, slots=True)
class Env:
    name: str
    value: Slot
    parent: Env | None


def lookup(env: Env | None, name: str) -> Slot:
    while env is not None:
        if env.name == name:
            return env.value
        env = env.parent
    raise _TerminalError(Failure(FailureKind.INTERNAL, f"{name} is not bound."))


@dataclass(slots=True)
class Recurring:
    values: tuple[Value, ...]


type Result = Value | Closure | Recurring


class _TerminalError(Exception):
    def __init__(self, failure: Failure) -> None:
        super().__init__(failure.message)
        self.failure = failure


@dataclass(slots=True)
class Raised:
    error: RecordValue
    span: Span


@dataclass(slots=True)
class Seq:
    origin: ir.Node
    children: tuple[ir.Node, ...]
    env: Env | None
    values: list[Value | Closure] = field(default_factory=list)


@dataclass(slots=True)
class LetFrame:
    node: ir.Let
    env: Env | None


@dataclass(slots=True)
class IfFrame:
    node: ir.If
    env: Env | None


@dataclass(slots=True)
class MatchFrame:
    node: ir.Match
    env: Env | None


@dataclass(slots=True)
class AttemptFrame:
    pass


@dataclass(slots=True)
class RetryFrame:
    node: ir.Retry
    env: Env | None
    attempt: int = 0


@dataclass(slots=True)
class WaitFrame:
    node: ir.WaitUntil
    env: Env | None
    round: int = 0


@dataclass(slots=True)
class WaitCheckFrame:
    wait: WaitFrame
    value: Value


@dataclass(slots=True)
class LoopFrame:
    node: ir.Loop
    env: Env | None
    iteration: int = 1


@dataclass(slots=True)
class EachFrame:
    op: str
    closure: Closure
    items: tuple[Value, ...]
    span: Span
    accumulator: Value | None = None
    index: int = 0
    results: list[Value] = field(default_factory=list)


@dataclass(slots=True)
class ThenEval:
    node: ir.Node
    env: Env | None


@dataclass(slots=True)
class JoinFrame:
    span: Span
    count: int
    as_tuple: bool
    closure: Closure | None = None
    items: tuple[Value, ...] = ()
    limit: int = 0
    started: int = 0
    results: dict[int, Value] = field(default_factory=dict)
    failures: dict[int, RecordValue] = field(default_factory=dict)
    children: list[Fiber] = field(default_factory=list)


type Frame = (
    Seq | LetFrame | IfFrame | MatchFrame | AttemptFrame | RetryFrame | WaitFrame | WaitCheckFrame | LoopFrame
    | EachFrame | ThenEval | JoinFrame
)


@dataclass(slots=True)
class Fiber:
    path: tuple[int, ...]
    parent: tuple[JoinFrame, int] | None
    stack: list[Frame] = field(default_factory=list)
    node: ir.Node | None = None
    env: Env | None = None
    returning: Result | None = None
    raising: Raised | None = None
    waiting: Request | None = None
    waiting_type: Type | None = None
    call_span: Span | None = None
    joining: bool = False
    issued: bool = False
    done: bool = False
    cancelled: bool = False
    result: Value | None = None
    failure: Raised | None = None
    sequence: int = 0

    def next_id(self) -> str:
        self.sequence += 1
        return "r" + "".join(f".{index}" for index in self.path) + f":{self.sequence}"

    @property
    def runnable(self) -> bool:
        if self.done or self.cancelled or self.waiting is not None or self.joining:
            return False
        return self.node is not None or self.returning is not None or self.raising is not None


def request_order(request_id: str) -> tuple[tuple[int, ...], int]:
    path, _, sequence = request_id.partition(":")
    return tuple(int(part) for part in path.split(".")[1:]), int(sequence or 0)


class Machine:
    def __init__(
        self, program: ir.Program, inputs: Mapping[str, Value] | None = None, budgets: Budgets = DEFAULT_BUDGETS
    ) -> None:
        self.program = program
        self.budgets = budgets
        env: Env | None = None
        for name, value in (inputs or {}).items():
            env = Env(name, value, env)
        self.root = Fiber(path=(), parent=None, node=program.body, env=env)
        self.fibers: list[Fiber] = [self.root]
        self.issued: dict[str, Fiber] = {}
        self.trace: list[dict[str, object]] = []
        self.steps = 0
        self.calls = 0
        self.applications = 0
        self.delay = 0
        self.finished: Outcome | None = None

    def start(self) -> Outcome:
        return self.run()

    def resume(self, responses: Sequence[tuple[str, Response]]) -> Outcome:
        if self.finished is not None:
            return self.finished
        seen: set[str] = set()
        for request_id, _ in responses:
            if request_id in seen:
                return self.fail(invalid(f"The host answered request {request_id} twice.", request_id))
            if request_id not in self.issued:
                unknown = f"The host answered {request_id}, which is not an outstanding request."
                return self.fail(invalid(unknown, request_id))
            seen.add(request_id)
        missing = sorted(set(self.issued) - seen, key=request_order)
        if missing:
            return self.fail(invalid(f"The host did not answer request {missing[0]}.", missing[0]))
        try:
            for request_id, response in sorted(responses, key=lambda item: request_order(item[0])):
                fiber = self.issued.pop(request_id)
                self.trace.append({"response": request_id, **response_trace(response)})
                if not fiber.cancelled:
                    self.deliver(fiber, response)
        except _TerminalError as terminal:
            return self.fail(terminal.failure)
        return self.run()

    def fail(self, failure: Failure) -> Failed:
        self.finished = Failed(failure)
        return self.finished

    def deliver(self, fiber: Fiber, response: Response) -> None:
        request, output_type = fiber.waiting, fiber.waiting_type
        assert request is not None
        fiber.waiting, fiber.waiting_type, fiber.issued = None, None, False
        if isinstance(request, DelayRequest):
            if not isinstance(response, Output) or response.value is not None:
                raise _TerminalError(invalid(f"Delay request {request.id} must be answered with null.", request.id))
            fiber.returning = UNIT_VALUE
            return
        span = fiber.call_span or self.program.body.span
        if isinstance(response, Refusal):
            if response.kind not in CALL_ERROR_KINDS:
                raise _TerminalError(invalid(f"{request.id}: {response.kind!r} is not a call error kind.", request.id))
            fiber.raising = Raised(call_error(response.kind, response.message), span)
            return
        assert output_type is not None
        try:
            value = decode(response.value, output_type)
        except DecodeError as error:
            raise _TerminalError(invalid(f"{request.id} ({request.capability}): {error}", request.id)) from error
        self.check_size(value, span)
        fiber.returning = value

    def run(self) -> Outcome:
        if self.finished is not None:
            return self.finished
        try:
            while True:
                for fiber in sorted(self.fibers, key=lambda f: f.path):
                    while fiber.runnable:
                        self.step(fiber)
                self.settle_joins()
                if self.root.done:
                    return self.complete()
                if any(f.runnable for f in self.fibers):
                    continue
                requests = self.issue()
                if requests:
                    return Suspended(requests)
                return self.fail(Failure(FailureKind.INTERNAL, "No fiber can run and no request is outstanding."))
        except _TerminalError as terminal:
            return self.fail(terminal.failure)

    def complete(self) -> Outcome:
        if self.root.failure is not None:
            kind = self.root.failure.error.get("kind")
            message = self.root.failure.error.get("message")
            assert isinstance(kind, VariantValue)
            text = f"A tool call failed with {kind.variant}: {message}"
            return self.fail(Failure(FailureKind.CALL_FAILED, text, span=self.root.failure.span))
        result = self.root.result
        assert result is not None
        self.check_size(result, self.program.body.span)
        self.finished = Completed(result, encode(result, self.program.result))
        return self.finished

    def issue(self) -> tuple[Request, ...]:
        waiting = [f for f in self.fibers if f.waiting is not None and not f.issued and not f.cancelled]
        waiting.sort(key=lambda f: request_order(f.waiting.id) if f.waiting is not None else ((), 0))
        requests: list[Request] = []
        for fiber in waiting[: self.budgets.concurrency]:
            request = fiber.waiting
            assert request is not None
            fiber.issued = True
            self.issued[request.id] = fiber
            requests.append(request)
            if isinstance(request, CallRequest):
                self.trace.append({"request": request.id, "capability": request.capability, "input": request.input})
            else:
                self.trace.append({"delay": request.id, "milliseconds": request.milliseconds})
        return tuple(requests)

    def settle_joins(self) -> None:
        for fiber in list(self.fibers):
            if not fiber.joining:
                continue
            frame = fiber.stack[-1]
            assert isinstance(frame, JoinFrame)
            if frame.failures:
                for child in frame.children:
                    self.cancel(child)
                first = frame.failures[min(frame.failures)]
                self.close_join(fiber, frame)
                fiber.raising = Raised(first, frame.span)
                continue
            if frame.closure is not None:
                self.start_children(fiber, frame)
            if len(frame.results) == frame.count:
                ordered = tuple(frame.results[i] for i in range(frame.count))
                self.close_join(fiber, frame)
                fiber.returning = TupleValue(ordered) if frame.as_tuple else ListValue(ordered)

    def close_join(self, fiber: Fiber, frame: JoinFrame) -> None:
        fiber.joining = False
        fiber.stack.pop()
        finished = {id(child) for child in frame.children}
        self.fibers = [f for f in self.fibers if id(f) not in finished or f.issued]

    def cancel(self, fiber: Fiber) -> None:
        fiber.cancelled = True
        for frame in fiber.stack:
            if isinstance(frame, JoinFrame):
                for child in frame.children:
                    self.cancel(child)

    def start_children(self, fiber: Fiber, frame: JoinFrame) -> None:
        closure = frame.closure
        assert closure is not None
        active = sum(1 for child in frame.children if not child.done)
        while frame.started < frame.count and active < frame.limit:
            index = frame.started
            child = Fiber(path=(*fiber.path, index), parent=(frame, index))
            self.apply(child, closure, (frame.items[index],), frame.span)
            frame.children.append(child)
            self.fibers.append(child)
            frame.started += 1
            active += 1

    def finish(self, fiber: Fiber, value: Value) -> None:
        fiber.done, fiber.result = True, value
        if fiber.parent is not None:
            frame, index = fiber.parent
            frame.results[index] = value

    def fail_fiber(self, fiber: Fiber, raised: Raised) -> None:
        fiber.done, fiber.failure = True, raised
        if fiber.parent is not None:
            frame, index = fiber.parent
            frame.failures[index] = raised.error

    def step(self, fiber: Fiber) -> None:
        self.steps += 1
        if self.steps > self.budgets.steps:
            raise _TerminalError(budget(f"The program ran more than {self.budgets.steps} steps (steps budget).", None))
        if fiber.raising is not None:
            self.unwind(fiber)
            return
        if fiber.node is not None:
            node, env = fiber.node, fiber.env
            fiber.node = None
            self.evaluate(fiber, node, env)
            return
        value = fiber.returning
        fiber.returning = None
        assert value is not None
        if not fiber.stack:
            if isinstance(value, Closure | Recurring):
                raise _TerminalError(Failure(FailureKind.INTERNAL, "A fiber ended with a function or a recur."))
            self.finish(fiber, value)
            return
        self.resume_frame(fiber, fiber.stack.pop(), value)

    def check_size(self, value: Value, span: Span | None) -> None:
        if size(value) > self.budgets.value_size:
            raise _TerminalError(budget(f"A value grew past the value-size budget of {self.budgets.value_size}.", span))

    def evaluate(self, fiber: Fiber, node: ir.Node, env: Env | None) -> None:
        match node:
            case ir.Literal(value=value):
                fiber.returning = value
            case ir.Variable(name=name):
                found = lookup(env, name)
                if isinstance(found, LocalScope):
                    raise _TerminalError(Failure(FailureKind.INTERNAL, f"{name} is a local function used as a value."))
                fiber.returning = found
            case ir.Lambda(params=params, body=body, captured=captured):
                closure_env = env
                if captured is not None:
                    scope = lookup(env, captured)
                    assert isinstance(scope, LocalScope)
                    closure_env = scope.env
                fiber.returning = Closure(params, body, closure_env)
            case ir.Let(value=value):
                fiber.stack.append(LetFrame(node, env))
                fiber.node, fiber.env = value, env
            case ir.LocalFunction(name=name, body=body):
                fiber.node, fiber.env = body, Env(name, LocalScope(env), env)
            case ir.If(condition=condition):
                fiber.stack.append(IfFrame(node, env))
                fiber.node, fiber.env = condition, env
            case ir.Match(scrutinee=scrutinee):
                fiber.stack.append(MatchFrame(node, env))
                fiber.node, fiber.env = scrutinee, env
            case ir.Attempt(body=body):
                fiber.stack.append(AttemptFrame())
                fiber.node, fiber.env = body, env
            case ir.Retry(body=body):
                fiber.stack.append(RetryFrame(node, env))
                fiber.node, fiber.env = body, env
            case ir.WaitUntil(body=body):
                fiber.stack.append(WaitFrame(node, env))
                fiber.node, fiber.env = body, env
            case ir.Par(branches=branches, span=span):
                frame = JoinFrame(span, len(branches), as_tuple=True)
                fiber.stack.append(frame)
                fiber.joining = True
                for index, branch in enumerate(branches):
                    child = Fiber(path=(*fiber.path, index), parent=(frame, index), node=branch, env=env)
                    frame.children.append(child)
                    self.fibers.append(child)
            case _:
                children = sequence_children(node)
                frame = Seq(node, children, env)
                if not children:
                    self.finish_sequence(fiber, frame)
                    return
                fiber.stack.append(frame)
                fiber.node, fiber.env = children[0], env

    def resume_frame(self, fiber: Fiber, frame: Frame, value: Result) -> None:
        if isinstance(value, Recurring):
            if isinstance(frame, LoopFrame):
                self.recur(fiber, frame, value)
            else:
                fiber.returning = value
            return
        match frame:
            case Seq():
                frame.values.append(value)
                if len(frame.values) < len(frame.children):
                    fiber.stack.append(frame)
                    fiber.node, fiber.env = frame.children[len(frame.values)], frame.env
                else:
                    self.finish_sequence(fiber, frame)
            case LetFrame(node=node, env=env):
                fiber.node, fiber.env = node.body, bind(env, node.binder, plain(value))
            case IfFrame(node=node, env=env):
                fiber.node, fiber.env = (node.then if value is True else node.otherwise), env
            case MatchFrame(node=node, env=env):
                for pattern, body in node.clauses:
                    bound = match_value(pattern, plain(value), env)
                    if bound is not None:
                        fiber.node, fiber.env = body, bound[0]
                        return
                raise _TerminalError(Failure(FailureKind.INTERNAL, "No match clause matched.", span=node.span))
            case AttemptFrame():
                fiber.returning = OkValue(plain(value))
            case RetryFrame() | LoopFrame():
                fiber.returning = value
            case WaitFrame(node=node, env=env):
                predicate = node.predicate
                fiber.stack.append(WaitCheckFrame(frame, plain(value)))
                self.apply(fiber, Closure(predicate.params, predicate.body, env), (plain(value),), node.span)
            case WaitCheckFrame(wait=wait, value=checked):
                self.wait_again(fiber, wait, checked, value is True)
            case EachFrame():
                self.each(fiber, frame, plain(value))
            case ThenEval(node=node, env=env):
                fiber.node, fiber.env = node, env
            case JoinFrame():
                raise _TerminalError(Failure(FailureKind.INTERNAL, "A join frame received a value directly."))

    def recur(self, fiber: Fiber, frame: LoopFrame, value: Recurring) -> None:
        node = frame.node
        if frame.iteration >= node.maximum:
            message = f"This loop asked to recur after {node.maximum} iterations, its :max."
            raise _TerminalError(Failure(FailureKind.LOOP_LIMIT, message, span=node.span))
        frame.iteration += 1
        env = frame.env
        for binder, item in zip(node.binders, value.values, strict=True):
            env = bind(env, binder, item)
        fiber.stack.append(frame)
        fiber.node, fiber.env = node.body, env

    def wait_again(self, fiber: Fiber, wait: WaitFrame, value: Value, satisfied: bool) -> None:
        node = wait.node
        if satisfied:
            fiber.returning = value
            return
        if wait.round >= len(node.intervals):
            message = f"wait-until gave up after {sum(node.intervals)} ms."
            fiber.raising = Raised(call_error("Timeout", message), node.span)
            return
        delay = node.intervals[wait.round]
        wait.round += 1
        fiber.stack.append(wait)
        fiber.stack.append(ThenEval(node.body, wait.env))
        self.request_delay(fiber, delay, node.span)

    def unwind(self, fiber: Fiber) -> None:
        raised = fiber.raising
        assert raised is not None
        fiber.raising = None
        while fiber.stack:
            frame = fiber.stack.pop()
            if isinstance(frame, AttemptFrame):
                fiber.returning = ErrValue(raised.error)
                return
            if isinstance(frame, RetryFrame):
                kind = raised.error.get("kind")
                assert isinstance(kind, VariantValue)
                if kind.variant in frame.node.kinds and frame.attempt < len(frame.node.delays):
                    delay = frame.node.delays[frame.attempt]
                    frame.attempt += 1
                    fiber.stack.append(frame)
                    fiber.stack.append(ThenEval(frame.node.body, frame.env))
                    self.request_delay(fiber, delay, frame.node.span)
                    return
            if isinstance(frame, JoinFrame):
                raise _TerminalError(Failure(FailureKind.INTERNAL, "A failure reached a join frame."))
        self.fail_fiber(fiber, raised)

    def request_delay(self, fiber: Fiber, milliseconds: int, span: Span) -> None:
        self.delay += milliseconds
        if self.delay > self.budgets.delay_ms:
            message = f"The program asked for more than {self.budgets.delay_ms} ms of delays (delay budget)."
            raise _TerminalError(budget(message, span))
        fiber.waiting = DelayRequest(fiber.next_id(), milliseconds)

    def each(self, fiber: Fiber, frame: EachFrame, value: Value) -> None:
        op = frame.op
        if op == "reduce":
            frame.accumulator = value
        elif op in {"any?", "all?"}:
            if value is (op == "any?"):
                fiber.returning = op == "any?"
                return
        else:
            frame.results.append(value)
        frame.index += 1
        if frame.index < len(frame.items):
            fiber.stack.append(frame)
            self.apply(fiber, frame.closure, each_args(frame), frame.span)
            return
        fiber.returning = finish_each(frame)

    def apply(self, fiber: Fiber, closure: Closure, args: tuple[Value, ...], span: Span) -> None:
        self.applications += 1
        if self.applications > self.budgets.applications:
            limit = self.budgets.applications
            message = f"The program applied functions more than {limit} times (applications budget)."
            raise _TerminalError(budget(message, span))
        env = closure.env
        for binder, arg in zip(closure.params, args, strict=True):
            env = bind(env, binder, arg)
        fiber.node, fiber.env = closure.body, env

    def finish_sequence(self, fiber: Fiber, frame: Seq) -> None:
        node, values = frame.origin, frame.values
        match node:
            case ir.Prelude(op=op, span=span):
                self.prelude(fiber, op, values, span)
            case ir.CoreCall(function=function, span=span):
                if function.implementation is None:
                    message = f"{function.name} has a signature but no implementation."
                    raise _TerminalError(Failure(FailureKind.INTERNAL, message, span=span))
                result = function.implementation(*(plain(v) for v in values))
                self.check_size(result, span)
                fiber.returning = result
            case ir.Call(capability=capability, span=span):
                self.calls += 1
                if self.calls > self.budgets.calls:
                    message = f"The program made more than {self.budgets.calls} tool calls (calls budget)."
                    raise _TerminalError(budget(message, span))
                payload = encode(plain(values[0]), capability.input)
                fiber.waiting = CallRequest(fiber.next_id(), capability.id, payload)
                fiber.waiting_type = capability.output
                fiber.call_span = span
            case ir.Get(field=name):
                record = values[0]
                assert isinstance(record, RecordValue)
                fiber.returning = record.get(name)
            case ir.MakeRecord(fields=fields, span=span):
                built = RecordValue(tuple((name, plain(v)) for (name, _), v in zip(fields, values, strict=True)))
                self.check_size(built, span)
                fiber.returning = built
            case ir.MakeList(span=span):
                built_list = ListValue(tuple(plain(v) for v in values))
                self.check_size(built_list, span)
                fiber.returning = built_list
            case ir.MakeTuple():
                fiber.returning = TupleValue(tuple(plain(v) for v in values))
            case ir.Wrap(kind=kind):
                inner = plain(values[0])
                wrappers = {"some": SomeValue, "ok": OkValue, "err": ErrValue}
                fiber.returning = wrappers[kind](inner)
            case ir.MakeVariant(enum=enum, variant=variant):
                fiber.returning = VariantValue(enum, variant, plain(values[0]) if values else None)
            case ir.Coerce(conversion=conversion):
                fiber.returning = convert(plain(values[0]), conversion)
            case ir.ApplyLocal(function=function, name=name, span=span):
                scope = lookup(frame.env, name)
                assert isinstance(scope, LocalScope)
                closure = Closure(function.params, function.body, scope.env)
                self.apply(fiber, closure, tuple(plain(v) for v in values), span)
            case ir.Recur():
                fiber.returning = Recurring(tuple(plain(v) for v in values))
            case ir.Loop(binders=binders, body=body):
                env = frame.env
                for binder, value in zip(binders, values, strict=True):
                    env = bind(env, binder, plain(value))
                fiber.stack.append(LoopFrame(node, frame.env))
                fiber.node, fiber.env = body, env
            case ir.ParMap(limit=limit, span=span):
                closure, items = values
                assert isinstance(closure, Closure)
                assert isinstance(items, ListValue)
                count = len(items.items)
                join = JoinFrame(span, count, as_tuple=False, closure=closure, items=items.items, limit=limit)
                fiber.stack.append(join)
                fiber.joining = True
                self.start_children(fiber, join)
            case _:
                raise _TerminalError(Failure(FailureKind.INTERNAL, f"No evaluation rule for {type(node).__name__}."))

    def prelude(self, fiber: Fiber, op: str, values: list[Value | Closure], span: Span) -> None:
        if op not in HIGHER_ORDER:
            fiber.returning = pure_prelude(op, [plain(v) for v in values], span, self)
            return
        closure, items_value = values[0], values[-1]
        assert isinstance(closure, Closure)
        assert isinstance(items_value, ListValue)
        accumulator = plain(values[1]) if op == "reduce" else None
        frame = EachFrame(op, closure, items_value.items, span, accumulator=accumulator)
        if not frame.items:
            fiber.returning = finish_each(frame)
            return
        fiber.stack.append(frame)
        self.apply(fiber, closure, each_args(frame), span)


def invalid(message: str, request: str) -> Failure:
    return Failure(FailureKind.INVALID_HOST_RESPONSE, message, request=request)


def budget(message: str, span: Span | None) -> Failure:
    return Failure(FailureKind.BUDGET_EXHAUSTED, message, span=span)


def plain(value: Result | Value) -> Value:
    if isinstance(value, Closure | Recurring):
        raise _TerminalError(Failure(FailureKind.INTERNAL, "A function or recur reached a value position."))
    return value


def each_args(frame: EachFrame) -> tuple[Value, ...]:
    item = frame.items[frame.index]
    if frame.op == "reduce":
        assert frame.accumulator is not None
        return (frame.accumulator, item)
    return (item,)


def sequence_children(node: ir.Node) -> tuple[ir.Node, ...]:
    match node:
        case ir.Prelude(args=args) | ir.CoreCall(args=args) | ir.ApplyLocal(args=args) | ir.Recur(args=args):
            return args
        case ir.Call(argument=argument):
            return (argument,)
        case ir.Get(record=record):
            return (record,)
        case ir.MakeRecord(fields=fields):
            return tuple(value for _, value in fields)
        case ir.MakeList(items=items) | ir.MakeTuple(items=items):
            return items
        case ir.Wrap(value=value) | ir.Coerce(value=value):
            return (value,)
        case ir.MakeVariant(payload=payload):
            return () if payload is None else (payload,)
        case ir.Loop(initial=initial):
            return initial
        case ir.ParMap(function=function, items=items):
            return (function, items)
        case _:
            raise _TerminalError(Failure(FailureKind.INTERNAL, f"No evaluation rule for {type(node).__name__}."))


def response_trace(response: Response) -> dict[str, object]:
    if isinstance(response, Output):
        return {"output": response.value}
    return {"error": {"kind": response.kind, "message": response.message}}


def bind(env: Env | None, binder: ir.Binder, value: Value) -> Env | None:
    match binder:
        case ir.BindName(name=name):
            return Env(name, value, env)
        case ir.Discard():
            return env
        case ir.BindTuple(items=items):
            assert isinstance(value, TupleValue)
            for item, part in zip(items, value.items, strict=True):
                env = bind(env, item, part)
            return env


def match_value(pattern: ir.MatchPattern, value: Value, env: Env | None) -> tuple[Env | None] | None:
    match pattern:
        case ir.MatchAny(binder=binder):
            return (bind(env, binder, value),)
        case ir.MatchNone():
            return (env,) if isinstance(value, NoneValue) else None
        case ir.MatchLiteral(value=literal):
            return (env,) if same(value, literal) else None
        case ir.MatchSome(inner=inner):
            return match_value(inner, value.value, env) if isinstance(value, SomeValue) else None
        case ir.MatchOk(inner=inner):
            return match_value(inner, value.value, env) if isinstance(value, OkValue) else None
        case ir.MatchErr(inner=inner):
            return match_value(inner, value.value, env) if isinstance(value, ErrValue) else None
        case ir.MatchVariant(variant=variant, inner=inner):
            if not isinstance(value, VariantValue) or value.variant != variant:
                return None
            if inner is None:
                return (env,)
            assert value.payload is not None
            return match_value(inner, value.payload, env)


def convert(value: Value, conversion: ir.Conversion) -> Value:
    match conversion:
        case ir.VariantName():
            assert isinstance(value, VariantValue)
            return value.variant
        case ir.ToSome(inner=inner):
            return SomeValue(value if inner is None else convert(value, inner))
        case ir.EachField(fields=fields):
            assert isinstance(value, RecordValue)
            conversions = dict(fields)
            converted = []
            for name, item in value.fields:
                field_conversion = conversions.get(name)
                converted.append((name, item if field_conversion is None else convert(item, field_conversion)))
            return RecordValue(tuple(converted))
        case ir.WidenRecord(fields=widened):
            assert isinstance(value, RecordValue)
            return RecordValue(tuple((field.name, widen_field(value, field)) for field in widened))
        case ir.EachElement(element=element):
            assert isinstance(value, ListValue)
            return ListValue(tuple(convert(item, element) for item in value.items))
        case ir.EachItem(items=items):
            assert isinstance(value, TupleValue)
            parts = zip(value.items, items, strict=True)
            return TupleValue(tuple(item if c is None else convert(item, c) for item, c in parts))


def widen_field(record: RecordValue, field: ir.WidenedField) -> Value:
    if not field.present:
        return NONE_VALUE
    item = record.get(field.name)
    return item if field.conversion is None else convert(item, field.conversion)


def compare_keys(a: Value, b: Value) -> int:
    if isinstance(a, TupleValue | ListValue) and isinstance(b, TupleValue | ListValue):
        for x, y in zip(a.items, b.items, strict=False):
            order = compare_keys(x, y)
            if order != 0:
                return order
        return (len(a.items) > len(b.items)) - (len(a.items) < len(b.items))
    if isinstance(a, bool) and isinstance(b, bool):
        return int(a) - int(b)
    if isinstance(a, int) and isinstance(b, int):
        return (a > b) - (a < b)
    if isinstance(a, str) and isinstance(b, str):
        return (a > b) - (a < b)
    raise _TerminalError(Failure(FailureKind.INTERNAL, "A sort key is not an Int, String, Bool, or tuple."))


def finish_each(frame: EachFrame) -> Value:
    items, results = frame.items, frame.results
    match frame.op:
        case "map":
            return ListValue(tuple(results))
        case "filter":
            return ListValue(tuple(item for item, keep in zip(items, results, strict=True) if keep is True))
        case "keep":
            return ListValue(tuple(r.value for r in results if isinstance(r, SomeValue)))
        case "any?":
            return False
        case "all?":
            return True
        case "reduce":
            assert frame.accumulator is not None
            return frame.accumulator
        case "sort-by":
            order = sorted(range(len(items)), key=cmp_to_key(lambda i, j: compare_keys(results[i], results[j])))
            return ListValue(tuple(items[i] for i in order))
        case "distinct-by":
            seen: set[Value] = set()
            kept: list[Value] = []
            for item, key in zip(items, results, strict=True):
                if key not in seen:
                    seen.add(key)
                    kept.append(item)
            return ListValue(tuple(kept))
        case "group-by":
            groups: dict[Value, list[Value]] = {}
            for item, key in zip(items, results, strict=True):
                groups.setdefault(key, []).append(item)
            return ListValue(tuple(TupleValue((key, ListValue(tuple(group)))) for key, group in groups.items()))
    raise _TerminalError(Failure(FailureKind.INTERNAL, f"No rule to finish {frame.op}."))


def checked_int(value: int, span: Span) -> int:
    if not INT_MIN <= value <= INT_MAX:
        message = "Integer overflow: the result does not fit in a signed 64-bit Int."
        raise _TerminalError(Failure(FailureKind.ARITHMETIC, message, span=span))
    return value


def int_arg(value: Value) -> int:
    assert isinstance(value, int)
    assert not isinstance(value, bool)
    return value


def list_arg(value: Value) -> tuple[Value, ...]:
    assert isinstance(value, ListValue)
    return value.items


def map_arg(value: Value) -> MapValue:
    assert isinstance(value, MapValue)
    return value


def str_arg(value: Value) -> str:
    assert isinstance(value, str)
    return value


def same(a: Value, b: Value) -> bool:
    return type(a) is type(b) and a == b


def text_of(value: Value, span: Span) -> str:
    match value:
        case bool():
            return "true" if value else "false"
        case int() | str():
            return str(value)
        case VariantValue(variant=variant, payload=None):
            return variant
    raise _TerminalError(Failure(FailureKind.INTERNAL, f"str cannot write {show_value(value)}.", span))


def pure_prelude(op: str, args: list[Value], span: Span, machine: Machine) -> Value:
    match op:
        case "+":
            return checked_int(int_arg(args[0]) + int_arg(args[1]), span)
        case "-":
            if len(args) == 1:
                return checked_int(-int_arg(args[0]), span)
            return checked_int(int_arg(args[0]) - int_arg(args[1]), span)
        case "*":
            return checked_int(int_arg(args[0]) * int_arg(args[1]), span)
        case "quot" | "rem":
            a, b = int_arg(args[0]), int_arg(args[1])
            if b == 0:
                raise _TerminalError(Failure(FailureKind.ARITHMETIC, f"{op} by zero.", span=span))
            quotient = abs(a) // abs(b) * (1 if (a >= 0) == (b >= 0) else -1)
            return checked_int(quotient if op == "quot" else a - b * quotient, span)
        case "=":
            return same(args[0], args[1])
        case "not=":
            return not same(args[0], args[1])
        case "<":
            return compare_keys(args[0], args[1]) < 0
        case "<=":
            return compare_keys(args[0], args[1]) <= 0
        case ">":
            return compare_keys(args[0], args[1]) > 0
        case ">=":
            return compare_keys(args[0], args[1]) >= 0
        case "not":
            return args[0] is False
        case "and":
            return args[0] is True and args[1] is True
        case "or":
            return args[0] is True or args[1] is True
        case "str":
            joined = "".join(text_of(a, span) for a in args)
            machine.check_size(joined, span)
            return joined
        case "count":
            if isinstance(args[0], MapValue):
                return len(args[0].entries)
            return len(list_arg(args[0]))
        case "first":
            items = list_arg(args[0])
            if not items:
                raise _TerminalError(Failure(FailureKind.OUT_OF_RANGE, "first of an empty list.", span=span))
            return items[0]
        case "rest":
            return ListValue(list_arg(args[0])[1:])
        case "nth":
            items, index = list_arg(args[0]), int_arg(args[1])
            if not 0 <= index < len(items):
                message = f"nth {index} of a list of {len(items)}."
                raise _TerminalError(Failure(FailureKind.OUT_OF_RANGE, message, span=span))
            return items[index]
        case "take":
            count, items = int_arg(args[0]), list_arg(args[1])
            if count < 0:
                message = f"take with a negative count, {count}."
                raise _TerminalError(Failure(FailureKind.ARITHMETIC, message, span=span))
            return ListValue(items[:count])
        case "concat":
            joined_list = ListValue(list_arg(args[0]) + list_arg(args[1]))
            machine.check_size(joined_list, span)
            return joined_list
        case "flatten":
            return ListValue(tuple(item for inner in list_arg(args[0]) for item in list_arg(inner)))
        case "contains?":
            if isinstance(args[0], MapValue):
                return args[0].has(str_arg(args[1]))
            return any(same(item, args[1]) for item in list_arg(args[0]))
        case "map-get":
            found = map_arg(args[0]).get(str_arg(args[1]))
            return NONE_VALUE if found is None else SomeValue(found)
        case "assoc":
            assigned = make_map([*map_arg(args[0]).entries, (str_arg(args[1]), args[2])])
            machine.check_size(assigned, span)
            return assigned
        case "dissoc":
            return MapValue(tuple(entry for entry in map_arg(args[0]).entries if entry[0] != str_arg(args[1])))
        case "keys":
            return ListValue(tuple(key for key, _ in map_arg(args[0]).entries))
        case "vals":
            return ListValue(tuple(item for _, item in map_arg(args[0]).entries))
        case "entries":
            return ListValue(tuple(TupleValue((key, item)) for key, item in map_arg(args[0]).entries))
        case "into-map":
            pairs: list[tuple[str, Value]] = []
            for pair in list_arg(args[0]):
                assert isinstance(pair, TupleValue)
                pairs.append((str_arg(pair.items[0]), pair.items[1]))
            built = make_map(pairs)
            machine.check_size(built, span)
            return built
    raise _TerminalError(Failure(FailureKind.INTERNAL, f"No prelude rule for {op}.", span=span))
