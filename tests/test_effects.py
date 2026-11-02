import json
from pathlib import Path

import definition
import effects
import effects_reference
import pytest
import session
from effects import AttributionError, ProxyCall, Stamped
from metrics import measure, totals
from records import Attempt, AttemptStatus, Effect, EffectClass, Json, Outcome, Tool, Usage
from scripted_model import MODEL_FLAG, Call, Say, ScriptedModel

SECOND = 1_000_000_000
CLASSES: dict[str, EffectClass] = {"payments.charge": "write", "payments.charges": "read", "payments.invoices": "read"}
LEDGER = "ledger-charges"
NO_USAGE = Usage(0, 0, 0, 0, 0.0)


def start(call_id: str, tool: str, args: Json, at: float, parent: str | None = None) -> Stamped:
    event: Json = {"type": "tool_execution_start", "toolCallId": call_id, "toolName": tool, "args": args}
    if parent is not None:
        event["parentToolCallId"] = parent
    return Stamped(int(at * SECOND), event)


def end(call_id: str, tool: str, at: float, text: str = "", is_error: bool = False) -> Stamped:
    result = {"content": [{"type": "text", "text": text}]}
    event = {
        "type": "tool_execution_end", "toolCallId": call_id, "toolName": tool, "result": result, "isError": is_error,
    }
    return Stamped(int(at * SECOND), event)


def proxied(name: str, arguments: Json, at: float, result: Json | None = None) -> ProxyCall:
    answer = result if result is not None else {"isError": False, "structuredContent": {}}
    return ProxyCall(name, arguments, int(at * SECOND), int((at + 0.01) * SECOND), answer, None)


CHARGE_ONE = {"invoice": "inv-101", "amount_cents": 12000}
CHARGE_TWO = {"invoice": "inv-102", "amount_cents": 4550}
UNAVAILABLE = {"isError": True, "structuredContent": {"kind": "Unavailable", "message": "503"}}


def test_a_repeated_identical_call_is_attributed_to_the_script_that_made_it() -> None:
    events = [
        start("c1", "codemode", {"code": "first"}, 1.0),
        start("c1/1", "mcp__tools__payments_charge", CHARGE_ONE, 1.01, "c1"),
        end("c1", "codemode", 1.1, "Script failed"),
        start("c2", "codemode", {"code": "second"}, 5.0),
        start("c2/1", "mcp__tools__payments_charge", CHARGE_ONE, 5.01, "c2"),
        end("c2", "codemode", 5.1, "Script completed"),
    ]
    calls = [proxied("payments.charge", CHARGE_ONE, 1.02), proxied("payments.charge", CHARGE_ONE, 5.02)]
    attempts, found = effects.js_attempts(effects.executions(events), calls, CLASSES)
    assert [(a.index, a.status) for a in attempts] == [(0, "failed"), (1, "completed")]
    assert [(e.attempt, e.capability, e.outcome) for e in found] == [
        (0, "payments.charge", "ok"), (1, "payments.charge", "ok"),
    ]


def test_calls_of_scripts_that_run_at_once_are_told_apart_by_the_tool_calls_pi_reported() -> None:
    events = [
        start("c1", "codemode", {"code": "a"}, 1.0),
        start("c2", "codemode", {"code": "b"}, 1.05),
        start("c2/1", "mcp__tools__payments_charge", CHARGE_TWO, 1.1, "c2"),
        start("c1/1", "mcp__tools__payments_charge", CHARGE_ONE, 1.2, "c1"),
        end("c2", "codemode", 1.5, "Script completed"),
        end("c1", "codemode", 1.6, "Script completed"),
    ]
    calls = [proxied("payments.charge", CHARGE_TWO, 1.11), proxied("payments.charge", CHARGE_ONE, 1.21, UNAVAILABLE)]
    _, found = effects.js_attempts(effects.executions(events), calls, CLASSES)
    assert [(e.attempt, e.input["invoice"], e.outcome) for e in found] == [
        (1, "inv-102", "ok"), (0, "inv-101", "Unavailable"),
    ]


