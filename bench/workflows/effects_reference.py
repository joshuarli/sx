import json
import subprocess
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path

import effects
import paths
from arms import Arm, Files, require_toolchain, scratch
from definition import EffectsWorkflow, Verdict, Violation
from effects import Stamped
from mcpclient import McpClient
from records import Attempt, Effect, Json
from session import normalize, recorded, wiring

SCRIPT_TIMEOUT_SECONDS = 300
REFERENCE_CALL = "reference"


@dataclass(frozen=True, slots=True)
class ReferenceRun:
    attempts: list[Attempt]
    effects: list[Effect]
    verdict: Verdict
    violations: list[Violation]
    failure: str | None


def execution_events(
    tool: str, args: Json, started: int, ended: int, result: Json, nested: list[Json]
) -> list[Stamped]:
    events = [Stamped(started, {
        "type": "tool_execution_start", "toolCallId": REFERENCE_CALL, "toolName": tool, "args": args,
    })]
    for number, call in enumerate(nested, start=1):
        events.append(Stamped(started, {
            "type": "tool_execution_start", "toolCallId": f"{REFERENCE_CALL}/{number}", "toolName": call["tool"],
            "args": call["args"], "parentToolCallId": REFERENCE_CALL,
        }))
    events.append(Stamped(ended, {
        "type": "tool_execution_end", "toolCallId": REFERENCE_CALL, "toolName": tool, "result": result,
        "isError": False,
    }))
    return events


def run_sx(wired_server: list[str], proxy: Path, program: str) -> tuple[list[Stamped], str | None]:
    started = time.time_ns()
    with McpClient(recorded(proxy, wired_server)) as client:
        result = client.call_tool("run", {"program": program})
    ended = time.time_ns()
    events = execution_events("mcp__sx_code__run", {"program": program}, started, ended, result, [])
    outcome = result["structuredContent"]["outcome"]
    return events, None if outcome == "completed" else f"the program {outcome}: {result['content'][0]['text']}"


def run_js(wired_server: list[str], proxy: Path, directory: Path, script: str) -> tuple[list[Stamped], str | None]:
    source = directory / "reference.js"
    source.write_text(script)
    command = [
        require_toolchain(), "run", str(paths.HARNESS / "codemode.ts"), "--script", str(source), "--",
        *recorded(proxy, wired_server),
    ]
    started = time.time_ns()
    completed = subprocess.run(
        command, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=SCRIPT_TIMEOUT_SECONDS, check=False,
    )
    ended = time.time_ns()
    if not completed.stdout.startswith("{"):
        raise RuntimeError(f"the codemode shim exited {completed.returncode}: {completed.stderr.strip()[-2000:]}")
    report: Json = json.loads(completed.stdout)
    heading = "Script completed" if report["outcome"] == "completed" else "Script failed"
    events = execution_events(
        effects.CODEMODE, {"code": script}, started, ended, {"content": [{"type": "text", "text": heading}]},
        report["calls"],
    )
    failure = report.get("error")
    return events, None if failure is None else str(failure)


def run_reference(workflow: EffectsWorkflow, arm: Arm) -> ReferenceRun:
    return run_program(workflow, arm, workflow.reference(arm))


def run_program(workflow: EffectsWorkflow, arm: Arm, template: str) -> ReferenceRun:
    with workflow.sandbox() as sandbox, tempfile.TemporaryDirectory(dir=scratch()) as scratch_directory:
        directory = Path(scratch_directory)
        files = Files(workflow, sandbox, directory)
        wired = wiring(arm, workflow, sandbox, files, directory)
        proxy = directory / "proxy.jsonl"
        proxy.touch()
        program = workflow.render(template, sandbox)
        if arm == "sx":
            events, failure = run_sx(wired.server, proxy, program)
        else:
            events, failure = run_js(wired.server, proxy, directory, program)
        classes = effects.effect_classes(workflow.declarations_json(sandbox))
        attempts, found = normalize(arm, events, proxy, classes)
        audit = workflow.audit(sandbox, found)
    return ReferenceRun(attempts, found, audit.verdict, audit.violations, failure)
