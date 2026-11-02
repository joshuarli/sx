import json
import re
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

from records import (
    ATTEMPT_STATUSES,
    CALL_KINDS,
    EFFECT_CLASSES,
    OUTCOMES,
    Attempt,
    AttemptStatus,
    Effect,
    EffectClass,
    Json,
    Outcome,
    Tool,
    Usage,
    narrowed,
)

SX_TOOLS: dict[str, Tool] = {"mcp__sx_code__check": "check", "mcp__sx_code__run": "run"}
CODEMODE = "codemode"
JS_SERVER = "tools"
SLACK_NS = 250_000_000
FINISHED_WORDS: dict[str, AttemptStatus] = {"Script completed": "completed", "Script failed": "failed"}


class AttributionError(ValueError):
    pass


@dataclass(frozen=True, slots=True)
class Stamped:
    time_ns: int
    event: Json


@dataclass(frozen=True, slots=True)
class Execution:
    call_id: str
    parent: str | None
    tool: str
    args: Json
    started_ns: int
    ended_ns: int | None
    result: Json | None
    failed: bool

    def window_holds(self, time_ns: int) -> bool:
        ended = self.ended_ns if self.ended_ns is not None else time_ns
        return self.started_ns - SLACK_NS <= time_ns <= ended + SLACK_NS


@dataclass(frozen=True, slots=True)
class ProxyCall:
    name: str
    arguments: Json
    started_ns: int
    ended_ns: int | None
    result: Json | None
    error: Json | None


def read_events(path: Path) -> list[Stamped]:
    return [Stamped(entry["time_ns"], entry["event"]) for entry in map(json.loads, path.read_text().splitlines())]


def read_proxy(path: Path) -> list[ProxyCall]:
    requests: list[Json] = []
    answers: dict[int, Json] = {}
    waiting: dict[str, int] = {}
    for entry in map(json.loads, path.read_text().splitlines()):
        key = json.dumps(entry["id"])
        if entry["kind"] == "request":
            waiting[key] = len(requests)
            requests.append(entry)
        else:
            answers[waiting.pop(key)] = entry
    return [
        ProxyCall(
            name=request["name"],
            arguments=request.get("arguments") or {},
            started_ns=request["time_ns"],
            ended_ns=answers[index]["time_ns"] if index in answers else None,
            result=answers[index].get("result") if index in answers else None,
            error=answers[index].get("error") if index in answers else None,
        )
        for index, request in enumerate(requests)
    ]


def executions(events: list[Stamped]) -> list[Execution]:
    started: dict[str, Stamped] = {}
    ended: dict[str, Stamped] = {}
    for stamped in events:
        kind = stamped.event.get("type")
        if kind == "tool_execution_start":
            started[stamped.event["toolCallId"]] = stamped
        elif kind == "tool_execution_end":
            ended[stamped.event["toolCallId"]] = stamped
    found: list[Execution] = []
    for call_id, begin in started.items():
        end = ended.get(call_id)
        found.append(Execution(
            call_id=call_id,
            parent=begin.event.get("parentToolCallId"),
            tool=begin.event["toolName"],
            args=begin.event.get("args") or {},
            started_ns=begin.time_ns,
            ended_ns=end.time_ns if end is not None else None,
            result=end.event.get("result") if end is not None else None,
            failed=bool(end.event.get("isError")) if end is not None else True,
        ))
    return sorted(found, key=lambda e: e.started_ns)


def replies(events: list[Stamped]) -> list[Json]:
    return [
        stamped.event["message"]
        for stamped in events
        if stamped.event.get("type") == "message_end" and stamped.event["message"]["role"] == "assistant"
    ]


def usage_of(events: list[Stamped]) -> Usage:
    turns = input_tokens = output = cache_read = 0
    cost = 0.0
    for message in replies(events):
        used = message["usage"]
        turns += 1
        input_tokens += int(used["input"])
        output += int(used["output"])
        cache_read += int(used["cacheRead"])
        cost += float(used["cost"]["total"])
    return Usage(turns, input_tokens, output, cache_read, cost)


def session_error(events: list[Stamped]) -> str | None:
    for message in replies(events):
        if message.get("stopReason") in ("error", "aborted"):
            return f"the model stopped with {message['stopReason']}: {message.get('errorMessage', '')}"
    return None


FS_PACK_CLASSES: dict[str, EffectClass] = {
    "fs.read-text": "read",
    "fs.read-bytes": "read",
    "fs.list": "read",
    "fs.stat": "read",
    "fs.write-text": "write",
    "fs.make-directory": "write",
    "fs.move": "write",
    "fs.delete": "write",
}


def effect_classes(declarations: Json) -> dict[str, EffectClass]:
    declared = {
        capability["id"]: narrowed(capability["effect"], EFFECT_CLASSES)
        for capability in declarations.get("capabilities", [])
    }
    return {**(FS_PACK_CLASSES if "fs" in declarations.get("packs", []) else {}), **declared}


def take[T](items: list[T], wanted: Callable[[T], bool]) -> T | None:
    for position, item in enumerate(items):
        if wanted(item):
            return items.pop(position)
    return None


def outcome_of_result(result: Json | None, error: Json | None) -> Outcome:
    if result is None or error is not None:
        return "unanswered" if error is None else "protocol-error"
    if not result.get("isError"):
        return "ok"
    kind = (result.get("structuredContent") or {}).get("kind")
    return narrowed(kind, CALL_KINDS) if kind in CALL_KINDS else "protocol-error"


