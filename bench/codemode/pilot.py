import argparse
import json
import re
import subprocess
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

from cores import task_registry

from sx.checker import check
from sx.diagnostics import render
from sx.help import guide, tool_listing
from sx.host import FixtureHost, UnexpectedCallError, run
from sx.machine import Completed, Failed
from sx.reader import read
from sx.syntax import Form, ListLiteral, Node, RecordLiteral, Symbol

FORMS = frozenset({
    "let", "if", "match", "fn", "get", "call", "attempt", "retry", "wait-until", "loop", "recur", "par", "par-map",
})
PRELUDE = frozenset({
    "+", "-", "*", "quot", "rem", "=", "not=", "<", "<=", ">", ">=", "not", "and", "or", "str", "count", "take",
    "concat", "flatten", "contains?", "map", "filter", "any?", "all?", "reduce", "sort-by", "distinct-by", "group-by",
    "some", "ok", "err", "first", "rest", "nth", "keep",
})

BENCH = Path(__file__).parent
ROOT = BENCH.parent.parent
PI = Path.home() / ".local" / "bin" / "pi"
SYSTEM_PROMPT = "You write one program that solves the task you are given. Reply with the program only."
PI_ISOLATION = (
    "--no-session", "--no-tools", "--no-extensions", "--no-mcp", "--no-skills", "--no-context-files",
    "--no-prompt-templates", "--offline",
)
type Json = dict[str, Any]

FENCE = re.compile(r"```([A-Za-z0-9_-]*)[^\n]*\n(.*?)```", re.DOTALL)


@dataclass(frozen=True, slots=True)
class Run:
    task: str
    sample: int
    model: str
    thinking: str | None
    reply: str
    fence: str | None
    program: str
    tokens: dict[str, int]
    error: str | None
    diagnostics: list[str]
    rendered: str
    unknown: list[str]
    outcome: str


def task_section(task: Json) -> str:
    registry = task_registry(task)
    listing = tool_listing(registry, [c["id"] for c in task["capabilities"]], task.get("cores", []))
    reply = "Reply with one Sx program in a ```sx code block, and nothing else."
    return "\n".join(["## Your task", "", task["description"], "", listing, reply])

def ask(prompt: Path, model: str, thinking: str, workdir: Path) -> tuple[str, dict[str, int], str | None]:
    command = [str(PI), "-p", "--mode", "json", *PI_ISOLATION, "--model", model, "--thinking", thinking]
    completed = subprocess.run(
        [*command, "--system-prompt", SYSTEM_PROMPT, prompt.read_text()],
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        cwd=workdir,
        timeout=900,
        check=False,
    )
    text: list[str] = []
    tokens: Counter[str] = Counter()
    error = None
    for line in completed.stdout.splitlines():
        if not line.startswith("{"):
            continue
        event = json.loads(line)
        if event.get("type") != "message_end" or event["message"].get("role") != "assistant":
            continue
        message = event["message"]
        for part in message.get("content", []):
            if part.get("type") == "text":
                text.append(str(part["text"]))
            elif part.get("type") != "thinking":
                error = f"the reply contained a {part.get('type')} part"
        usage = message["usage"]
        tokens.update(
            input=int(usage["input"]) + int(usage["cacheRead"]),
            output=int(usage["output"]),
            reasoning=int(usage["reasoning"]),
        )
        if message.get("stopReason") != "stop":
            error = f"the model stopped with {message.get('stopReason')}: {message.get('errorMessage', '')}"
    if completed.returncode != 0 and error is None:
        error = f"pi exited {completed.returncode}: {completed.stderr.strip()[-500:]}"
    return "".join(text), dict(tokens), error


def extract(reply: str) -> tuple[str, str | None]:
    if FENCE.search(reply) is None:
        return reply.strip(), None
    blocks = [(str(m.group(1)), str(m.group(2)).strip()) for m in FENCE.finditer(reply)]
    for language, body in blocks:
        if language == "sx":
            return body, language
    language, body = blocks[0]
    return body, language


