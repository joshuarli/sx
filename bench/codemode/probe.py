import argparse
import json
import random
import subprocess
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict, dataclass, field
from pathlib import Path
from statistics import mean
from typing import Any

from cores import task_registry
from pilot import ask, extract, task_section
from typescript import declarations

from sx.checker import check
from sx.diagnostics import render
from sx.help import guide
from sx.host import FixtureHost, UnexpectedCallError, run
from sx.machine import Completed, Failed
from sx.reader import read

BENCH = Path(__file__).parent
ROOT = BENCH.parent.parent
TSC = BENCH / "node_modules" / ".bin" / "tsc"
TSCONFIG = {
    "compilerOptions": {
        "strict": True, "noEmit": True, "target": "es2023", "lib": ["es2023"], "module": "nodenext",
        "moduleResolution": "nodenext", "erasableSyntaxOnly": True, "skipLibCheck": True, "types": [],
    },
    "files": ["api.d.ts", "solution.mts"],
}
REPAIRS = 2
FEEDBACK_LINES = 40
type Json = dict[str, Any]


@dataclass(slots=True)
class Attempt:
    reply: str
    code: str
    tokens: dict[str, int]
    error: str | None
    status: str
    detail: str
    feedback: str | None
    diagnostics: list[str] = field(default_factory=list)
    calls: int = 0
    duplicate_calls: int = 0


@dataclass(slots=True)
class Trial:
    task: str
    arm: str
    model: str
    thinking: str
    sample: int
    attempts: list[Attempt]

    @property
    def first_correct(self) -> bool:
        return self.attempts[0].status == "correct"

    @property
    def final_correct(self) -> bool:
        return self.attempts[-1].status == "correct"


def load_tasks(split: str, only: list[str] | None) -> list[Json]:
    tasks = []
    for path in sorted((BENCH / "tasks").glob("*/task.json")):
        task = json.loads(path.read_text())
        if task["split"] == split and task["sx-expressible"] and (only is None or task["id"] in only):
            tasks.append(task)
    return tasks


def reference_delays(task: Json) -> list[int]:
    reference = BENCH / "tasks" / task["id"] / "reference.mjs"
    output = node_run(task, reference)
    assert output["outcome"] == "completed", output
    return list(output["log"]["delays"])


def node_run(task: Json, solution: Path) -> Json:
    task_file = BENCH / "tasks" / task["id"] / "task.json"
    completed = subprocess.run(
        ["node", str(BENCH / "run_js.mjs"), str(task_file), str(solution)],
        stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=60, check=False,
    )
    if completed.returncode != 0 or not completed.stdout.startswith("{"):
        log: Json = {"calls": [], "delays": []}
        return {"outcome": "failed", "kind": "exception", "error": completed.stderr.strip()[-2000:], "log": log}
    result: Json = json.loads(completed.stdout)
    return result


def sx_prompt(task: Json) -> str:
    return f"{guide()}\n\n{task_section(task)}\n"


def js_prompt(task: Json, cores: Json) -> str:
    return "\n".join([
        "## Your task", "", task["description"], "",
        "Write a TypeScript module that solves the task. Its default export is `async function main(api: Api)`, "
        "which returns the result as plain JSON data. The tools are methods on `api`, declared below. A failed "
        "tool call throws an error with `kind` and `message`. `await api.sleep(ms)` waits. The module uses no "
        "imports, and the types below are already in scope.", "",
        "```ts", declarations(task, cores).rstrip(), "```", "",
        "Reply with the module in one ```ts code block, and nothing else.", "",
    ])


def duplicates(calls: list[Json], task: Json) -> int:
    repeatable = {
        json.dumps([r["capability"], r["input"]], sort_keys=True) for r in task["responses"] if len(r["results"]) > 1
    }
    seen: Counter[str] = Counter()
    for call in calls:
        identity = json.dumps([call["capability"], call["input"]], sort_keys=True)
        seen[identity] += 1
    return sum(count - 1 for identity, count in seen.items() if count > 1 and identity not in repeatable)


def unexpected(message: str) -> str:
    return f"A tool call failed because the tool rejected its input: {message}"


type Verdict = tuple[str, str, str | None, list[str], int, int]


def judge(
    task: Json, result: object, delays: list[int], wanted_delays: list[int], writes_missing: str | None
) -> tuple[str, str]:
    if writes_missing is not None:
        return "wrong", writes_missing
    if result != task["expected"]:
        return "wrong", f"wrong result: {json.dumps(result)}"
    if sorted(delays) != sorted(wanted_delays):
        return "wrong", f"waited {sorted(delays)} instead of {sorted(wanted_delays)}"
    return "correct", ""


def evaluate_sx(task: Json, code: str, wanted_delays: list[int]) -> Verdict:
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