def trace_effects(attempt: int, started_ns: int, trace: list[Json], classes: dict[str, EffectClass]) -> list[Effect]:
    answers = {entry["response"]: entry for entry in trace if "response" in entry}
    found: list[Effect] = []
    for entry in trace:
        if "request" not in entry or "capability" not in entry:
            continue
        answer = answers.get(entry["request"])
        outcome: Outcome = "unanswered"
        if answer is not None:
            outcome = narrowed(answer["error"]["kind"], OUTCOMES) if "error" in answer else "ok"
        found.append(Effect(
            attempt, entry["capability"], entry["input"], outcome, classes[entry["capability"]], started_ns,
        ))
    return found


def diagnostic_codes(structured: Json) -> tuple[str, ...]:
    return tuple(str(d["code"]) for d in structured.get("diagnostics", []))


def refusal_kinds(policy: Json) -> tuple[str, ...]:
    return tuple(str(r["kind"]) for r in policy.get("refusals", []))


def sx_status(tool: Tool, structured: Json) -> tuple[AttemptStatus, tuple[str, ...], tuple[str, ...]]:
    if tool == "run":
        outcome = narrowed(structured["outcome"], ATTEMPT_STATUSES)
        return outcome, diagnostic_codes(structured), refusal_kinds(structured.get("policy", {}))
    if "diagnostics" in structured:
        return "rejected", diagnostic_codes(structured), ()
    policy: Json = structured["policy"]
    if policy["verdict"] == "refused":
        return "refused", (), refusal_kinds(policy)
    return "checked", (), ()


def sx_call_of(execution: Execution, tool: Tool, program: str) -> Callable[[ProxyCall], bool]:
    def matches(call: ProxyCall) -> bool:
        same_program = str(call.arguments.get("program")) == program
        return call.name == tool and same_program and execution.window_holds(call.started_ns)

    return matches


def sx_attempts(
    runs: list[Execution], calls: list[ProxyCall], classes: dict[str, EffectClass]
) -> tuple[list[Attempt], list[Effect]]:
    unmatched = list(calls)
    attempts: list[Attempt] = []
    effects: list[Effect] = []
    ours = [e for e in runs if e.parent is None and e.tool in SX_TOOLS]
    for index, execution in enumerate(ours):
        tool = SX_TOOLS[execution.tool]
        program = str(execution.args.get("program", ""))
        call = take(unmatched, sx_call_of(execution, tool, program))
        ended = execution.ended_ns or execution.started_ns
        answer = call.result if call is not None else None
        if call is None or answer is None:
            if not execution.failed:
                raise AttributionError(f"the {tool} execution {execution.call_id} has no recorded server call")
            attempts.append(Attempt(index, tool, program, "failed", started_ns=execution.started_ns, ended_ns=ended))
            continue
        structured: Json = answer["structuredContent"]
        status, codes, refusals = sx_status(tool, structured)
        attempts.append(Attempt(index, tool, program, status, codes, refusals, execution.started_ns, ended))
        if tool == "run":
            effects += trace_effects(index, call.started_ns, structured.get("trace", []), classes)
    if unmatched:
        raise AttributionError(f"{len(unmatched)} recorded calls belong to no tool execution: {unmatched[0].name}")
    return attempts, effects


def pi_name(capability: str) -> str:
    return f"mcp__{JS_SERVER}__" + re.sub(r"[^A-Za-z0-9_]", "_", capability)


def script_status(execution: Execution) -> AttemptStatus:
    if execution.result is None:
        return "failed"
    for part in execution.result.get("content", []):
        text = part.get("text", "")
        for words, status in FINISHED_WORDS.items():
            if text.startswith(words):
                return status
    return "failed"


def nested_of(call: ProxyCall) -> Callable[[Execution], bool]:
    def matches(execution: Execution) -> bool:
        return execution.tool == pi_name(call.name) and execution.args == call.arguments

    return matches


def js_attempts(
    runs: list[Execution], calls: list[ProxyCall], classes: dict[str, EffectClass]
) -> tuple[list[Attempt], list[Effect]]:
    scripts = [e for e in runs if e.parent is None and e.tool == CODEMODE]
    index_of = {script.call_id: index for index, script in enumerate(scripts)}
    nested = [e for e in runs if e.parent in index_of]
    attempts = [
        Attempt(
            index, "codemode", str(script.args.get("code", "")), script_status(script),
            started_ns=script.started_ns, ended_ns=script.ended_ns or script.started_ns,
        )
        for index, script in enumerate(scripts)
    ]
    effects: list[Effect] = []
    for call in sorted(calls, key=lambda c: c.started_ns):
        owner = take(nested, nested_of(call))
        if owner is None or owner.parent is None:
            raise AttributionError(f"the recorded call {call.name} {call.arguments} belongs to no codemode script")
        script = scripts[index_of[owner.parent]]
        if not script.window_holds(call.started_ns):
            raise AttributionError(f"the recorded call {call.name} {call.arguments} is outside its script's window")
        outcome = outcome_of_result(call.result, call.error)
        effects.append(Effect(
            index_of[owner.parent], call.name, call.arguments, outcome, classes[call.name], call.started_ns,
        ))
    return attempts, effects
