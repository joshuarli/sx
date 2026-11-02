from __future__ import annotations

import json
import stat
import sys
from pathlib import Path
from typing import Literal

import pytest

from tsdiff.case import CaseFiles, Json
from tsdiff.compare import (
    Failure,
    Outcome,
    Status,
    SxUnsupportedLanguageError,
    compare,
    normalize_js,
    normalize_sx,
    run_sx,
    same_json,
)


def outcome(
    side: Literal["js", "sx"] = "js",
    status: Status = "completed",
    result: Json = 1,
    logs: tuple[str, ...] = (),
    calls: tuple[tuple[str, str], ...] = (),
    delays: tuple[int, ...] = (),
    failure: Failure | None = None,
    codes: tuple[str, ...] = (),
) -> Outcome:
    return Outcome(side, status, result, logs, calls, delays, failure, codes)


def test_sx_report_is_normalized_to_calls_delays_logs_and_result() -> None:
    raw = {
        "outcome": "completed",
        "result": {"id": "a"},
        "logs": ["hi"],
        "trace": [
            {"request": "r:1", "capability": "store.get", "input": {"id": "a", "extra": None}},
            {"response": "r:1", "output": 1},
            {"delay": "r:2", "milliseconds": 1000},
            {"response": "r:2", "output": None},
        ],
    }
    normalized = normalize_sx(raw)
    assert normalized.calls == (("store.get", '{"id":"a"}'),)
    assert normalized.delays == (1000,)
    assert normalized.logs == ("hi",)
    assert normalized.result == {"id": "a"}


def test_sx_call_failure_carries_the_call_error_kind() -> None:
    message = "A tool call failed with NotFound: gone"
    raw = {"outcome": "failed", "failure": {"kind": "call-failed", "message": message}, "trace": []}
    assert normalize_sx(raw).failure == Failure("call-failed", "NotFound", message)


def test_sx_rejection_and_unexpected_call() -> None:
    rejected = normalize_sx({"ok": False, "diagnostics": [{"code": "ts-while"}, {"code": "ts-date"}]})
    assert (rejected.status, rejected.codes) == ("rejected", ("ts-while", "ts-date"))
    listed = [{"capability": "a.b", "input": {}}]
    unexpected = normalize_sx({"outcome": "unexpected-call", "message": "m", "calls": listed})
    assert unexpected.failure is not None
    assert (unexpected.failure.cls, unexpected.calls) == ("unexpected-call", (("a.b", "{}"),))


def test_js_report_maps_traps_and_call_errors_to_failure_classes() -> None:
    base = {"result": None, "logs": [], "calls": [], "delays": []}
    trap = normalize_js({**base, "error": {"class": "trap", "trap": "out-of-range", "message": "m"}})
    assert trap.trap == "out-of-range"
    failed = normalize_js({**base, "error": {"class": "call-failed", "kind": "Timeout", "message": "m"}})
    assert failed.failure == Failure("call-failed", "Timeout", "m")


def test_json_equality_ignores_key_order_and_int_float_but_not_bool() -> None:
    assert same_json({"a": 1, "b": [2.0]}, {"b": [2], "a": 1.0})
    assert not same_json(1, True)
    assert not same_json({"a": 1}, {"a": 1, "b": 1})
    assert not same_json("1", 1)


def test_agreement_requires_result_logs_calls_and_delays() -> None:
    call = (("a.b", "{}"),)
    js = outcome(calls=call, logs=("x",), delays=(5,))
    assert compare(js, outcome("sx", calls=call, logs=("x",), delays=(5,)), parallel=False).kind == "agree"
    assert compare(js, outcome("sx", calls=call, logs=("y",), delays=(5,)), parallel=False).kind == "disagree"
    assert compare(js, outcome("sx", calls=(), logs=("x",), delays=(5,)), parallel=False).kind == "disagree"
    assert compare(js, outcome("sx", calls=call, logs=("x",), delays=()), parallel=False).kind == "disagree"
    assert compare(outcome(result=2), outcome("sx", result=3), parallel=False).kind == "disagree"


