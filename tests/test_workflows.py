import json
import re
import subprocess
from dataclasses import replace
from pathlib import Path
from typing import Any

import arms
import definition
import pytest
import trial
from arms import Arm, ToolchainMissingError, run_arm
from definition import Evidence
from probe import TSC, tokens
from sandbox import Response, Sandbox

BOTH: list[Arm] = ["sx", "js"]
HARNESS = Path(__file__).parent.parent / "bench" / "workflows" / "harness"
EXAMPLE = "example-summary"
OPEN_STORE_GRANTS = json.dumps({
    "fs.list": {"roots": ["{{root}}/docs"]},
    "fs.read-text": {"roots": ["{{root}}/docs"]},
    "fs.stat": {"roots": ["{{root}}/docs"]},
    "store.put": {"origins": ["{{store_origin}}"], "methods": ["PUT"]},
})
BROKEN = {
    "sx": '(call fs.read-text {:path "/x"})',
    "js": "export default async function main() { return nope; }",
}


@pytest.mark.parametrize("workflow_id", definition.available())
@pytest.mark.parametrize("arm", BOTH)
def test_every_workflow_reference_passes_in_both_arms(workflow_id: str, arm: Arm) -> None:
    _, status, detail = trial.reference(definition.load(workflow_id), arm)
    assert (status, detail) == ("correct", "")


@pytest.mark.parametrize("arm", BOTH)
def test_without_the_granted_credential_the_object_store_refuses_the_publish(arm: Arm) -> None:
    workflow = replace(definition.load("object-store-index"), grants=OPEN_STORE_GRANTS)
    with workflow.sandbox() as sandbox:
        run = run_arm(arm, workflow, sandbox, workflow.render(workflow.reference(arm), sandbox))
    assert run.status == "failed"
    assert "PermissionDenied" in run.detail or "credential required" in run.detail


@pytest.mark.parametrize("arm", BOTH)
def test_the_example_workflow_passes_in_both_arms(arm: Arm) -> None:
    run, status, detail = trial.reference(definition.load(EXAMPLE), arm)
    assert (status, detail) == ("correct", "")
    assert [call.capability for call in run.calls].count("fs.write-text") == 1


@pytest.mark.parametrize(
    ("arm", "program"),
    [
        ("sx", '(let [x (call fs.list {:path "{{root}}/data"})] {:count (count x) :bytes 0})'),
        ("js", 'export default async function main(api: Api) { return { count: 4, bytes: 0 }; }'),
    ],
)
def test_the_check_inspects_the_sandbox_not_only_the_result(arm: Arm, program: str) -> None:
    workflow = definition.load(EXAMPLE)
    with workflow.sandbox() as sandbox:
        run = run_arm(arm, workflow, sandbox, workflow.render(program, sandbox))
        assert run.status == "completed"
        verdict = workflow.check(sandbox, run.result, Evidence("", run.transcript))
    assert verdict.status == "wrong"


def test_a_denied_call_has_the_same_kind_in_both_arms() -> None:
    workflow = definition.load(EXAMPLE)
    programs = {
        "sx": '(attempt (call fs.stat {:path (str "/" "etc")}))',
        "js": (
            "export default async function main(api: Api) {"
            ' try { return await api.fs.stat({ path: "/etc" }); } catch (error) { return (error as CallError).kind; } }'
        ),
    }
    kinds: dict[str, Any] = {}
    for arm in BOTH:
        with workflow.sandbox() as sandbox:
            run = run_arm(arm, workflow, sandbox, programs[arm])
            assert run.status == "completed", run.detail
            kinds[arm] = run.result
    assert kinds["js"] == "PermissionDenied"
    assert kinds["sx"]["err"]["kind"] == "PermissionDenied"


def test_a_tool_that_is_not_granted_is_absent_from_the_typescript_api() -> None:
    workflow = definition.load(EXAMPLE)
    program = 'export default async function main(api: Api) { return await api.fs.readText({ path: "{{root}}/x" }); }'
    with workflow.sandbox() as sandbox:
        run = run_arm("js", workflow, sandbox, workflow.render(program, sandbox))
    assert (run.status, run.calls) == ("rejected", [])
    assert "readText" in run.detail


def test_a_tool_that_is_not_granted_is_rejected_by_the_sx_checker() -> None:
    workflow = definition.load(EXAMPLE)
    with workflow.sandbox() as sandbox:
        run = run_arm("sx", workflow, sandbox, '(call fs.read-text {:path "/x"})')
    assert (run.status, run.calls) == ("rejected", [])
    assert run.labels == ["unauthorized"]