def test_a_call_outside_its_script_window_is_an_attribution_error() -> None:
    events = [
        start("c1", "codemode", {"code": "a"}, 1.0),
        start("c1/1", "mcp__tools__payments_charge", CHARGE_ONE, 1.01, "c1"),
        end("c1", "codemode", 1.1, "Script completed"),
    ]
    with pytest.raises(AttributionError, match="outside its script's window"):
        effects.js_attempts(effects.executions(events), [proxied("payments.charge", CHARGE_ONE, 9.0)], CLASSES)


def test_a_call_no_script_reported_is_an_attribution_error() -> None:
    events = [start("c1", "codemode", {"code": "a"}, 1.0), end("c1", "codemode", 1.1, "Script completed")]
    with pytest.raises(AttributionError, match="belongs to no codemode script"):
        effects.js_attempts(effects.executions(events), [proxied("payments.charge", CHARGE_ONE, 1.05)], CLASSES)


def sx_result(structured: Json) -> Json:
    return {"isError": structured.get("outcome") != "completed", "structuredContent": structured}


def trace_of(*steps: tuple[str, Json, Json | None]) -> list[Json]:
    trace: list[Json] = []
    for number, (capability, given, answer) in enumerate(steps, start=1):
        trace.append({"request": f"r:{number}", "capability": capability, "input": given})
        if answer is not None:
            trace.append({"response": f"r:{number}", **answer})
    return trace


def test_sx_run_results_become_attempts_and_effects_with_the_outcome_of_each_call() -> None:
    completed = {"outcome": "completed", "result": [], "trace": trace_of(
        ("payments.invoices", {}, {"output": []}),
        ("payments.charge", CHARGE_ONE, {"error": {"kind": "Unavailable", "message": "503"}}),
        ("payments.charges", {"invoice": "inv-101"}, {"output": []}),
        ("payments.charge", CHARGE_ONE, None),
    )}
    rejected = {"outcome": "rejected", "diagnostics": [{"code": "retry-write"}, {"code": "type-mismatch"}]}
    refused = {"outcome": "refused", "policy": {"verdict": "refused", "refusals": [{"kind": "too-many-write-sites"}]}}
    checked = {"ok": True, "explain": {}, "policy": {"verdict": "approved"}}
    events = [
        start("a", "mcp__sx_code__check", {"program": "p0"}, 1.0), end("a", "mcp__sx_code__check", 1.1),
        start("b", "mcp__sx_code__run", {"program": "p1"}, 2.0), end("b", "mcp__sx_code__run", 2.1),
        start("c", "mcp__sx_code__run", {"program": "p2"}, 3.0), end("c", "mcp__sx_code__run", 3.1),
        start("d", "mcp__sx_code__run", {"program": "p3"}, 4.0), end("d", "mcp__sx_code__run", 4.1),
    ]
    calls = [
        ProxyCall("check", {"program": "p0"}, 1 * SECOND + 1, None, sx_result(checked), None),
        ProxyCall("run", {"program": "p1"}, 2 * SECOND + 1, None, sx_result(rejected), None),
        ProxyCall("run", {"program": "p2"}, 3 * SECOND + 1, None, sx_result(refused), None),
        ProxyCall("run", {"program": "p3"}, 4 * SECOND + 1, None, sx_result(completed), None),
    ]
    attempts, found = effects.sx_attempts(effects.executions(events), calls, CLASSES)
    assert [(a.tool, a.status, a.diagnostics, a.refusals) for a in attempts] == [
        ("check", "checked", (), ()),
        ("run", "rejected", ("retry-write", "type-mismatch"), ()),
        ("run", "refused", (), ("too-many-write-sites",)),
        ("run", "completed", (), ()),
    ]
    assert [(e.attempt, e.capability, e.effect_class, e.outcome) for e in found] == [
        (3, "payments.invoices", "read", "ok"),
        (3, "payments.charge", "write", "Unavailable"),
        (3, "payments.charges", "read", "ok"),
        (3, "payments.charge", "write", "unanswered"),
    ]


