from pathlib import Path

import pytest
from probe import judge, sx_binary, unperformed_write

RETRY_TASK = {"expected": "done", "kinds": ["retry"]}


def test_an_immediate_retry_scores_the_same_with_or_without_a_zero_wait() -> None:
    assert judge(RETRY_TASK, "done", [0, 0], [], None) == ("correct", "")
    assert judge(RETRY_TASK, "done", [], [0], None) == ("correct", "")


def test_a_real_wait_still_has_to_match() -> None:
    assert judge(RETRY_TASK, "done", [0, 100], [100, 100], None)[0] == "wrong"


def test_a_missing_binary_stops_the_probe(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    monkeypatch.setenv("SX_BIN", str(tmp_path / "absent"))
    with pytest.raises(FileNotFoundError, match="SX_BIN"):
        sx_binary()


def test_a_write_with_unserved_results_is_reported_as_not_performed() -> None:
    task = {
        "capabilities": [{"id": "chat.post", "effect": "write"}, {"id": "chat.read", "effect": "read"}],
        "responses": [
            {"capability": "chat.read", "input": {}, "results": [{"output": 1}]},
            {"capability": "chat.post", "input": {"text": "hi"}, "results": [{"output": None}]},
        ],
    }
    assert unperformed_write(task, [{"capability": "chat.read", "input": {}}]) == "chat.post"
    assert unperformed_write(task, [{"capability": "chat.post", "input": {"text": "hi"}}]) is None