def evaluate_js(task: Json, code: str, wanted_delays: list[int], workdir: Path, cores: Json) -> Verdict:
    workdir.mkdir(parents=True, exist_ok=True)
    (workdir / "api.d.ts").write_text(declarations(task, cores))
    (workdir / "solution.mts").write_text(code)
    (workdir / "tsconfig.json").write_text(json.dumps(TSCONFIG))
    typed = subprocess.run(
        [str(TSC), "-p", str(workdir)],
        stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=120, check=False,
    )
    if typed.returncode != 0:
        errors = "\n".join((typed.stdout + typed.stderr).replace(str(workdir) + "/", "").splitlines()[:FEEDBACK_LINES])
        codes = sorted({line.split("error ")[1].split(":")[0] for line in errors.splitlines() if " error TS" in line})
        return "rejected", "", errors, codes, 0, 0
    output = node_run(task, workdir / "solution.mts")
    calls, dups = len(output["log"]["calls"]), duplicates(output["log"]["calls"], task)
    if output["outcome"] == "failed":
        message = str(output["error"])
        feedback = unexpected(message) if output["kind"] == "host" else f"The module threw: {message}"
        return "failed", message, feedback, [], calls, dups
    writes = output.get("reason") if output["outcome"] == "wrong" else None
    status, detail = judge(task, output.get("result"), list(output["log"]["delays"]), wanted_delays, writes)
    return status, detail, None, [], calls, dups


def repair_prompt(original: str, attempts: list[Attempt], language: str) -> str:
    parts = [original.rstrip(), ""]
    for attempt in attempts:
        feedback = attempt.feedback if attempt.feedback is not None else ""
        parts += ["Your previous answer:", f"```{language}", attempt.code, "```", "", "It failed:", feedback, ""]
    parts.append(f"Reply with a corrected answer in one ```{language} code block, and nothing else.")
    return "\n".join(parts) + "\n"


def run_trial(
    task: Json, arm: str, model: str, thinking: str, sample: int, out: Path, cores: Json, wanted: list[int]
) -> Trial:
    directory = out / task["id"] / f"{arm}-{sample}"
    directory.mkdir(parents=True, exist_ok=True)
    original = sx_prompt(task) if arm == "sx" else js_prompt(task, cores)
    language = "sx" if arm == "sx" else "ts"
    attempts: list[Attempt] = []
    prompt = original
    for index in range(REPAIRS + 1):
        prompt_file = directory / f"prompt-{index}.md"
        prompt_file.write_text(prompt)
        reply, tokens, error = ask(prompt_file, model, thinking, out)
        code, _ = extract(reply)
        if not code.strip():
            attempts.append(Attempt(reply, code, tokens, error, "no-answer", error or "empty reply", None))
            break
        if arm == "sx":
            status, detail, feedback, codes, calls, dups = evaluate_sx(task, code, wanted)
        else:
            workdir = directory / f"attempt-{index}"
            status, detail, feedback, codes, calls, dups = evaluate_js(task, code, wanted, workdir, cores)
        attempts.append(Attempt(reply, code, tokens, error, status, detail, feedback, codes, calls, dups))
        if status == "correct" or feedback is None:
            break
        prompt = repair_prompt(original, attempts, language)
    trial = Trial(task["id"], arm, model, thinking, sample, attempts)
    (directory / "trial.json").write_text(json.dumps(asdict(trial), indent=2))
    return trial


def load_trials(out: Path) -> list[Trial]:
    trials = []
    for path in sorted(out.glob("*/*/trial.json")):
        saved = json.loads(path.read_text())
        attempts = [Attempt(**a) for a in saved.pop("attempts")]
        trials.append(Trial(**saved, attempts=attempts))
    return trials


def bootstrap(differences: list[float], rounds: int = 10_000, seed: int = 7) -> tuple[float, float]:
    rng = random.Random(seed)
    means = sorted(mean(rng.choices(differences, k=len(differences))) for _ in range(rounds))
    return means[int(rounds * 0.025)], means[int(rounds * 0.975)]


def tokens(trial: Trial) -> int:
    return sum(a.tokens.get("input", 0) + a.tokens.get("output", 0) for a in trial.attempts)


