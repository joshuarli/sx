import json
from collections import Counter
from pathlib import Path
from statistics import mean
from typing import Any

import arms
import definition
from probe import Trial, load_trials, tokens

type Json = dict[str, Any]
HARNESS_FILES = ("mcp.ts", "api.ts", "run.ts")


def lines_of(path: Path) -> int:
    return sum(1 for line in path.read_text().splitlines() if line.strip())


def extras_of(out: Path, trial: Trial) -> list[Json]:
    js_folder = "js-notes" if trial.guide == "notes" else "js"
    folder = js_folder if trial.arm == "js" else f"sx-{trial.guide}"
    path = out / trial.task / f"{folder}-{trial.sample}" / "extras.json"
    loaded: list[Json] = json.loads(path.read_text())
    return loaded


def output_tokens(trial: Trial) -> int:
    return sum(a.tokens.get("output", 0) for a in trial.attempts)


def table(out: Path, trials: list[Trial]) -> list[str]:
    lines = [
        "| Workflow | Arm | Trials | First attempt | Final | Attempts | Tokens per trial | Output per solved task "
        "| Model seconds | Run seconds |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for workflow in sorted({t.task for t in trials}):
        for arm in ("sx", "js"):
            mine = [t for t in trials if t.task == workflow and t.arm == arm]
            if not mine:
                continue
            solved = sum(t.final_correct for t in mine)
            extras = [extras_of(out, t) for t in mine]
            model = mean(sum(e["model_seconds"] for e in each) for each in extras)
            run = mean(sum(e.get("run_seconds", 0.0) for e in each) for each in extras)
            per_solved = f"{sum(output_tokens(t) for t in mine) / solved:,.0f}" if solved else "none solved"
            lines.append(
                f"| {workflow} | {arm} | {len(mine)} | {sum(t.first_correct for t in mine)}/{len(mine)} "
                f"| {solved}/{len(mine)} | {mean(len(t.attempts) for t in mine):.1f} "
                f"| {mean(tokens(t) for t in mine):,.0f} | {per_solved} | {model:.0f} | {run:.1f} |"
            )
    return lines


def host_code(ids: list[str], root: Path) -> list[str]:
    harness = sum(lines_of(Path(__file__).parent / "harness" / name) for name in HARNESS_FILES)
    lines = [
        "| Workflow | declarations.json | grants.json | server.json | api.d.ts (generated, TypeScript only) |",
        "| --- | --- | --- | --- | --- |",
    ]
    for workflow_id in ids:
        workflow = definition.load(workflow_id, root)
        server = workflow.directory / "server.json"
        with workflow.sandbox() as sandbox:
            declarations = arms.typescript_declarations(
                {**arms.described_for(workflow, sandbox), "cores": []}, arms.NO_CORES
            )
        shown = lines_of(server) if server.is_file() else 0
        lines.append(
            f"| {workflow_id} | {lines_of(workflow.directory / 'declarations.json')} "
            f"| {lines_of(workflow.directory / 'grants.json')} | {shown} | {len(declarations.splitlines())} |"
        )
    files = len(HARNESS_FILES)
    lines += ["", f"The Bun harness shared by every workflow is {harness} non-blank lines in {files} files."]
    return lines


def explained(out: Path, trials: list[Trial]) -> list[str]:
    lines = [
        "| Workflow | Sx answers | Explained before running | Tools called | Scope verdicts |",
        "| --- | --- | --- | --- | --- |",
    ]
    for workflow in sorted({t.task for t in trials}):
        answers: list[Json] = []
        for trial in (t for t in trials if t.task == workflow and t.arm == "sx"):
            pairs = zip(extras_of(out, trial), trial.attempts, strict=True)
            answers += [e for e, a in pairs if a.status != "no-answer"]
        reports = [e["explain"] for e in answers if e.get("explain") is not None]
        tools: Counter[str] = Counter()
        verdicts: Counter[str] = Counter()
        for each in reports:
            for tool in each["tools"]:
                tools[f"{tool['id']}{'' if tool['allowed'] else ' (not allowed)'}"] += 1
                verdicts.update(v["verdict"] for v in tool.get("verdicts", []))
        shown_tools = ", ".join(f"{name} x{count}" for name, count in sorted(tools.items())) or "none"
        shown_verdicts = ", ".join(f"{name} {count}" for name, count in sorted(verdicts.items())) or "none"
        lines.append(f"| {workflow} | {len(answers)} | {len(reports)} | {shown_tools} | {shown_verdicts} |")
    return lines


def render(out: Path, root: Path) -> str:
    trials = load_trials(out)
    ids = sorted({t.task for t in trials})
    return "\n".join([
        "## Correctness, tokens, and time", "", *table(out, trials), "",
        "## Host code", "", *host_code(ids, root), "",
        "## What `sx explain` reported before the Sx answers ran", "", *explained(out, trials), "",
    ])
