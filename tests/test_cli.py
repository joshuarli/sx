import json
from pathlib import Path

import pytest

from sx.cli import EXIT_FAILED, EXIT_OK, EXIT_REJECTED, EXIT_USAGE, main

CLI = Path(__file__).parent.parent / "fixtures" / "cli"
DECLARATIONS = ["--declarations", str(CLI / "declarations.json")]


def test_run_prints_the_result_and_reports_a_handled_failure(capsys: pytest.CaptureFixture[str]) -> None:
    code = main([
        "run", str(CLI / "uptimes.sx"), *DECLARATIONS, "--responses", str(CLI / "responses.json"),
        "--allow", "metrics.uptime", "--input", 'hosts=["db1","db2"]', "--format", "json",
    ])
    out = json.loads(capsys.readouterr().out)
    assert code == EXIT_OK
    assert out["result"] == [40, -1]
    assert [event.get("request") for event in out["trace"] if "request" in event] == ["r.0:1", "r.1:1"]


def test_check_rejects_an_unauthorized_tool(capsys: pytest.CaptureFixture[str]) -> None:
    code = main(["check", str(CLI / "uptimes.sx"), *DECLARATIONS, "--format", "json"])
    out = json.loads(capsys.readouterr().out)
    assert code == EXIT_REJECTED
    assert [d["code"] for d in out["diagnostics"]] == ["unauthorized"]
    assert out["diagnostics"][0]["span"] == {"start": {"line": 3, "column": 27}, "end": {"line": 3, "column": 41}}


def test_explain_lists_tools_and_literal_inputs_before_authorization(capsys: pytest.CaptureFixture[str]) -> None:
    code = main(["explain", str(CLI / "explain.sx"), *DECLARATIONS, "--format", "json"])
    out = json.loads(capsys.readouterr().out)
    assert code == EXIT_OK
    assert out["tools"] == [{
        "id": "metrics.uptime", "effect": "read", "allowed": False,
        "inputs": [{"host": "db1"}, "computed at run time"],
    }]
    assert out["inputs"] == ["hosts"]


def test_run_fails_loudly_when_a_tool_call_has_no_recorded_response(capsys: pytest.CaptureFixture[str]) -> None:
    code = main([
        "run", str(CLI / "uptimes.sx"), *DECLARATIONS, "--responses", str(CLI / "responses.json"),
        "--allow", "metrics.uptime", "--input", 'hosts=["db9"]',
    ])
    assert code == EXIT_FAILED
    assert "metrics.uptime has no fixture for this input" in capsys.readouterr().out


def test_a_missing_input_is_a_usage_error(capsys: pytest.CaptureFixture[str]) -> None:
    code = main(["run", str(CLI / "uptimes.sx"), *DECLARATIONS, "--allow", "metrics.uptime"])
    assert code == EXIT_USAGE
    assert "needs a value for input hosts" in capsys.readouterr().err


def test_help_suggests_a_close_name(capsys: pytest.CaptureFixture[str]) -> None:
    assert main(["help", "filtr"]) == EXIT_REJECTED
    assert "Did you mean filter?" in capsys.readouterr().err


def test_help_describes_a_declared_tool_with_its_types(capsys: pytest.CaptureFixture[str]) -> None:
    assert main(["help", "pager.alert", *DECLARATIONS]) == EXIT_OK
    first = capsys.readouterr().out.splitlines()[0]
    assert first == "(call pager.alert Record {host: String}) -> Unit   [write, not idempotent]"