def local_functions(node: Node) -> set[str]:
    names: set[str] = set()
    stack: list[Node] = [node]
    while stack:
        current = stack.pop()
        if isinstance(current, Form):
            items = current.items
            if len(items) > 1 and isinstance(items[0], Symbol) and items[0].name == "let":
                bindings = items[1]
                if isinstance(bindings, ListLiteral):
                    pairs = zip(bindings.items[::2], bindings.items[1::2], strict=False)
                    names.update(
                        name.name
                        for name, value in pairs
                        if isinstance(name, Symbol)
                        and isinstance(value, Form)
                        and value.items
                        and isinstance(value.items[0], Symbol)
                        and value.items[0].name == "fn"
                    )
            stack.extend(items)
        elif isinstance(current, ListLiteral):
            stack.extend(current.items)
        elif isinstance(current, RecordLiteral):
            stack.extend(field.value for field in current.fields)
    return names


def unknown_names(node: Node, task: Json, cores: Json) -> list[str]:
    functions = {f["name"] for f in cores["functions"] if f["name"].split(".")[0] in task.get("cores", [])}
    capabilities = {c["id"] for c in task["capabilities"]}
    local = local_functions(node)
    found: list[str] = []
    stack: list[Node] = [node]
    while stack:
        current = stack.pop()
        if isinstance(current, Form):
            items = current.items
            head = items[0] if items else None
            if isinstance(head, Symbol):
                if head.name == "call" and len(items) > 1 and isinstance(items[1], Symbol):
                    if items[1].name not in capabilities:
                        found.append(f"call {items[1].name}")
                elif head.name == "str" and len(items) < 2:
                    found.append("str with no arguments")
                elif head.name == "loop" and len(items) > 1 and not isinstance(items[1], RecordLiteral):
                    found.append("loop without options")
                elif head.name not in FORMS | PRELUDE | functions | local and not head.name[0].isupper():
                    found.append(head.name)
            stack.extend(items)
        elif isinstance(current, ListLiteral):
            stack.extend(current.items)
        elif isinstance(current, RecordLiteral):
            stack.extend(field.value for field in current.fields)
    return found


def score(
    task: Json,
    sample: int,
    model: str,
    thinking: str | None,
    reply: str,
    tokens: dict[str, int],
    error: str | None,
    cores: Json,
) -> Run:
    program, fence = extract(reply)
    result = read(program)
    diagnostics = result.diagnostics
    outcome = "rejected"
    if result.program is not None:
        registry = task_registry(task)
        allowed = frozenset(capability["id"] for capability in task["capabilities"])
        checked = check(result.program, registry, allowed=allowed)
        diagnostics = checked.diagnostics
        if checked.program is not None:
            outcome = execute(task, checked.program, registry)
    return Run(
        task=task["id"],
        sample=sample,
        model=model,
        thinking=thinking,
        reply=reply,
        fence=fence,
        program=program,
        tokens=tokens,
        error=error,
        diagnostics=[d.label for d in diagnostics],
        rendered=render(program, diagnostics),
        unknown=[] if result.program is None else unknown_names(result.program, task, cores),
        outcome=outcome,
    )


def execute(task: Json, program: Any, registry: Any) -> str:
    host = FixtureHost.from_responses(registry, task["responses"])
    try:
        finished = run(program, host).outcome
    except UnexpectedCallError as unexpected:
        return f"wrong call: {unexpected}"
    if isinstance(finished, Failed):
        return f"failed: {finished.failure.kind}: {finished.failure.message}"
    assert isinstance(finished, Completed)
    if host.unperformed_writes():
        return "wrong: a required write did not happen"
    return "correct" if finished.json == task["expected"] else f"wrong result: {json.dumps(finished.json)}"


def run_one(task: Json, sample: int, model: str, thinking: str, prompt: Path, workdir: Path, cores: Json) -> Run:
    reply, tokens, error = ask(prompt, model, thinking, workdir)
    return score(task, sample, model, thinking, reply, tokens, error, cores)


