import json
import stat
from dataclasses import asdict
from pathlib import Path

import definition
import gate
import oneshot
import pilot
import pytest
import trial
from arms import Arm, run_arm
from probe import Attempt, Trial

EXAMPLE = "example-summary"
RULES = Path(__file__).parent.parent / "rust" / "sx-cli" / "guide" / "typescript-rules.md"
BODY = """const dir = "{{root}}/data";
const entries = await tools.fs.list({ path: dir });
let count = 0;
let bytes = 0;
for (const entry of entries) {
  if (entry.kind === "File") {
    const info = await tools.fs.stat({ path: dir + "/" + entry.name });
    count += 1;
    bytes += info == null ? 0 : info.size;
  }
}
console.log("counted", count);
await tools.fs.writeText({ path: "{{root}}/summary.txt", text: "files: " + count + "\\nbytes: " + bytes + "\\n" });
return { count, bytes };
"""


def run_body(arm: Arm, body: str):
    workflow = definition.load(EXAMPLE)
    with workflow.sandbox() as sandbox:
        run = run_arm(arm, workflow, sandbox, workflow.render(body, sandbox))
        return run, trial.verdict_of(workflow, sandbox, run, body)


def test_the_two_body_prompts_differ_only_by_the_rules_section() -> None:
    workflow = definition.load(EXAMPLE)
    with workflow.sandbox() as sandbox:
        ts = trial.initial_prompt(workflow, sandbox, "ts", "standard", ts_notes=False)
        jsbody = trial.initial_prompt(workflow, sandbox, "jsbody", "standard", ts_notes=False)
    rules = RULES.read_text().strip()
    assert ts.count(rules) == 1
    assert jsbody.count("## Writing the program") == 0
    assert ts.replace(rules + "\n\n", "") == jsbody
    assert "## Your tools" in jsbody
    assert "## How the tools behave" in jsbody


@pytest.mark.parametrize("arm", ["ts", "jsbody"])
def test_a_body_runs_in_both_executors_and_is_judged_alike(arm: Arm) -> None:
    run, verdict = run_body(arm, BODY)
    assert run.status == "completed", run.detail
    assert verdict == ("correct", "")
    assert [c.capability for c in run.calls].count("fs.write-text") == 1
    if arm == "jsbody":
        assert json.loads(run.transcript)["logs"] == ["counted 3"]


def test_a_type_error_in_a_jsbody_names_the_line_of_the_body() -> None:
    body = 'const entries = await tools.fs.list({ path: "{{root}}/data" });\n\nconst n: number = "x";\nreturn entries;'
    run, _ = run_body("jsbody", body)
    assert run.status == "rejected"
    assert run.labels == ["TS2322"]
    assert "solution.mts(3," in run.detail


def test_a_thrown_error_in_a_jsbody_is_reported_as_the_existing_js_arm_does() -> None:
    run, _ = run_body("jsbody", 'return await tools.fs.list({ path: "{{root}}/absent" });')
    assert run.status == "failed"
    assert run.feedback is not None
    assert run.feedback.startswith("The module threw: CallError")


def test_a_ts_body_the_subset_rejects_is_fed_back_with_its_diagnostic_code() -> None:
    run, _ = run_body("ts", 'while (true) { await tools.fs.list({ path: "{{root}}/data" }); }')
    assert run.status == "rejected"
    assert run.calls == []
    assert run.labels
    assert run.feedback is not None


def test_the_cost_of_every_reply_is_summed(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    def message(cost: float) -> str:
        usage = {"input": 10, "output": 5, "cacheRead": 0, "cost": {"total": cost}}
        reply = {"role": "assistant", "content": [{"type": "text", "text": "ok"}], "usage": usage, "stopReason": "stop"}
        return json.dumps({"type": "message_end", "message": reply})

    fake = tmp_path / "pi"
    fake.write_text(f"#!/bin/sh\ncat <<'EOF'\n{message(0.25)}\n{message(0.5)}\nEOF\n")
    fake.chmod(fake.stat().st_mode | stat.S_IXUSR)
    prompt = tmp_path / "prompt.md"
    prompt.write_text("hi")
    monkeypatch.setattr(pilot, "PI", fake)
    reply, tokens, error, cost = pilot.ask(prompt, "m", "medium", tmp_path)
    assert (reply, error, cost) == ("okok", None, 0.75)
    assert tokens["output"] == 10


def attempt(status: str, calls: int, cost: float, diagnostics: list[str] | None = None) -> Attempt:
    return Attempt("", "", {}, None, status, "", None, diagnostics or [], calls, cost=cost)


def record(out: Path, name: str, arm: str, model: str, workflow_set: str, attempts: list[Attempt]) -> None:
    folder = out / name / f"{arm}-0"
    folder.mkdir(parents=True)
    saved = Trial(name, arm, model, "medium", "none", 0, attempts, workflow_set)
    (folder / "trial.json").write_text(json.dumps(asdict(saved)))


def synthetic(tmp_path: Path) -> list[Path]:
    blind, blind2 = tmp_path / "blind", tmp_path / "blind2"
    first = [attempt("rejected", 0, 0.1, ["unsupported:while"]), attempt("correct", 3, 0.1)]
    record(blind, "a", "ts", "m1", "blind", first)
    record(blind, "b", "ts", "m1", "blind", [attempt("failed", 2, 0.1), attempt("wrong", 4, 0.1)])
    record(blind, "a", "jsbody", "m1", "blind", [attempt("correct", 3, 0.1)])
    record(blind, "b", "jsbody", "m1", "blind", [attempt("wrong", 1, 0.1)])
    record(blind2, "c", "ts", "m1", "blind2", [attempt("correct", 2, 0.2)])
    record(blind2, "c", "jsbody", "m1", "blind2", [attempt("correct", 2, 0.2)])
    return [blind, blind2]


def test_the_report_counts_first_attempts_final_answers_cost_and_wasted_calls(tmp_path: Path) -> None:
    trials = oneshot.load(synthetic(tmp_path))
    rows = oneshot.summary_rows(trials)
    ts_blind = next(r for r in rows if r.startswith("| blind | m1 | ts "))
    assert ts_blind == "| blind | m1 | ts | 2 | 1/2 (50.0%) | 0/2 (0.0%) | $0.2000 | 3.00 |"
    pooled = next(r for r in rows if r.startswith("| all sets | all models | jsbody "))
    assert pooled == "| all sets | all models | jsbody | 3 | 2/3 (66.7%) | 2/3 (66.7%) | $0.1333 | 0.33 |"


def test_the_gate_compares_ts_to_jsbody_pooled_over_trials(tmp_path: Path) -> None:
    runs = synthetic(tmp_path)
    found = gate.cells(oneshot.scored(oneshot.load(runs)))
    gap, ratio, passed = gate.verdict(found[gate.POOLED, "ts"], found[gate.POOLED, "jsbody"])
    assert gap == pytest.approx(0.0)
    assert ratio == pytest.approx(0.6 / 0.4)
    assert not passed
    assert "| all models | +0.0 | 1.50x | fail |" in oneshot.render(runs)


def test_the_stop_table_credits_each_code_with_the_repairs_it_caused(tmp_path: Path) -> None:
    table = oneshot.render(synthetic(tmp_path))
    assert "| unsupported:while | 1 | 1 |" in table
    assert "| (no code) | 1 | 1 |" in table
