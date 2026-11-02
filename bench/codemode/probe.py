import argparse
import json
import os
import random
import subprocess
import tempfile
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict, dataclass, field
from pathlib import Path
from statistics import mean
from typing import Any

from cores import SIGNATURES
from pilot import ask, extract, task_section
from typescript import declarations

from sx.help import GUIDE_LEVELS, guide

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
WAIT_KINDS = frozenset({"retry", "polling"})
QUICK_TASKS = (
    "dev-retry-ci", "dev-wait-for-job", "dev-events-pages", "dev-ci-partial", "dev-tag-counts", "dev-csv-summary",
)
FEEDBACK_LINES = 40
SX_TIMEOUT = 60
SX_EXITS = {"ok": 0, "rejected": 1, "failed": 2}
DECLARATION_KEYS = ("types", "enums", "cores", "capabilities")
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
    guide: str
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


TAILORED = "tailored"


def tailored_guide(task: Json) -> str:
    scratch = ROOT / ".work" / "tmp"
    scratch.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=scratch) as workdir:
        declarations_file, _, allowed = cli_task(task, Path(workdir))
        completed = sx_run([
            "guide", "--declarations", str(declarations_file), *(a for tool in allowed for a in ("--allow", tool)),
        ])
    if completed.returncode != 0:
        raise RuntimeError(f"sx guide exited {completed.returncode}: {completed.stderr.strip()}")
    return completed.stdout


def sx_prompt(task: Json, level: str) -> str:
    if level == TAILORED:
        reply = "Reply with one Sx program in a ```sx code block, and nothing else."
        return "\n".join([tailored_guide(task).rstrip(), "", "## Your task", "", task["description"], "", reply]) + "\n"
    return f"{guide(level)}\n\n{task_section(task)}\n"


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
    states_a_wait_policy = bool(WAIT_KINDS & set(task["kinds"]))
    waited, wanted = sorted(d for d in delays if d > 0), sorted(d for d in wanted_delays if d > 0)
    if states_a_wait_policy and waited != wanted:
        return "wrong", f"waited {waited} instead of {wanted}"
    return "correct", ""


def sx_binary() -> Path:
    override = os.environ.get("SX_BIN")
    if override is not None:
        binary = Path(override)
        if not binary.is_file():
            raise FileNotFoundError(f"SX_BIN={override} is not a file")
        return binary
    candidates = [ROOT / "rust" / "target" / profile / "sx" for profile in ("release", "debug")]
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    searched = " or ".join(str(c) for c in candidates)
    raise FileNotFoundError(
        f"no sx binary at {searched}; build one with `cargo build --release --manifest-path rust/Cargo.toml -p sx-cli`"
        " or point SX_BIN at one"
    )


def cli_task(task: Json, directory: Path) -> tuple[Path, Path, list[str]]:
    declarations_file, responses_file = directory / "declarations.json", directory / "responses.json"
    declarations_file.write_text(json.dumps({key: task[key] for key in DECLARATION_KEYS if key in task}))
    responses_file.write_text(json.dumps(task["responses"]))
    return declarations_file, responses_file, [c["id"] for c in task["capabilities"]]


def sx_run(arguments: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(sx_binary()), *arguments],
        stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=SX_TIMEOUT, check=False,
    )


def label(diagnostic: Json) -> str:
    code, construct = str(diagnostic["code"]), diagnostic["construct"]
    return code if construct is None else f"{code}:{construct}"


def show_span(span: Json) -> str:
    return f"{span['start']['line']}:{span['start']['column']}-{span['end']['line']}:{span['end']['column']}"


def unperformed_write(task: Json, calls: list[Json]) -> str | None:
    writes = {c["id"] for c in task["capabilities"] if c["effect"] == "write"}
    served: Counter[int] = Counter()
    for call in calls:
        for index, entry in enumerate(task["responses"]):
            if entry["capability"] == call["capability"] and entry["input"] == call["input"]:
                served[index] += 1
                break
    for index, entry in enumerate(task["responses"]):
        if entry["capability"] in writes and served[index] != len(entry["results"]):
            return str(entry["capability"])
    return None