def test_a_runtime_failure_in_each_arm_carries_what_a_model_would_be_shown() -> None:
    workflow = definition.load(EXAMPLE)
    programs = {
        "sx": '(call fs.list {:path "{{root}}/absent"})',
        "js": 'export default async function main(api: Api) { return await api.fs.list({ path: "{{root}}/absent" }); }',
    }
    for arm in BOTH:
        with workflow.sandbox() as sandbox:
            run = run_arm(arm, workflow, sandbox, workflow.render(programs[arm], sandbox))
        assert run.status == "failed", arm
        assert run.feedback is not None
        assert "No such file" in run.detail, run.detail


def test_resetting_a_sandbox_rebuilds_the_same_state_and_keeps_its_services() -> None:
    workflow = definition.load(EXAMPLE)
    with Sandbox() as sandbox:
        workflow.setup(sandbox)
        service = sandbox.serve("api", lambda request: Response.json(json.dumps({"path": request.path})))
        origin = sandbox.variables["api_origin"]

        def tree() -> dict[str, str]:
            return {str(p.relative_to(sandbox.root)): p.read_text() for p in sandbox.root.rglob("*") if p.is_file()}

        before = tree()
        (sandbox.root / "stray.txt").write_text("left by a program")
        sandbox.reset()
        assert tree() == {}
        workflow.setup(sandbox)
        sandbox.serve("api", lambda request: Response.json("{}"))
        assert (tree(), sandbox.variables["api_origin"], len(sandbox.services)) == (before, origin, 1)
        assert service.requests == []


def test_a_local_git_repository_is_deterministic_and_a_service_records_its_requests() -> None:
    with Sandbox() as sandbox:
        repository = sandbox.git_repository("repo", [{"a.txt": "one"}, {"a.txt": "two", "b.txt": "x"}])
        assert sandbox.git(repository, "log", "--format=%s").split() == ["commit", "2", "commit", "1"]
        first = sandbox.git(repository, "rev-parse", "HEAD")
    with Sandbox() as other:
        again = other.git_repository("repo", [{"a.txt": "one"}, {"a.txt": "two", "b.txt": "x"}])
        assert other.git(again, "rev-parse", "HEAD") == first


def test_a_placeholder_without_a_sandbox_variable_is_an_error() -> None:
    workflow = definition.load(EXAMPLE)
    with Sandbox() as sandbox, pytest.raises(KeyError, match="nowhere"):
        workflow.render("{{nowhere}}", sandbox)


def test_a_missing_bun_stops_the_typescript_arm(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("PATH", "")
    with pytest.raises(ToolchainMissingError, match="bun"):
        arms.require_toolchain()


def test_a_missing_tsc_stops_the_typescript_arm(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    monkeypatch.setattr(arms, "TSC", tmp_path / "absent")
    with pytest.raises(ToolchainMissingError, match="npm ci"):
        arms.require_toolchain()


def test_the_harness_type_checks_with_the_pinned_tsc() -> None:
    checked = subprocess.run([str(TSC), "-p", str(HARNESS)], capture_output=True, text=True, check=False)
    assert checked.returncode == 0, checked.stdout + checked.stderr


def reply(program: str) -> tuple[str, dict[str, int], str | None]:
    language = "sx" if program.lstrip().startswith("(") else "ts"
    return f"```{language}\n{program}\n```", {"input": 100, "output": 10, "reasoning": 0}, None


@pytest.mark.parametrize("arm", BOTH)
def test_a_model_trial_repairs_a_rejected_answer_and_counts_tokens(arm: Arm, tmp_path: Path) -> None:
    workflow = definition.load(EXAMPLE)
    prompts: list[str] = []
    answers: list[str] = []

    def fake_model(prompt: Path, model: str, thinking: str, workdir: Path) -> tuple[str, dict[str, int], str | None]:
        text = prompt.read_text()
        prompts.append(text)
        root = re.search(r"`(/[^`]+)/data`", text)
        assert root is not None
        root_path = root.group(1)
        if not answers:
            answers.append("rejected")
            return reply(BROKEN[arm])
        answers.append("repaired")
        return reply(workflow.reference(arm).replace("{{root}}", root_path))

    finished = trial.run_trial(workflow, arm, "fake", "none", "standard", 0, tmp_path, fake_model)
    assert [a.status for a in finished.attempts] == ["rejected", "correct"]
    assert tokens(finished) == 220
    assert "It failed:" in prompts[1]
    folder = "js-0" if arm == "js" else "sx-standard-0"
    saved: dict[str, Any] = json.loads((tmp_path / EXAMPLE / folder / "trial.json").read_text())
    assert saved["attempts"][1]["status"] == "correct"
