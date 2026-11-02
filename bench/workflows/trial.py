import json
import time
from collections.abc import Callable
from dataclasses import asdict
from pathlib import Path
from typing import Any

import arms
from arms import NO_CORES, Arm, ArmRun
from definition import Evidence, Workflow
from pilot import ask as ask_model
from pilot import extract
from probe import REPAIRS, Attempt, Trial, js_prompt, repair_prompt
from sandbox import Sandbox

from sx.help import guide

type Json = dict[str, Any]
type Ask = Callable[[Path, str, str, Path], tuple[str, dict[str, int], str | None]]

LANGUAGE: dict[str, str] = {"sx": "sx", "js": "ts"}


TAILORED = "tailored"
VARIANTS = ("slim",)
REPLY_SX = "Reply with one Sx program in a ```sx code block, and nothing else."


def sections(guide: str) -> list[str]:
    return ["## " + part if index else part for index, part in enumerate(guide.split("\n## "))]


def slimmed(guide: str) -> str:
    kept = []
    for section in sections(guide):
        if section.startswith("## Coming from TypeScript"):
            intro, _, rest = section.partition("\n| TypeScript | Sx |")
            differs = rest[rest.index("### Where Sx differs") :] if "### Where Sx differs" in rest else ""
            section = f"{intro.rstrip()}\n\n{differs}"
        elif section.startswith("## The ") and " core" in section.split("\n", 1)[0]:
            heading = section.split("\n", 1)[0]
            section = f"{heading}\n\n{section[section.index('Functions:') :]}"
        kept.append(section.rstrip())
    return "\n\n".join(kept)


def tailored_variants(level: str) -> set[str] | None:
    head, *parts = level.split("+")
    if head not in (TAILORED, *VARIANTS) or any(part not in VARIANTS for part in parts):
        return None
    chosen: set[str] = {*parts}
    if head != TAILORED:
        chosen.add(head)
    return chosen


def sx_prompt(workflow: Workflow, sandbox: Sandbox, level: str) -> str:
    task = workflow.render(workflow.task, sandbox).strip()
    variants = tailored_variants(level)
    if variants is not None:
        shown = arms.sx_guide(workflow, sandbox, notes=False).rstrip()
        if "slim" in variants:
            shown = slimmed(shown)
        return "\n".join([shown, "", "## Your task", "", task, "", REPLY_SX]) + "\n"
    return "\n".join([
        guide(level), "", "## Your task", "", task, "", arms.sx_listing(workflow, sandbox),
        REPLY_SX,
    ]) + "\n"


def js_arm_prompt(workflow: Workflow, sandbox: Sandbox, *, notes: bool) -> str:
    task = {"description": workflow.render(workflow.task, sandbox).strip()}
    prompt = js_prompt({**arms.described_for(workflow, sandbox), **task}, NO_CORES)
    prompt = prompt.replace(arms.VIRTUAL_WAIT, arms.REAL_WAIT)
    if not notes:
        return prompt
    behavior = arms.sx_guide(workflow, sandbox, notes=True).strip()
    if not behavior:
        return prompt
    reply = prompt.rindex("Reply with the module")
    return f"{prompt[:reply]}## How the tools behave\n\n{behavior}\n\n{prompt[reply:]}"


def verdict_of(workflow: Workflow, sandbox: Sandbox, run: ArmRun, program: str) -> tuple[str, str]:
    if run.status != "completed":
        return run.status, run.detail
    verdict = workflow.check(sandbox, run.result, Evidence(program, run.transcript))
    return verdict.status, verdict.detail


def reference(workflow: Workflow, arm: Arm) -> tuple[ArmRun, str, str]:
    with workflow.sandbox() as sandbox:
        program = workflow.render(workflow.reference(arm), sandbox)
        run = arms.run_arm(arm, workflow, sandbox, program)
        status, detail = verdict_of(workflow, sandbox, run, program)
    return run, status, detail


def run_trial(
    workflow: Workflow, arm: Arm, model: str, thinking: str, level: str, sample: int, out: Path, ask: Ask = ask_model,
    *, ts_notes: bool,
) -> Trial:
    js_label = "js-notes" if ts_notes else "js"
    directory = out / workflow.id / f"{js_label if arm == 'js' else 'sx-' + level}-{sample}"
    directory.mkdir(parents=True, exist_ok=True)
    attempts: list[Attempt] = []
    extras: list[Json] = []
    with workflow.sandbox() as sandbox:
        original = (
            sx_prompt(workflow, sandbox, level) if arm == "sx" else js_arm_prompt(workflow, sandbox, notes=ts_notes)
        )
        prompt = original
        for index in range(REPAIRS + 1):
            prompt_file = directory / f"prompt-{index}.md"
            prompt_file.write_text(prompt)
            asked = time.perf_counter()
            reply, tokens, error = ask(prompt_file, model, thinking, out)
            model_seconds = time.perf_counter() - asked
            code, _ = extract(reply)
            if not code.strip():
                attempts.append(Attempt(reply, code, tokens, error, "no-answer", error or "empty reply", None))
                extras.append({"model_seconds": model_seconds})
                break
            sandbox.reset()
            workflow.setup(sandbox)
            run = arms.run_arm(arm, workflow, sandbox, code)
            status, detail = verdict_of(workflow, sandbox, run, code)
            attempt = Attempt(reply, code, tokens, error, status, detail, run.feedback, run.labels, len(run.calls))
            attempts.append(attempt)
            extras.append({
                "model_seconds": model_seconds, "run_seconds": run.seconds, "program_seconds": run.program_seconds,
                "explain": run.explain,
            })
            if status == "correct" or run.feedback is None:
                break
            prompt = repair_prompt(original, attempts, LANGUAGE[arm])
    guide_level = level if arm == "sx" else ("notes" if ts_notes else "none")
    trial = Trial(workflow.id, arm, model, thinking, guide_level, sample, attempts)
    (directory / "trial.json").write_text(json.dumps(asdict(trial), indent=2))
    (directory / "extras.json").write_text(json.dumps(extras, indent=2))
    return trial