def evaluate_sx(task: Json, code: str, wanted_delays: list[int]) -> Verdict:
    scratch = ROOT / ".work" / "tmp"
    scratch.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=scratch) as workdir:
        directory = Path(workdir)
        declarations_file, responses_file, allowed = cli_task(task, directory)
        program = directory / "program.sx"
        program.write_bytes(code.encode())
        arguments = [
            str(program), "--declarations", str(declarations_file), *(a for tool in allowed for a in ("--allow", tool)),
        ]
        completed = sx_run(["run", *arguments, "--responses", str(responses_file), "--format", "json"])
        if completed.returncode == SX_EXITS["rejected"]:
            diagnostics = json.loads(completed.stdout)["diagnostics"]
            rendered = sx_run(["check", *arguments]).stdout
            return "rejected", "", rendered, [label(d) for d in diagnostics], 0, 0
        if completed.returncode not in (SX_EXITS["ok"], SX_EXITS["failed"]):
            raise RuntimeError(f"sx run exited {completed.returncode}: {completed.stderr.strip()}")
    report = json.loads(completed.stdout)
    if report["outcome"] == "unexpected-call":
        message = str(report["message"])
        seen: list[Json] = report["calls"]
        return "failed", message, unexpected(message), [], len(seen), duplicates(seen, task)
    calls = [{"capability": e["capability"], "input": e["input"]} for e in report["trace"] if "request" in e]
    delays = [e["milliseconds"] for e in report["trace"] if "delay" in e]
    count, dups = len(calls), duplicates(calls, task)
    if report["outcome"] == "failed":
        failure = report["failure"]
        where = f" at {show_span(failure['span'])}" if failure["span"] is not None else ""
        message = f"The program failed{where} with {failure['kind']}: {failure['message']}"
        return "failed", message, message, [], count, dups
    missing = unperformed_write(task, calls)
    writes = None if missing is None else f"a required write did not happen: {missing}"
    status, detail = judge(task, report["result"], delays, wanted_delays, writes)
    return status, detail, None, [], count, dups


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
    task: Json, arm: str, level: str, model: str, thinking: str, sample: int, out: Path, cores: Json, wanted: list[int]
) -> Trial:
    label = arm if arm == "js" else f"sx-{level}"
    directory = out / task["id"] / f"{label}-{sample}"
    directory.mkdir(parents=True, exist_ok=True)
    original = sx_prompt(task, level) if arm == "sx" else js_prompt(task, cores)
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
    trial = Trial(task["id"], arm, model, thinking, level if arm == "sx" else "none", sample, attempts)
    (directory / "trial.json").write_text(json.dumps(asdict(trial), indent=2))
    return trial


def rejudge(out: Path, cores: Json) -> int:
    tasks: dict[str, Json] = {}
    changed = 0
    for path in sorted(out.glob("*/*/trial.json")):
        trial = load_trial(path)
        if trial.task not in tasks:
            tasks[trial.task] = json.loads((BENCH / "tasks" / trial.task / "task.json").read_text())
        task = tasks[trial.task]
        wanted = reference_delays(task)
        kept: list[Attempt] = []
        for index, attempt in enumerate(trial.attempts):
            if attempt.status != "no-answer":
                if trial.arm == "sx":
                    status, detail, *_ = evaluate_sx(task, attempt.code, wanted)
                else:
                    workdir = path.parent / f"attempt-{index}"
                    status, detail, *_ = evaluate_js(task, attempt.code, wanted, workdir, cores)
                if (status, detail) != (attempt.status, attempt.detail):
                    changed += 1
                    attempt.status, attempt.detail = status, detail
            kept.append(attempt)
            if attempt.status == "correct":
                break
        trial.attempts = kept
        path.write_text(json.dumps(asdict(trial), indent=2))
    return changed


def load_trial(path: Path) -> Trial:
    saved = json.loads(path.read_text())
    attempts = [Attempt(**a) for a in saved.pop("attempts")]
    return Trial(**saved, attempts=attempts)


def load_trials(out: Path) -> list[Trial]:
    return [load_trial(path) for path in sorted(out.glob("*/*/trial.json"))]


def bootstrap(differences: list[float], rounds: int = 10_000, seed: int = 7) -> tuple[float, float]:
    rng = random.Random(seed)
    means = sorted(mean(rng.choices(differences, k=len(differences))) for _ in range(rounds))
    return means[int(rounds * 0.025)], means[int(rounds * 0.975)]


