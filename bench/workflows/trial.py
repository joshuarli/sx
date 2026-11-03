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
type Ask = Callable[[Path, str, str, Path], tuple[str, dict[str, int], str | None, float]]

LANGUAGE: dict[str, str] = {"sx": "sx", "js": "ts", "ts": "ts", "jsbody": "ts"}
BODY_ARMS = ("ts", "jsbody")


TAILORED = "tailored"
VARIANTS = ("slim",)
REPLY_SX = "Reply with one Sx program in a ```sx code block, and nothing else."
BODY_INSTRUCTION = (
    "Write the body of an async function that solves the task. It calls the tools as "
    "`tools.namespace.name({ ... })` with `await`, may print lines with `console.log`, and `return`s the final "
    "answer as plain JSON data. The declarations below are already in scope."
)
REPLY_BODY = "Reply with the body in one ```ts code block, and nothing else."


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


def body_prompt(workflow: Workflow, sandbox: Sandbox, *, rules: bool) -> str:
    task = workflow.render(workflow.task, sandbox).strip()
    guide = arms.typescript_guide_for(workflow, sandbox, rules=rules)
    return "\n".join(["## Your task", "", task, "", BODY_INSTRUCTION, "", guide, "", REPLY_BODY]) + "\n"


def condition_folder(arm: Arm, guide: str) -> str:
    match arm:
        case "sx":
            return f"sx-{guide}"
        case "js":
            return "js-notes" if guide == "notes" else "js"
        case _:
            return arm


def initial_prompt(workflow: Workflow, sandbox: Sandbox, arm: Arm, level: str, *, ts_notes: bool) -> str:
    match arm:
        case "sx":
            return sx_prompt(workflow, sandbox, level)
        case "js":
            return js_arm_prompt(workflow, sandbox, notes=ts_notes)
        case "ts" | "jsbody":
            return body_prompt(workflow, sandbox, rules=arm == "ts")


def guide_level_of(arm: Arm, level: str, *, ts_notes: bool) -> str:
    match arm:
        case "sx":
            return level
        case "js":
            return "notes" if ts_notes else "none"
        case _:
            return "rules" if arm == "ts" else "none"


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
    *, ts_notes: bool, workflow_set: str = "",
) -> Trial:
    guide_level = guide_level_of(arm, level, ts_notes=ts_notes)
    directory = out / workflow.id / f"{condition_folder(arm, guide_level)}-{sample}"
    directory.mkdir(parents=True, exist_ok=True)
    attempts: list[Attempt] = []
    extras: list[Json] = []
    with workflow.sandbox() as sandbox:
        original = initial_prompt(workflow, sandbox, arm, level, ts_notes=ts_notes)
        prompt = original
        for index in range(REPAIRS + 1):
            prompt_file = directory / f"prompt-{index}.md"
            prompt_file.write_text(prompt)
            asked = time.perf_counter()
            reply, tokens, error, cost = ask(prompt_file, model, thinking, out)
            model_seconds = time.perf_counter() - asked
            code, _ = extract(reply)
            if not code.strip():
                attempts.append(
                    Attempt(reply, code, tokens, error, "no-answer", error or "empty reply", None, cost=cost)
                )
                extras.append({"model_seconds": model_seconds})
                break
            sandbox.reset()
            workflow.setup(sandbox)
            run = arms.run_arm(arm, workflow, sandbox, code)
            status, detail = verdict_of(workflow, sandbox, run, code)
            attempt = Attempt(
                reply, code, tokens, error, status, detail, run.feedback, run.labels, len(run.calls), cost=cost,
            )
            attempts.append(attempt)
            extras.append({
                "model_seconds": model_seconds, "run_seconds": run.seconds, "program_seconds": run.program_seconds,
                "explain": run.explain,
            })
            if status == "correct" or run.feedback is None:
                break
            prompt = repair_prompt(original, attempts, LANGUAGE[arm])
    trial = Trial(workflow.id, arm, model, thinking, guide_level, sample, attempts, workflow_set)
    (directory / "trial.json").write_text(json.dumps(asdict(trial), indent=2))
    (directory / "extras.json").write_text(json.dumps(extras, indent=2))
    return trial
