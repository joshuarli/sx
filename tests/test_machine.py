from collections.abc import Mapping
from typing import Any

import pytest

from sx import ir
from sx.checker import Budgets
from sx.host import FixtureHost, run
from sx.machine import (
    CallRequest,
    Completed,
    DelayRequest,
    Failed,
    FailureKind,
    Machine,
    Output,
    Refusal,
    Suspended,
)
from sx.program import compile_source
from sx.registry import Registry, load_declarations


def capability(identifier: str, effect: str, input_type: str, output: str, **extra: bool) -> dict[str, Any]:
    return {"id": identifier, "effect": effect, "input": input_type, "output": output, "description": "-", **extra}


DECLARATIONS: Mapping[str, Any] = {
    "types": {"Item": "Record {id: String, size: Int}"},
    "capabilities": [
        capability("store.get", "read", "Record {id: String}", "Item"),
        capability("store.list", "read", "Record {}", "List<String>"),
        capability("store.put", "write", "Record {id: String, size: Int}", "Unit", idempotent=True),
        capability("mail.send", "write", "Record {to: String}", "Unit", idempotent=False),
        capability("job.state", "read", "Record {id: String}", "String"),
    ],
}
ALL: frozenset[str] = frozenset(str(c["id"]) for c in DECLARATIONS["capabilities"])


def registry() -> Registry:
    built = Registry()
    load_declarations(built, DECLARATIONS)
    return built



def compiled(source: str, allowed: frozenset[str] = ALL, budgets: Budgets | None = None) -> ir.Program:
    result = compile_source(source, registry(), allowed=allowed, budgets=budgets or Budgets())
    assert result.program is not None, [d.message for d in result.diagnostics]
    return result.program


def fixture_run(
    source: str, responses: list[dict[str, Any]], budgets: Budgets | None = None
) -> tuple[Completed | Failed, FixtureHost]:
    host = FixtureHost.from_responses(registry(), responses)
    outcome = run(compiled(source, budgets=budgets), host, budgets=budgets or Budgets()).outcome
    assert isinstance(outcome, Completed | Failed)
    return outcome, host


def fixture(name: str, payload: dict[str, Any], *results: dict[str, Any]) -> dict[str, Any]:
    return {"capability": name, "input": payload, "results": list(results)}


def ok(value: object) -> dict[str, Any]:
    return {"output": value}


def refused(kind: str, message: str) -> dict[str, Any]:
    return {"error": {"kind": kind, "message": message}}


def item(identifier: str, size: int) -> dict[str, Any]:
    return fixture("store.get", {"id": identifier}, ok({"id": identifier, "size": size}))


def test_a_rejected_program_has_no_program_to_run() -> None:
    result = compile_source('(call store.get {:id 1})', registry(), allowed=ALL)
    assert result.program is None
    assert [d.code for d in result.diagnostics] == ["type-mismatch"]


def test_an_unauthorized_capability_rejects_the_whole_program() -> None:
    source = '(let [a (call store.get {:id "a"})] (call mail.send {:to "x"}))'
    result = compile_source(source, registry(), allowed=frozenset({"store.get"}))
    assert result.program is None
    assert [d.code for d in result.diagnostics] == ["unauthorized"]


def test_par_results_follow_source_order_when_responses_arrive_reversed() -> None:
    machine = Machine(compiled('(par (call store.get {:id "a"}) (call store.get {:id "b"}))'))
    first = machine.start()
    assert isinstance(first, Suspended)
    assert [r.id for r in first.requests] == ["r.0:1", "r.1:1"]
    calls = [r for r in reversed(first.requests) if isinstance(r, CallRequest)]
    responses = [(r.id, Output({"id": "a" if r.id == "r.0:1" else "b", "size": 1})) for r in calls]
    done = machine.resume(responses)
    assert isinstance(done, Completed)
    assert done.json == [{"id": "a", "size": 1}, {"id": "b", "size": 1}]


@pytest.mark.parametrize(
    ("answer", "message"),
    [
        (lambda ids: [], "did not answer request r.0:1"),
        (lambda ids: [(ids[0], Output({"id": "a", "size": 1}))] * 2, "answered request r.0:1 twice"),
        (lambda ids: [*((i, Output({"id": "a", "size": 1})) for i in ids), ("r.9:1", Output(None))], "outstanding"),
    ],
)
def test_the_host_must_answer_each_request_exactly_once(answer: Any, message: str) -> None:
    machine = Machine(compiled('(par (call store.get {:id "a"}) (call store.get {:id "b"}))'))
    first = machine.start()
    assert isinstance(first, Suspended)
    outcome = machine.resume(answer([r.id for r in first.requests]))
    assert isinstance(outcome, Failed)
    assert outcome.failure.kind is FailureKind.INVALID_HOST_RESPONSE
    assert message in outcome.failure.message


