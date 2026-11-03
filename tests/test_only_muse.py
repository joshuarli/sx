import subprocess
import sys
from pathlib import Path

import pilot
import pytest
import real
import session

OTHER = "opencode-go/kimi-k3"
ROOT = Path(__file__).parent.parent


def test_the_one_shot_harness_refuses_another_model_before_starting_pi(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    started: list[object] = []
    monkeypatch.setattr(pilot.subprocess, "run", lambda *a, **k: started.append(a))
    prompt = tmp_path / "prompt.md"
    prompt.write_text("hi")
    with pytest.raises(ValueError, match=OTHER):
        pilot.ask(prompt, OTHER, "medium", tmp_path)
    assert started == []


def test_a_session_refuses_another_model() -> None:
    wired = session.Wiring("sx", ["sx"], "sx_*", (), {}, None)
    with pytest.raises(ValueError, match=OTHER):
        session.pi_command(wired, OTHER, "medium", "task")
    assert pilot.MODEL in session.pi_command(wired, pilot.MODEL, "medium", "task")


def test_a_real_request_refuses_another_model_before_creating_its_run(tmp_path: Path) -> None:
    request = real.Request("r", "task")
    with pytest.raises(ValueError, match=OTHER):
        real.run_one(request, "sx", OTHER, "medium", 0, tmp_path, tmp_path, "direct")
    assert list(tmp_path.iterdir()) == []


@pytest.mark.parametrize("script", [
    "bench/workflows/runner.py", "bench/workflows/real.py", "bench/codemode/probe.py", "bench/codemode/pilot.py",
])
def test_every_runner_rejects_another_model_on_its_command_line(script: str) -> None:
    completed = subprocess.run(
        [sys.executable, script, "--model", OTHER, "--help"], cwd=ROOT, capture_output=True, text=True,
    )
    assert completed.returncode == 2
    assert "--model" in completed.stderr