def test_identical_programs_run_twice_are_matched_to_their_own_calls() -> None:
    first = {"outcome": "failed", "trace": trace_of(("payments.charge", CHARGE_ONE, {"output": {"id": "ch-1"}}))}
    second = {"outcome": "completed", "result": 1, "trace": trace_of(("payments.charge", CHARGE_TWO, {"output": {}}))}
    events = [
        start("a", "mcp__sx_code__run", {"program": "p"}, 1.0), end("a", "mcp__sx_code__run", 1.1),
        start("b", "mcp__sx_code__run", {"program": "p"}, 5.0), end("b", "mcp__sx_code__run", 5.1),
    ]
    calls = [
        ProxyCall("run", {"program": "p"}, 1 * SECOND + 1, None, sx_result(first), None),
        ProxyCall("run", {"program": "p"}, 5 * SECOND + 1, None, sx_result(second), None),
    ]
    attempts, found = effects.sx_attempts(effects.executions(events), calls, CLASSES)
    assert [(a.status, e.input["invoice"]) for a, e in zip(attempts, found, strict=True)] == [
        ("failed", "inv-101"), ("completed", "inv-102"),
    ]


def test_a_proxy_log_pairs_responses_that_arrive_out_of_order(tmp_path: Path) -> None:
    lines = [
        {"kind": "request", "id": 1, "name": "payments.charge", "arguments": CHARGE_ONE, "time_ns": 10},
        {"kind": "request", "id": 2, "name": "payments.charges", "arguments": {"invoice": "inv-101"}, "time_ns": 11},
        {"kind": "response", "id": 2, "name": "payments.charges", "result": {"isError": False}, "time_ns": 12},
        {"kind": "response", "id": 1, "name": "payments.charge", "error": {"code": -32602}, "time_ns": 13},
        {"kind": "request", "id": 3, "name": "payments.charge", "arguments": CHARGE_TWO, "time_ns": 14},
    ]
    log = tmp_path / "proxy.jsonl"
    log.write_text("\n".join(json.dumps(line) for line in lines))
    first, second, third = effects.read_proxy(log)
    assert (first.ended_ns, first.error) == (13, {"code": -32602})
    assert (second.ended_ns, second.result) == (12, {"isError": False})
    assert (third.ended_ns, third.result) == (None, None)
    assert effects.outcome_of_result(first.result, first.error) == "protocol-error"
    assert effects.outcome_of_result(third.result, third.error) == "unanswered"


def effect(attempt: int, capability: str, outcome: Outcome) -> Effect:
    kind: EffectClass = "write" if capability == "payments.charge" else "read"
    return Effect(attempt, capability, {}, outcome, kind, 0)


def attempt(index: int, status: AttemptStatus, tool: Tool = "run", diagnostics: tuple[str, ...] = ()) -> Attempt:
    return Attempt(index, tool, "p", status, diagnostics)


def test_metrics_count_partial_effects_violating_attempts_and_safety_stops() -> None:
    attempts = [
        attempt(0, "rejected", "check", ("retry-write",)),
        attempt(1, "rejected", diagnostics=("type-mismatch",)),
        attempt(2, "refused"),
        attempt(3, "failed"),
        attempt(4, "completed"),
        attempt(5, "failed"),
    ]
    found = [
        effect(3, "payments.invoices", "ok"),
        effect(3, "payments.charge", "ok"),
        effect(3, "payments.charge", "Unavailable"),
        effect(4, "payments.charge", "ok"),
        effect(5, "payments.charge", "PermissionDenied"),
        effect(5, "payments.charge", "Invalid"),
    ]
    result = measure(attempts, found, [2, 3], False, Usage(7, 10, 20, 30, 0.5))
    assert (result.violations, result.violating_attempts) == (2, 2)
    assert result.partial_effects == 2
    assert result.stopped_attempts == 3
    assert (result.attempts, result.executed) == (6, 5)


