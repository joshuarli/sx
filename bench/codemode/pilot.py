import argparse
import json
import os
import re
import subprocess
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

from sx.diagnostics import render
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
OPENCODE = Path.home() / ".opencode" / "bin" / "opencode"
AGENT = "sx-probe"
AGENT_CONFIG = {
    "$schema": "https://opencode.ai/config.json",
    "agent": {
        AGENT: {
            "mode": "primary",
            "description": "Answers with one program and uses no tools.",
            "prompt": "You write one program that solves the task you are given. Reply with the program only.",
            "tools": {"*": False},
        }
    },
}
type Json = dict[str, Any]

FENCE = re.compile(r"```([A-Za-z0-9_-]*)[^\n]*\n(.*?)```", re.DOTALL)


@dataclass(frozen=True, slots=True)
class Run:
    task: str
    sample: int
    model: str
    reply: str
    fence: str | None
    program: str
    tokens: dict[str, int]
    error: str | None
    diagnostics: list[str]
    rendered: str
    unknown: list[str]


def task_section(task: Json, cores: Json) -> str:
    lines = ["## Your task", "", task["description"], ""]
    if task.get("types"):
        lines += ["Types:", *(f"  {name} = {definition}" for name, definition in task["types"].items()), ""]
    if task.get("enums"):
        lines.append("Enums (match on their variants):")
        for name, variants in task["enums"].items():
            shown = " | ".join(
                variant if payload is None else f"({variant} {payload})" for variant, payload in variants.items()
            )
            lines.append(f"  {name} = {shown}")
        lines.append("")
    lines.append("Tools:")
    for capability in task["capabilities"]:
        effect = capability["effect"]
        if effect == "write":
            effect += ", idempotent" if capability["idempotent"] else ", not idempotent"
        lines.append(f"  (call {capability['id']} {capability['input']}) -> {capability['output']}   [{effect}]")
        lines.append(f"      {capability['description']}")
    lines.append("")
    functions = [f for f in cores["functions"] if f["name"].split(".")[0] in task.get("cores", [])]
    if functions:
        lines.append("Functions:")
        lines += [
            f"  {name} = {definition}"
            for name, definition in cores["types"].items()
            if name.split(".")[0] in task["cores"]
        ]
        for function in functions:
            params = " ".join(function["params"])
            lines.append(f"  ({function['name']} {params}) -> {function['output']}   {function['description']}")
        lines.append("")
    lines.append("Reply with one Sx program in a ```sx code block, and nothing else.")
    return "\n".join(lines)


def isolated_environment(workdir: Path) -> dict[str, str]:
    config = workdir / "config"
    config.mkdir(exist_ok=True)
    return {
        **os.environ,
        "PWD": str(workdir),
        "OPENCODE_CONFIG_DIR": str(config),
        "OPENCODE_DISABLE_CLAUDE_CODE": "1",
        "OPENCODE_DISABLE_CLAUDE_CODE_SKILLS": "1",
    }


def ask(prompt: Path, model: str, workdir: Path) -> tuple[str, dict[str, int], str | None]:
    command = [str(OPENCODE), "run", "--standalone", "--agent", AGENT, "-m", model, "--format", "json"]
    completed = subprocess.run(
        [*command, "--file", str(prompt), "Solve the task in the attached file."],
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        cwd=workdir,
        env=isolated_environment(workdir),
        timeout=600,
        check=False,
    )
    text: list[str] = []
    tokens: Counter[str] = Counter()
    error = None
    for line in completed.stdout.splitlines():
        if not line.startswith("{"):
            continue
        event = json.loads(line)
        if event["type"] == "text":
            text.append(str(event["part"]["text"]))
        elif event["type"] == "step_finish":
            usage = event["part"]["tokens"]
            tokens.update(
                input=int(usage["input"]) + int(usage["cache"]["read"]),
                output=int(usage["output"]),
                reasoning=int(usage["reasoning"]),
            )
        elif event["type"] == "tool_use":
            error = f"the model tried to use the {event['part']['tool']} tool"
        elif event["type"] == "error":
            error = str(event["error"].get("message", json.dumps(event["error"])))
    if completed.returncode != 0 and error is None:
        error = f"opencode exited {completed.returncode}: {completed.stderr.strip()[-500:]}"
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
    task: Json, sample: int, model: str, reply: str, tokens: dict[str, int], error: str | None, cores: Json
) -> Run:
    program, fence = extract(reply)
    result = read(program)
    return Run(
        task=task["id"],
        sample=sample,
        model=model,
        reply=reply,
        fence=fence,
        program=program,
        tokens=tokens,
        error=error,
        diagnostics=[d.label for d in result.diagnostics],
        rendered=render(program, result.diagnostics),
        unknown=[] if result.program is None else unknown_names(result.program, task, cores),
    )


def run_one(task: Json, sample: int, model: str, prompt: Path, workdir: Path, cores: Json) -> Run:
    reply, tokens, error = ask(prompt, model, workdir)
    return score(task, sample, model, reply, tokens, error, cores)


def main() -> None:
    parser = argparse.ArgumentParser(description="Run the syntax pilot on the development tasks.")
    parser.add_argument("--model", default="opencode-go/iceman-contributor-free")
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
    (workdir / "opencode.json").write_text(json.dumps(AGENT_CONFIG, indent=2))
    guide = (BENCH / "guide.md").read_text()
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
                fields = (saved["sample"], saved["model"], saved["reply"], saved["tokens"], saved["error"])
                runs.append(score(task, *fields, cores))
        report(runs, tasks, args.out)
        return

    jobs = []
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for task in tasks:
            prompt = args.out / task["id"] / "prompt.md"
            prompt.parent.mkdir(parents=True, exist_ok=True)
            prompt.write_text(f"{guide}\n\n{task_section(task, cores)}\n")
            jobs.extend(
                pool.submit(run_one, task, sample, args.model, prompt, workdir, cores) for sample in range(args.samples)
            )
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
            print(f"### sample {run['sample']}: {status}{unknown}; tokens {run['tokens']}")
            print(run["program"])
            if run["diagnostics"]:
                print(run["rendered"])


def report(runs: list[Run], tasks: list[Json], out: Path) -> None:
    labels: Counter[str] = Counter()
    unknown: Counter[str] = Counter()
    for run in runs:
        unknown.update(set(run.unknown))
        (out / run.task / f"{run.sample}.json").write_text(json.dumps(asdict(run), indent=2))
        labels.update(set(run.diagnostics))
    for task in tasks:
        mine = [r for r in runs if r.task == task["id"]]
        answered = [r for r in mine if r.program]
        read_ok = sum(1 for r in answered if not r.diagnostics)
        noisy = sum(1 for r in mine if r.error is not None)
        print(f"{task['id']:24} read {read_ok}/{len(answered)}" + (f"  harness noise {noisy}" if noisy else ""))
    answered = [r for r in runs if r.program]
    total_ok = sum(1 for r in answered if not r.diagnostics)
    noisy = sum(1 for r in runs if r.error is not None)
    print(f"total read {total_ok}/{len(answered)} answers from {len(runs)} runs; harness noise in {noisy} runs")
    for label, count in labels.most_common():
        print(f"  {label}: {count} answers")
    print("names Sx does not have, in answers that read cleanly:")
    for name, count in unknown.most_common():
        print(f"  {name}: {count} answers")


if __name__ == "__main__":
    main()
