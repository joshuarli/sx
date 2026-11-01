import argparse
import json
import re
import subprocess
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

from sx.diagnostics import render
from sx.reader import read

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


def ask(prompt: str, model: str, workdir: Path) -> tuple[str, dict[str, int], str | None]:
    completed = subprocess.run(
        [str(OPENCODE), "run", "--standalone", "--agent", AGENT, "-m", model, "--format", "json", prompt],
        capture_output=True,
        text=True,
        cwd=workdir,
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


def run_one(task: Json, sample: int, model: str, prompt: str, workdir: Path) -> Run:
    reply, tokens, error = ask(prompt, model, workdir)
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
    )


def main() -> None:
    parser = argparse.ArgumentParser(description="Run the syntax pilot on the development tasks.")
    parser.add_argument("--model", default="opencode-go/iceman-contributor-free")
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--out", type=Path, default=ROOT / ".work" / "pilot" / "runs")
    parser.add_argument("--task", action="append", help="Run only these task ids")
    args = parser.parse_args()

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

    jobs = []
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for task in tasks:
            prompt = f"{guide}\n\n{task_section(task, cores)}\n"
            (args.out / task["id"]).mkdir(parents=True, exist_ok=True)
            (args.out / task["id"] / "prompt.md").write_text(prompt)
            jobs.extend(
                pool.submit(run_one, task, sample, args.model, prompt, workdir) for sample in range(args.samples)
            )
        runs = [job.result() for job in jobs]

    labels: Counter[str] = Counter()
    for run in runs:
        (args.out / run.task / f"{run.sample}.json").write_text(json.dumps(asdict(run), indent=2))
        labels.update(set(run.diagnostics))
    for task in tasks:
        mine = [r for r in runs if r.task == task["id"]]
        read_ok = sum(1 for r in mine if r.error is None and not r.diagnostics)
        failed = sum(1 for r in mine if r.error is not None)
        print(f"{task['id']:24} read {read_ok}/{len(mine)}" + (f"  model errors {failed}" if failed else ""))
    total_ok = sum(1 for r in runs if r.error is None and not r.diagnostics)
    print(f"total read {total_ok}/{len(runs)}; model errors {sum(1 for r in runs if r.error is not None)}")
    for label, count in labels.most_common():
        print(f"  {label}: {count} answers")


if __name__ == "__main__":
    main()