def test_a_response_of_the_wrong_type_is_an_invalid_host_response() -> None:
    wrong = fixture("store.get", {"id": "a"}, ok({"id": "a", "size": "big"}))
    outcome, _ = fixture_run('(call store.get {:id "a"})', [wrong])
    assert isinstance(outcome, Failed)
    assert outcome.failure.kind is FailureKind.INVALID_HOST_RESPONSE
    assert "$.size: expected Int, found a string" in outcome.failure.message


def test_an_extra_field_in_a_response_is_rejected_not_dropped() -> None:
    extra = fixture("store.get", {"id": "a"}, ok({"id": "a", "size": 1, "owner": "x"}))
    outcome, _ = fixture_run('(call store.get {:id "a"})', [extra])
    assert isinstance(outcome, Failed)
    assert "$.owner" in outcome.failure.message


def test_an_unhandled_call_error_ends_the_program_and_attempt_captures_it() -> None:
    gone = [fixture("store.get", {"id": "a"}, refused("NotFound", "gone"))]
    outcome, _ = fixture_run('(call store.get {:id "a"})', gone)
    assert isinstance(outcome, Failed)
    assert outcome.failure.kind is FailureKind.CALL_FAILED
    handled = '(match (attempt (call store.get {:id "a"})) (ok i) "found" (err e) (get e :message))'
    captured, _ = fixture_run(handled, gone)
    assert isinstance(captured, Completed)
    assert captured.json == "gone"


def test_retry_waits_the_listed_delays_for_transient_failures() -> None:
    results = (refused("Unavailable", "502"), refused("Timeout", "slow"), ok({"id": "a", "size": 7}))
    flaky = [fixture("store.get", {"id": "a"}, *results)]
    outcome, host = fixture_run('(get (retry {:delays [1s 2s 4s]} (call store.get {:id "a"})) :size)', flaky)
    assert isinstance(outcome, Completed)
    assert outcome.json == 7
    assert host.waited_ms == 3000


def test_retry_does_not_repeat_a_failure_outside_its_kinds() -> None:
    gone = [fixture("store.get", {"id": "a"}, refused("NotFound", "gone"))]
    outcome, host = fixture_run('(attempt (retry {:delays [1s 2s]} (call store.get {:id "a"})))', gone)
    assert isinstance(outcome, Completed)
    assert host.waited_ms == 0
    assert host.entries[0].served == 1


def test_retry_rejects_a_write_that_is_not_idempotent() -> None:
    result = compile_source('(retry {:delays [1s]} (call mail.send {:to "x"}))', registry(), allowed=ALL)
    assert [d.code for d in result.diagnostics] == ["retry-write"]


def test_wait_until_returns_the_first_value_that_satisfies_it() -> None:
    states = [fixture("job.state", {"id": "j"}, ok("queued"), ok("running"), ok("done"))]
    source = '(wait-until {:within 1m :every 5s} (fn [s] (= s "done")) (call job.state {:id "j"}))'
    outcome, host = fixture_run(source, states)
    assert isinstance(outcome, Completed)
    assert outcome.json == "done"
    assert host.waited_ms == 10_000


def test_wait_until_times_out_with_a_catchable_timeout() -> None:
    states = [fixture("job.state", {"id": "j"}, ok("running"))]
    waited = '(wait-until {:within 10s :every 5s} (fn [s] (= s "done")) (call job.state {:id "j"}))'
    source = f'(match (attempt {waited}) (ok s) s (err e) (match (get e :kind) Timeout "gave up" _ "other"))'
    outcome, host = fixture_run(source, states)
    assert isinstance(outcome, Completed)
    assert outcome.json == "gave up"
    assert host.waited_ms == 10_000


def test_loop_follows_a_cursor_until_it_ends() -> None:
    source = '(loop {:max 5} [n 0 total 0] (if (= n 3) total (recur (+ n 1) (+ total n))))'
    outcome, _ = fixture_run(source, [])
    assert isinstance(outcome, Completed)
    assert outcome.json == 3


def test_a_loop_that_needs_more_than_its_max_fails_with_loop_limit() -> None:
    outcome, _ = fixture_run('(loop {:max 3} [n 0] (recur (+ n 1)))', [])
    assert isinstance(outcome, Failed)
    assert outcome.failure.kind is FailureKind.LOOP_LIMIT


def test_par_map_sends_at_most_its_limit_of_requests_at_once() -> None:
    ids = [f'"{c}"' for c in "abcdef"]
    machine = Machine(compiled(f"(par-map {{:limit 2}} (fn [i] (call store.get {{:id i}})) [{' '.join(ids)}])"))
    first = machine.start()
    assert isinstance(first, Suspended)
    assert len(first.requests) == 2


