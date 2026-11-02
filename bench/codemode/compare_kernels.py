import argparse
import json
from collections import Counter
from collections.abc import Iterator
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from pathlib import Path

from cores import task_registry
from probe import BENCH, ROOT, Json, Verdict, duplicates, evaluate_sx, judge, reference_delays, unexpected

from sx.checker import check
from sx.diagnostics import render
from sx.host import FixtureHost, UnexpectedCallError, run
from sx.machine import Completed, Failed
from sx.reader import read

ALLOWED_PREFIXES = ("dev-", "held-")
BLIND_PREFIX = "fresh-"


def evaluate_sx_python(task: Json, code: str, wanted_delays: list[int]) -> Verdict:
    parsed = read(code)
    if parsed.program is None:
        return "rejected", "", render(code, parsed.diagnostics), [d.label for d in parsed.diagnostics], 0, 0
    registry = task_registry(task)
    allowed = frozenset(c["id"] for c in task["capabilities"])
    checked = check(parsed.program, registry, allowed=allowed)
    if checked.program is None:
        return "rejected", "", render(code, checked.diagnostics), [d.label for d in checked.diagnostics], 0, 0
    host = FixtureHost.from_responses(registry, task["responses"])
    try:
        finished = run(checked.program, host)
    except UnexpectedCallError as error:
        return "failed", str(error), unexpected(str(error)), [], len(host.calls), duplicates(host.calls, task)
    calls, dups = len(host.calls), duplicates(host.calls, task)
    outcome = finished.outcome
    if isinstance(outcome, Failed):
        failure = outcome.failure
        where = f" at {failure.span}" if failure.span is not None else ""
        message = f"The program failed{where} with {failure.kind.value}: {failure.message}"
        return "failed", message, message, [], calls, dups
    assert isinstance(outcome, Completed)
    missing = host.unperformed_writes()
    writes = None if not missing else f"a required write did not happen: {missing[0].capability}"
    status, detail = judge(task, outcome.json, host.delays, wanted_delays, writes)
    return status, detail, None, [], calls, dups


@dataclass(frozen=True, slots=True)
class Saved:
    name: str
    task: str
    code: str


def saved_answers(probe: Path) -> Iterator[Saved]:
    for run_directory in sorted(probe.iterdir()):
        if not run_directory.is_dir() or run_directory.name.startswith(BLIND_PREFIX):
            continue
        for task_directory in sorted(run_directory.iterdir()):
            if not task_directory.is_dir() or not task_directory.name.startswith(ALLOWED_PREFIXES):
                continue
            for trial_directory in sorted(task_directory.iterdir()):
                trial_file = trial_directory / "trial.json"
                if not trial_file.is_file():
                    continue
                trial = json.loads(trial_file.read_text())
                if trial["arm"] != "sx" or not str(trial["task"]).startswith(ALLOWED_PREFIXES):
                    continue
                for index, attempt in enumerate(trial["attempts"]):
                    if attempt["status"] != "no-answer" and attempt["code"].strip():
                        name = f"{run_directory.name}/{task_directory.name}/{trial_directory.name}#{index}"
                        yield Saved(name, trial["task"], attempt["code"])


def main() -> None:
    parser = argparse.ArgumentParser(description="Score every saved Sx answer with the Python and Rust kernels.")
    parser.add_argument("--probe", type=Path, default=ROOT / ".work" / "probe")
    parser.add_argument("--jobs", type=int, default=8)
    args = parser.parse_args()
    answers = list(saved_answers(args.probe))
    tasks: dict[str, Json] = {}
    delays: dict[str, list[int]] = {}
    for answer in answers:
        if answer.task not in tasks:
            tasks[answer.task] = json.loads((BENCH / "tasks" / answer.task / "task.json").read_text())
            delays[answer.task] = reference_delays(tasks[answer.task])
    unique = sorted({(a.task, a.code) for a in answers})

    def through_rust(key: tuple[str, str]) -> Verdict:
        task_id, code = key
        return evaluate_sx(tasks[task_id], code, delays[task_id])

    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        rust = dict(zip(unique, pool.map(through_rust, unique), strict=True))
    disagreements: list[str] = []
    statuses: Counter[str] = Counter()
    for answer in answers:
        key = (answer.task, answer.code)
        python = evaluate_sx_python(tasks[answer.task], answer.code, delays[answer.task])
        statuses[python[0]] += 1
        if python != rust[key]:
            disagreements.append(answer.name)
    print(f"{len(answers)} attempts compared across {len(tasks)} tasks, {len(unique)} distinct programs")
    print(f"python verdicts: {dict(sorted(statuses.items()))}")
    print(f"{len(disagreements)} disagreements")
    for name in disagreements:
        print(name)


if __name__ == "__main__":
    main()