def main() -> None:
    parser = argparse.ArgumentParser(description="Run the syntax pilot on the development tasks.")
    parser.add_argument("--model", default="opencode-go/muse-spark-1.3-contributor")
    parser.add_argument("--thinking", default="medium", help="Thinking level: off, minimal, low, medium, high")
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--out", type=Path, default=ROOT / ".work" / "pilot" / "runs")
    parser.add_argument("--task", action="append", help="Run only these task ids")
    parser.add_argument("--rescore", action="store_true", help="Score saved replies again without calling the model")
    parser.add_argument("--show", action="store_true", help="Print saved answers with their status, task by task")
    args = parser.parse_args()

    args.out = args.out.resolve()
    workdir = args.out.parent
    workdir.mkdir(parents=True, exist_ok=True)
    cores = json.loads((BENCH / "cores.json").read_text())
    tasks = []
    for path in sorted((BENCH / "tasks").glob("*/task.json")):
        task = json.loads(path.read_text())
        if task["split"] != "development":
            continue
        if args.task and task["id"] not in args.task:
            continue
        tasks.append(task)

    if args.show:
        show(args.out, {task["id"] for task in tasks})
        return

    if args.rescore:
        by_id = {task["id"]: task for task in tasks}
        runs = []
        for path in sorted(args.out.glob("*/[0-9]*.json")):
            saved = json.loads(path.read_text())
            if saved["task"] in by_id:
                task = by_id[saved["task"]]
                reply, tokens, error = saved["reply"], saved["tokens"], saved["error"]
                fields = (saved["sample"], saved["model"], saved.get("thinking"), reply, tokens, error)
                runs.append(score(task, *fields, cores))
        report(runs, tasks, args.out)
        return

    jobs = []
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for task in tasks:
            prompt = args.out / task["id"] / "prompt.md"
            prompt.parent.mkdir(parents=True, exist_ok=True)
            prompt.write_text(f"{guide()}\n\n{task_section(task)}\n")
            settings = (args.model, args.thinking, prompt, workdir, cores)
            jobs.extend(pool.submit(run_one, task, sample, *settings) for sample in range(args.samples))
        runs = [job.result() for job in jobs]
    report(runs, tasks, args.out)


def show(out: Path, task_ids: set[str]) -> None:
    for task_dir in sorted(path for path in out.iterdir() if path.is_dir() and path.name in task_ids):
        print(f"## {task_dir.name}")
        for path in sorted(task_dir.glob("[0-9]*.json")):
            run = json.loads(path.read_text())
            if run["error"] is not None:
                status = f"model error: {run['error']}"
            elif run["diagnostics"]:
                status = "read failed: " + ", ".join(run["diagnostics"])
            else:
                status = "read ok"
            unknown = f"; lacks {', '.join(run['unknown'])}" if run["unknown"] else ""
            unknown += f"; {run['outcome']}"
            print(f"### sample {run['sample']}: {status}{unknown}; tokens {run['tokens']}")
            print(run["program"])
            if run["diagnostics"]:
                print(run["rendered"])


def report(runs: list[Run], tasks: list[Json], out: Path) -> None:
    labels: Counter[str] = Counter()
    unknown: Counter[str] = Counter()
    for saved in runs:
        unknown.update(set(saved.unknown))
        (out / saved.task / f"{saved.sample}.json").write_text(json.dumps(asdict(saved), indent=2))
        labels.update(set(saved.diagnostics))
    for task in tasks:
        mine = [r for r in runs if r.task == task["id"]]
        answered = [r for r in mine if r.program]
        checks_ok = sum(1 for r in answered if not r.diagnostics)
        correct = sum(1 for r in answered if r.outcome == "correct")
        noisy = sum(1 for r in mine if r.error is not None)
        line = f"{task['id']:24} checks {checks_ok}/{len(answered)}  correct {correct}/{len(answered)}"
        print(line + (f"  run errors {noisy}" if noisy else ""))
    answered = [r for r in runs if r.program]
    total_ok = sum(1 for r in answered if not r.diagnostics)
    total_correct = sum(1 for r in answered if r.outcome == "correct")
    noisy = sum(1 for r in runs if r.error is not None)
    summary = f"total correct {total_correct}, checks {total_ok}, of {len(answered)} answers from {len(runs)} runs"
    print(f"{summary}; errors in {noisy} runs")
    for label, count in labels.most_common():
        print(f"  {label}: {count} answers")
    print("names Sx does not have, in answers that read cleanly:")
    for name, count in unknown.most_common():
        print(f"  {name}: {count} answers")


if __name__ == "__main__":
    main()