def test_the_concurrency_budget_queues_extra_requests_for_the_next_suspension() -> None:
    calls = " ".join(f'(call store.get {{:id "{i}"}})' for i in range(5))
    budgets = Budgets(concurrency=3)
    machine = Machine(compiled(f"(par {calls})", budgets=budgets), budgets=budgets)
    first = machine.start()
    assert isinstance(first, Suspended)
    assert [r.id for r in first.requests] == ["r.0:1", "r.1:1", "r.2:1"]
    second = machine.resume([(r.id, Output({"id": "x", "size": 0})) for r in first.requests])
    assert isinstance(second, Suspended)
    assert [r.id for r in second.requests] == ["r.3:1", "r.4:1"]


def test_a_failed_par_branch_fails_the_par_with_the_lowest_failed_branch() -> None:
    responses = [
        item("a", 1),
        fixture("store.get", {"id": "b"}, refused("NotFound", "b gone")),
        fixture("store.get", {"id": "c"}, refused("Failed", "c broke")),
    ]
    branches = " ".join(f'(call store.get {{:id "{i}"}})' for i in "abc")
    source = f'(match (attempt (par {branches})) (ok t) "ok" (err e) (get e :message))'
    outcome, _ = fixture_run(source, responses)
    assert isinstance(outcome, Completed)
    assert outcome.json == "b gone"


def test_the_calls_budget_ends_a_program_that_calls_too_often() -> None:
    budgets = Budgets(calls=2)
    source = '(map (fn [i] (call store.get {:id i})) ["a" "b" "c"])'
    outcome, _ = fixture_run(source, [item("a", 1), item("b", 2), item("c", 3)], budgets=budgets)
    assert isinstance(outcome, Failed)
    assert outcome.failure.kind is FailureKind.BUDGET_EXHAUSTED


@pytest.mark.parametrize(
    ("source", "kind"),
    [
        ("(+ 9223372036854775807 1)", FailureKind.ARITHMETIC),
        ("(quot 1 0)", FailureKind.ARITHMETIC),
        ("(first [])", FailureKind.OUT_OF_RANGE),
        ("(nth [1 2] 2)", FailureKind.OUT_OF_RANGE),
    ],
)
def test_runtime_errors_end_the_program_with_their_class(source: str, kind: FailureKind) -> None:
    outcome, _ = fixture_run(source, [])
    assert isinstance(outcome, Failed)
    assert outcome.failure.kind is kind


def test_quot_and_rem_truncate_toward_zero() -> None:
    outcome, _ = fixture_run("[(quot -7 2) (rem -7 2) (quot 7 -2) (rem 7 -2)]", [])
    assert isinstance(outcome, Completed)
    assert outcome.json == [-3, -1, -3, 1]


def test_option_promotion_wraps_values_at_runtime() -> None:
    outcome, _ = fixture_run('(map (fn [n] (if (> n 1) n none)) [1 2 3])', [])
    assert isinstance(outcome, Completed)
    assert outcome.json == [None, 2, 3]


def test_sort_by_tuple_keys_sorts_descending_then_alphabetical() -> None:
    rows = '[{:t "b" :n 1} {:t "a" :n 3} {:t "c" :n 3}]'
    source = f'(map (fn [r] (get r :t)) (sort-by (fn [r] [(- (get r :n)) (get r :t)]) {rows}))'
    outcome, _ = fixture_run(source, [])
    assert isinstance(outcome, Completed)
    assert outcome.json == ["a", "c", "b"]


def test_local_functions_see_their_definition_scope() -> None:
    source = "(let [x 1 add (fn [y] (+ x y)) x 100] [(add 1) (add x)])"
    outcome, _ = fixture_run(source, [])
    assert isinstance(outcome, Completed)
    assert outcome.json == [2, 101]


def test_writes_inside_map_run_once_each_in_order() -> None:
    sends = [fixture("mail.send", {"to": t}, ok(None)) for t in ("a", "b")]
    outcome, host = fixture_run('(let [_ (map (fn [t] (call mail.send {:to t})) ["a" "b"])] "sent")', sends)
    assert isinstance(outcome, Completed)
    assert host.unperformed_writes() == []


def test_delays_are_requests_the_host_answers_with_null() -> None:
    machine = Machine(compiled('(retry {:delays [250ms]} (call store.get {:id "a"}))'))
    first = machine.start()
    assert isinstance(first, Suspended)
    second = machine.resume([(first.requests[0].id, Refusal("Unavailable", "x"))])
    assert isinstance(second, Suspended)
    assert second.requests == (DelayRequest("r:2", 250),)
    third = machine.resume([("r:2", Output(None))])
    assert isinstance(third, Suspended)
    assert isinstance(third.requests[0], CallRequest)
    assert third.requests[0].id == "r:3"