def test_a_trap_agrees_only_when_the_other_side_traps_the_same_way_or_the_case_expects_it() -> None:
    trapped = outcome(status="failed", result=None, failure=Failure("arithmetic"))
    same = outcome("sx", status="failed", result=None, failure=Failure("arithmetic"))
    other = outcome("sx", status="failed", result=None, failure=Failure("out-of-range"))
    assert compare(trapped, same, parallel=False).kind == "agree"
    assert compare(trapped, other, parallel=False).kind == "disagree"
    assert compare(trapped, outcome("sx"), parallel=False).kind == "disagree"
    assert compare(outcome(), same, parallel=False).kind == "disagree"
    assert compare(outcome(), same, parallel=False, expect_trap="arithmetic").kind == "agree"
    assert compare(outcome(), other, parallel=False, expect_trap="arithmetic").kind == "disagree"


def test_call_error_kinds_must_match_but_an_unknown_sx_kind_is_not_held_against_it() -> None:
    js = outcome(status="failed", result=None, failure=Failure("call-failed", "NotFound"))
    sx = outcome("sx", status="failed", result=None, failure=Failure("call-failed", "Timeout"))
    assert compare(js, sx, parallel=False).kind == "disagree"
    unknown = outcome("sx", status="failed", result=None, failure=Failure("call-failed", None))
    assert compare(js, unknown, parallel=False).kind == "agree"


def test_parallel_programs_may_start_reads_in_another_order_and_that_is_a_refinement() -> None:
    js = outcome(calls=(("a.b", "1"), ("a.b", "2")))
    sx = outcome("sx", calls=(("a.b", "2"), ("a.b", "1")))
    assert compare(js, sx, parallel=False).kind == "disagree"
    assert compare(js, sx, parallel=True).kind == "agree-refined"
    timeout = Failure("call-failed", "Timeout")
    fewer = outcome("sx", status="failed", result=None, calls=(("a.b", "1"),), failure=timeout)
    both = (("a.b", "1"), ("a.b", "2"))
    js_failed = outcome(status="failed", result=None, calls=both, failure=timeout)
    assert compare(js_failed, fewer, parallel=True).kind == "agree-refined"
    assert compare(js_failed, fewer, parallel=False).kind == "disagree"


def test_a_rejection_by_sx_is_its_own_verdict_with_the_codes() -> None:
    verdict = compare(outcome(), outcome("sx", status="rejected", codes=("ts-while",)), parallel=False)
    assert (verdict.kind, verdict.reasons) == ("rejected", ("ts-while",))


def test_sx_accepting_what_the_js_side_cannot_run_is_a_disagreement() -> None:
    broken = outcome(status="failed", result=None, failure=Failure("js-error", None, "x is not a function"))
    assert compare(broken, outcome("sx"), parallel=False).kind == "disagree"


def case_files(tmp_path: Path) -> CaseFiles:
    files = {"program.ts": "return 1;", "declarations.json": "{}", "responses.json": "[]", "tools.json": "[]"}
    for name, text in files.items():
        (tmp_path / name).write_text(text)
    (tmp_path / "case.json").write_text(json.dumps({"name": "c", "allow": ["a.b"]}))
    return CaseFiles.read(tmp_path)


def stub(tmp_path: Path, body: str) -> Path:
    path = tmp_path / "sx"
    path.write_text(f"#!{sys.executable}\nimport json, sys\n{body}\n")
    path.chmod(path.stat().st_mode | stat.S_IEXEC)
    return path


def test_run_sx_passes_the_language_flag_and_reads_the_json_report(tmp_path: Path) -> None:
    binary = stub(tmp_path, "print(json.dumps({'outcome': 'completed', 'result': sys.argv[1:], 'trace': []}))")
    ran = run_sx(case_files(tmp_path), binary)
    argv = ran.result
    assert argv[:2] == ["run", str(tmp_path / "program.ts")]
    assert argv[argv.index("--language") + 1] == "ts"
    assert argv[argv.index("--allow") + 1] == "a.b"
    assert argv[argv.index("--format") + 1] == "json"


def test_run_sx_fails_loudly_when_sx_rejects_the_flag(tmp_path: Path) -> None:
    refusal = "print('sx: error: unrecognized arguments: --language ts', file=sys.stderr); sys.exit(2)"
    binary = stub(tmp_path, refusal)
    with pytest.raises(SxUnsupportedLanguageError, match="--language ts"):
        run_sx(case_files(tmp_path), binary)