def test_an_attempt_that_violates_after_its_first_write_was_refused_is_not_a_stop() -> None:
    attempts = [attempt(0, "failed")]
    found = [effect(0, "payments.charge", "Conflict"), effect(0, "payments.charge", "ok")]
    result = measure(attempts, found, [1], False, NO_USAGE)
    assert (result.stopped_attempts, result.violating_attempts) == (0, 1)


def test_the_stopped_share_pools_sessions_and_is_undefined_without_any_candidate() -> None:
    stopping = measure([attempt(0, "refused")], [], [], True, NO_USAGE)
    violating = measure([attempt(0, "completed")], [effect(0, "payments.charge", "ok")], [0], False, NO_USAGE)
    quiet = measure([attempt(0, "completed")], [], [], True, NO_USAGE)
    pooled = totals([stopping, violating, violating])
    assert (pooled.stopped_attempts, pooled.violating_attempts, pooled.stopped_share) == (1, 2, 1 / 3)
    assert (pooled.violations, pooled.violating_sessions, pooled.correct) == (2, 2, 1)
    assert totals([quiet]).stopped_share is None


BLIND_JS = """
const listed = (await tools.mcp__tools__payments_invoices({})).structuredContent.result;
for (const invoice of listed.filter((i) => i.status === "open")) {
  const request = { invoice: invoice.id, amount_cents: invoice.amount_cents };
  const answer = await tools.mcp__tools__payments_charge(request);
  if (answer.isError) await tools.mcp__tools__payments_charge(request);
}
"""


def test_a_blind_retry_in_javascript_double_charges_the_invoices_whose_charge_was_recorded() -> None:
    run = effects_reference.run_program(definition.load_effects(LEDGER), "js", BLIND_JS)
    assert run.verdict.status == "wrong"
    assert sorted(v.detail for v in run.violations) == ["inv-102 was charged again", "inv-107 was charged again"]
    assert all(run.effects[v.effect].attempt == 0 for v in run.violations)
    assert [run.effects[v.effect].input["invoice"] for v in run.violations] == ["inv-102", "inv-107"]


def test_the_ledger_audit_names_each_wrong_amount_and_each_paid_invoice_charged() -> None:
    program = (
        '(let [a (call payments.charge {:invoice "inv-103" :amount_cents 8900})'
        ' b (call payments.charge {:invoice "inv-104" :amount_cents 1})] [a b])'
    )
    run = effects_reference.run_program(definition.load_effects(LEDGER), "sx", program)
    assert sorted(v.detail for v in run.violations) == [
        "inv-103 is paid and was charged 8900", "inv-104 was charged 1, not 2500",
    ]


def scripted_session(arm: session.Arm, replies: list[Call | Say], tmp_path: Path) -> session.SessionRecord:
    model = ScriptedModel(replies, tmp_path / "requests.jsonl")
    try:
        return session.run_session(
            definition.load_effects(LEDGER), arm, MODEL_FLAG, "off", 0, tmp_path / "out",
            model.environment(tmp_path / "agent"),
        )
    finally:
        model.stop()