def tokens(trial: Trial) -> int:
    return sum(a.tokens.get("input", 0) + a.tokens.get("output", 0) for a in trial.attempts)


def condition(trial: Trial) -> str:
    return "js" if trial.arm == "js" else f"sx-{trial.guide}"


def summarize(arm_trials: list[Trial], tasks: list[str]) -> dict[str, Any]:
    first = {task: mean(t.first_correct for t in arm_trials if t.task == task) for task in tasks}
    final = {task: mean(t.final_correct for t in arm_trials if t.task == task) for task in tasks}
    solved = sum(t.final_correct for t in arm_trials)
    total = sum(tokens(t) for t in arm_trials)
    output = sum(sum(a.tokens.get("output", 0) for a in t.attempts) for t in arm_trials)
    return {
        "first": first,
        "final": final,
        "solved": solved,
        "trials": len(arm_trials),
        "prompt": mean(t.attempts[0].tokens.get("input", 0) for t in arm_trials),
        "input": mean(sum(a.tokens.get("input", 0) for a in t.attempts) for t in arm_trials),
        "output": mean(sum(a.tokens.get("output", 0) for a in t.attempts) for t in arm_trials),
        "reasoning": mean(sum(a.tokens.get("reasoning", 0) for a in t.attempts) for t in arm_trials),
        "attempts": mean(len(t.attempts) for t in arm_trials),
        "per_solved": total / solved if solved else float("inf"),
        "output_per_solved": output / solved if solved else float("inf"),
    }


def report(trials: list[Trial], expressiveness: tuple[str, int, int]) -> str:
    lines: list[str] = []
    for model in sorted({t.model for t in trials}):
        mine = [t for t in trials if t.model == model]
        conditions = sorted({condition(t) for t in mine}, key=lambda c: (c != "js", c))
        present = {(condition(t), t.task) for t in mine}
        tasks = sorted(task for task in {t.task for t in mine} if all((c, task) in present for c in conditions))
        stats = {c: summarize([t for t in mine if condition(t) == c and t.task in tasks], tasks) for c in conditions}
        lines += [f"## {model}", "", f"Tasks with every condition: {len(tasks)}", ""]
        lines += [
            "| Condition | First attempt | Final | Attempts | First prompt | Input per trial | Output per trial "
            "| Of which reasoning | Tokens per solved task | Output per solved task |",
            "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
        ]
        for name, st in stats.items():
            per_solved = f"{st['per_solved']:,.0f}" if st["solved"] else "none solved"
            output_per_solved = f"{st['output_per_solved']:,.0f}" if st["solved"] else "none solved"
            lines.append(
                f"| {name} | {mean(st['first'].values()):.1%} | {mean(st['final'].values()):.1%} "
                f"| {st['attempts']:.2f} | {st['prompt']:,.0f} | {st['input']:,.0f} | {st['output']:,.0f} "
                f"| {st['reasoning']:,.0f} | {per_solved} | {output_per_solved} |"
            )
        lines.append("")
        if "js" in stats:
            for name in [c for c in conditions if c != "js"]:
                sx, js = stats[name], stats["js"]
                first_diff = [sx["first"][t] - js["first"][t] for t in tasks]
                final_diff = [sx["final"][t] - js["final"][t] for t in tasks]
                low, high = bootstrap(first_diff)
                final_low, final_high = bootstrap(final_diff)
                within = mean(first_diff) >= -0.05
                better = mean(final_diff) > 0 or sx["per_solved"] < js["per_solved"]
                verdict = "PASS" if within and better else "FAIL"
                lines += [
                    f"{name} minus js, first attempt: {mean(first_diff):+.1%} "
                    f"(95% interval {low:+.1%} to {high:+.1%}); "
                    f"final: {mean(final_diff):+.1%} ({final_low:+.1%} to {final_high:+.1%}); gate {verdict}",
                ]
            lines.append("")
        fixed: Counter[str] = Counter()
        seen: Counter[str] = Counter()
        for trial in mine:
            for before, after in zip(trial.attempts, trial.attempts[1:], strict=False):
                for code in set(before.diagnostics):
                    seen[f"{trial.arm} {code}"] += 1
                    fixed[f"{trial.arm} {code}"] += code not in after.diagnostics
        if seen:
            lines += ["Diagnostics and whether the next attempt fixed them:", ""]
            lines += [f"- {code}: fixed {fixed[code]} of {count}" for code, count in seen.most_common()]
            lines.append("")
        outcomes: Counter[str] = Counter(f"{condition(t)} {t.attempts[-1].status}" for t in mine)
        lines += ["Final status counts: " + ", ".join(f"{k} {v}" for k, v in sorted(outcomes.items())), ""]
    split, missing, total = expressiveness
    lines.append(f"Expressiveness gap: {missing} of {total} {split} tasks need a type Sx lacks.")
    return "\n".join(lines)