def report(trials: list[Trial], expressiveness: tuple[int, int]) -> str:
    lines = []
    for model in sorted({t.model for t in trials}):
        mine = [t for t in trials if t.model == model]
        tasks = sorted({t.task for t in mine})
        paired = [task for task in tasks if {t.arm for t in mine if t.task == task} == {"sx", "js"}]
        lines += [f"## {model}", "", f"Tasks with both arms: {len(paired)}", ""]
        rows = {}
        for arm in ("sx", "js"):
            arm_trials = [t for t in mine if t.arm == arm and t.task in paired]
            per_task_first = {task: mean(t.first_correct for t in arm_trials if t.task == task) for task in paired}
            per_task_final = {task: mean(t.final_correct for t in arm_trials if t.task == task) for task in paired}
            solved = sum(t.final_correct for t in arm_trials)
            total_tokens = sum(tokens(t) for t in arm_trials)
            rows[arm] = (per_task_first, per_task_final, solved, total_tokens, arm_trials)
        lines += [
            "| Arm | First attempt | Final | Trials | Tokens per solved task |",
            "| --- | --- | --- | --- | --- |",
        ]
        for arm, (first, final, solved, total, arm_trials) in rows.items():
            per_solved = f"{total / solved:,.0f}" if solved else "none solved"
            rates = f"{mean(first.values()):.1%} | {mean(final.values()):.1%}"
            row = f"| {arm} | {rates} | {len(arm_trials)} | {per_solved} |"
            lines.append(row)
        first_diff = [rows["sx"][0][t] - rows["js"][0][t] for t in paired]
        final_diff = [rows["sx"][1][t] - rows["js"][1][t] for t in paired]
        low, high = bootstrap(first_diff)
        final_low, final_high = bootstrap(final_diff)
        lines += [
            "",
            f"Sx minus JS, first attempt: {mean(first_diff):+.1%} (95% interval {low:+.1%} to {high:+.1%})",
            f"Sx minus JS, final: {mean(final_diff):+.1%} (95% interval {final_low:+.1%} to {final_high:+.1%})",
        ]
        sx_tps = rows["sx"][3] / rows["sx"][2] if rows["sx"][2] else float("inf")
        js_tps = rows["js"][3] / rows["js"][2] if rows["js"][2] else float("inf")
        within = mean(first_diff) >= -0.05
        better = mean(final_diff) > 0 or sx_tps < js_tps
        verdict = "PASS" if within and better else "FAIL"
        lines += [
            f"Gate: first attempt within 5 points: {'yes' if within else 'no'}; "
            f"better on final correctness or tokens per solved task: {'yes' if better else 'no'}; {verdict}",
            "",
        ]
        sx_trials = [t for t in mine if t.arm == "sx"]
        fixed: Counter[str] = Counter()
        seen: Counter[str] = Counter()
        for trial in sx_trials:
            for before, after in zip(trial.attempts, trial.attempts[1:], strict=False):
                for code in set(before.diagnostics):
                    seen[code] += 1
                    fixed[code] += code not in after.diagnostics
        if seen:
            lines += ["Sx diagnostics and whether the next attempt fixed them:", ""]
            lines += [f"- {code}: fixed {fixed[code]} of {count}" for code, count in seen.most_common()]
            lines.append("")
        outcomes: Counter[str] = Counter()
        for trial in mine:
            outcomes[f"{trial.arm} {trial.attempts[-1].status}"] += 1
        lines += ["Final status counts: " + ", ".join(f"{k} {v}" for k, v in sorted(outcomes.items())), ""]
    missing, total = expressiveness
    lines.append(f"Expressiveness gap: {missing} of {total} held-out tasks need a type Sx v0 lacks.")
    return "\n".join(str(line) for line in lines)


def main() -> None:
    parser = argparse.ArgumentParser(description="Compare Sx and TypeScript on the probe tasks.")
    parser.add_argument("--split", choices=["development", "held-out"], default="development")
    parser.add_argument("--model", default="opencode-go/muse-spark-1.3-contributor")
    parser.add_argument("--thinking", default="medium")
    parser.add_argument("--samples", type=int, default=2)
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--arm", action="append", choices=["sx", "js"])
    parser.add_argument("--task", action="append")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--report", action="store_true", help="Report on saved trials without calling a model")
    args = parser.parse_args()
    args.out = args.out.resolve()
    all_tasks = [json.loads(p.read_text()) for p in (BENCH / "tasks").glob("*/task.json")]
    held_out = [t for t in all_tasks if t["split"] == "held-out"]
    gap = (sum(1 for t in held_out if not t["sx-expressible"]), len(held_out))
    if args.report:
        print(report(load_trials(args.out), gap))
        return
    cores = json.loads((BENCH / "cores.json").read_text())
    tasks = load_tasks(args.split, args.task)
    arms = args.arm or ["sx", "js"]
    args.out.mkdir(parents=True, exist_ok=True)
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        jobs = []
        for task in tasks:
            wanted = reference_delays(task)
            for arm in arms:
                for sample in range(args.samples):
                    settings = (args.model, args.thinking, sample, args.out, cores, wanted)
                    jobs.append(pool.submit(run_trial, task, arm, *settings))
        for job in jobs:
            trial = job.result()
            print(f"{trial.task:28} {trial.arm} {trial.sample} {[a.status for a in trial.attempts]}", flush=True)
    print(report(load_trials(args.out), gap))


if __name__ == "__main__":
    main()