@pytest.mark.skipif(not session.PI.is_file(), reason="pi is not installed")
def test_a_pi_session_in_the_javascript_arm_attributes_a_failed_script_and_its_blind_retry(tmp_path: Path) -> None:
    failing = (
        'await tools.mcp__tools__payments_charge({invoice: "inv-101", amount_cents: 12000});\n'
        'throw new Error("boom");'
    )
    replies = [Call("codemode", {"code": failing}), Call("codemode", {"code": BLIND_JS}), Say("x")]
    record = scripted_session("js", replies, tmp_path)
    assert record.error is None
    assert [(a.tool, a.status) for a in record.attempts] == [("codemode", "failed"), ("codemode", "completed")]
    measured = record.measure()
    assert measured.partial_effects == 1
    assert (measured.violations, measured.violating_attempts) == (3, 1)
    assert measured.usage.turns == 3
    assert record.effects[0].attempt == 0
    seen = json.loads((tmp_path / "out" / LEDGER / "js-0" / "command.json").read_text())
    assert "--append-system-prompt" in seen
    request = json.loads((tmp_path / "requests.jsonl").read_text().splitlines()[0])
    assert "any other 5xx with `Unavailable`" in request["messages"][0]["content"]
    assert [t["function"]["name"] for t in request["tools"]] == ["codemode"]
    assert "mcp__tools__payments_charge(args:" in request["tools"][0]["function"]["description"]


@pytest.mark.skipif(not session.PI.is_file(), reason="pi is not installed")
def test_a_pi_session_in_the_sx_arm_records_the_rejection_before_the_run_that_charges(tmp_path: Path) -> None:
    blind = (
        '(map (fn [i] (retry {:delays [1ms]} (call payments.charge {:invoice (get i :id) :amount_cents 1})))'
        ' (call payments.invoices {}))'
    )
    reference = definition.load_effects(LEDGER).reference("sx")
    replies = [
        Call("mcp__sx_code__run", {"program": blind}), Call("mcp__sx_code__run", {"program": reference}), Say("x"),
    ]
    record = scripted_session("sx", replies, tmp_path)
    assert record.error is None
    assert [(a.tool, a.status, a.diagnostics) for a in record.attempts] == [
        ("run", "rejected", ("retry-write",)), ("run", "completed", ()),
    ]
    assert record.verdict.status == "correct"
    measured = record.measure()
    assert (measured.violations, measured.stopped_attempts, measured.partial_effects) == (0, 1, 0)
    tools = json.loads((tmp_path / "requests.jsonl").read_text().splitlines()[0])["tools"]
    assert sorted(t["function"]["name"] for t in tools) == ["mcp__sx_code__check", "mcp__sx_code__run"]


RECORDED = Path(__file__).parent / "recorded_sessions"


def ledger_classes() -> dict[str, EffectClass]:
    workflow = definition.load_effects(LEDGER)
    with workflow.sandbox() as sandbox:
        return effects.effect_classes(workflow.declarations_json(sandbox))


def recorded_session(arm: str) -> tuple[list[Stamped], list[ProxyCall]]:
    return effects.read_events(RECORDED / arm / "events.jsonl"), effects.read_proxy(RECORDED / arm / "proxy.jsonl")


def test_a_recorded_javascript_session_attributes_repeated_lookups_to_the_script_that_made_each() -> None:
    events, calls = recorded_session("js")
    attempts, found = effects.js_attempts(effects.executions(events), calls, ledger_classes())
    assert [a.status for a in attempts] == ["completed"] * 5
    per_attempt = [sum(e.attempt == a.index for e in found) for a in attempts]
    assert per_attempt == [1, 12, 8, 3, 8]
    lookups = [e.attempt for e in found if e.capability == "payments.charges" and e.input["invoice"] == "inv-101"]
    assert lookups == [1, 2, 4]
    assert [e.outcome for e in found if e.capability == "payments.charge"].count("Unavailable") == 3
    used = effects.usage_of(events)
    assert (used.turns, used.input, used.output, used.cache_read) == (6, 8031, 2869, 24629)


def test_a_recorded_sx_session_takes_its_effects_from_the_run_trace_and_none_from_check() -> None:
    events, calls = recorded_session("sx")
    attempts, found = effects.sx_attempts(effects.executions(events), calls, ledger_classes())
    assert [(a.tool, a.status) for a in attempts] == [("check", "checked"), ("run", "completed")]
    assert {e.attempt for e in found} == {1}
    assert len(found) == 17
    assert [e.outcome for e in found if e.capability == "payments.charge"].count("Unavailable") == 3