def measure_guides(model: str, thinking: str, out: Path) -> str:
    out.mkdir(parents=True, exist_ok=True)
    instruction = "Reply with only the word OK."

    def input_tokens(text: str, name: str) -> int:
        prompt = out / f"measure-{name}.md"
        prompt.write_text(text)
        _, used, error = ask(prompt, model, thinking, out)
        if error is not None:
            raise RuntimeError(f"measuring {name} failed: {error}")
        return used.get("input", 0)

    baseline = input_tokens(instruction, "baseline")
    lines = [f"Baseline, a one-line prompt: {baseline} input tokens", ""]
    for level in GUIDE_LEVELS:
        cost = input_tokens(f"{guide(level)}\n\n{instruction}", level) - baseline
        lines.append(f"- guide {level}: {cost} tokens")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description="Compare Sx and TypeScript on the probe tasks.")
    parser.add_argument("--split", choices=["development", "held-out", "fresh"], default="development")
    parser.add_argument("--model", default="opencode-go/muse-spark-1.3-contributor")
    parser.add_argument("--thinking", default="medium")
    parser.add_argument("--samples", type=int, default=2)
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--arm", action="append", choices=["sx", "js"])
    parser.add_argument("--task", action="append")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument(
        "--guide", action="append", choices=[*GUIDE_LEVELS, TAILORED], help="Sx guide levels to run",
    )
    parser.add_argument("--quick", action="store_true", help="One sample of six development tasks, for iterating")
    parser.add_argument("--report", action="store_true", help="Report on saved trials without calling a model")
    parser.add_argument("--measure-guides", action="store_true", help="Measure each guide level's token cost")
    parser.add_argument("--rejudge", action="store_true", help="Score saved attempts again with the current rules")
    args = parser.parse_args()
    args.out = args.out.resolve()
    all_tasks = [json.loads(p.read_text()) for p in (BENCH / "tasks").glob("*/task.json")]
    in_split = [t for t in all_tasks if t["split"] == args.split]
    gap = (args.split, sum(1 for t in in_split if not t["sx-expressible"]), len(in_split))
    if args.report:
        print(report(load_trials(args.out), gap))
        return
    if args.rejudge:
        print(f"{rejudge(args.out, SIGNATURES)} attempts changed")
        print(report(load_trials(args.out), gap))
        return
    if args.measure_guides:
        print(measure_guides(args.model, args.thinking, args.out))
        return
    cores = SIGNATURES
    if args.quick:
        args.split, args.samples, args.task = "development", 1, args.task or list(QUICK_TASKS)
    tasks = load_tasks(args.split, args.task)
    arms = args.arm or ["sx", "js"]
    levels = args.guide or ["standard"]
    args.out.mkdir(parents=True, exist_ok=True)
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        jobs = []
        for task in tasks:
            wanted = reference_delays(task)
            for arm in arms:
                for level in levels if arm == "sx" else ["none"]:
                    for sample in range(args.samples):
                        settings = (args.model, args.thinking, sample, args.out, cores, wanted)
                        jobs.append(pool.submit(run_trial, task, arm, level, *settings))
        for job in jobs:
            trial = job.result()
            statuses = [a.status for a in trial.attempts]
            print(f"{trial.task:28} {condition(trial):12} {trial.sample} {statuses}", flush=True)
    print(report(load_trials(args.out), gap))


if __name__ == "__main__":
    main()
