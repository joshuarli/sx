import json
import os
import signal
import subprocess
import sys
import threading
import time
from dataclasses import asdict, dataclass
from pathlib import Path

import arms
import effects
from arms import Files
from definition import Audit, AuditError, EffectsWorkflow, Verdict, Violation
from metrics import Measure, measure
from probe import sx_binary
from records import (
    ATTEMPT_STATUSES,
    CODE_SERVER_ARMS,
    EFFECT_CLASSES,
    OUTCOMES,
    TOOLS,
    Attempt,
    Effect,
    EffectClass,
    Json,
    SessionArm,
    Usage,
    narrowed,
)
from sandbox import Sandbox

PI = Path.home() / ".local" / "bin" / "pi"
RECORDER = Path(__file__).resolve().parent / "recorder.py"
SESSION_TIMEOUT_SECONDS = 900
SX_SERVER = "sx-code"
SX_TOOLS_PATTERN = "mcp__sx_code__*"
JS_TOOLS_PATTERN = f"codemode,mcp__{effects.JS_SERVER}__*"
PROVIDER_RETRY_SETTINGS = {"retry": {"maxRetries": 8, "baseDelayMs": 5000, "maxAgentDelayMs": 120000}}
CODEMODE_INLINE_BUDGET_TOKENS = 200_000
CODEMODE_SETTINGS = {
    "codemode": {"mode": "only", "inlineBudget": CODEMODE_INLINE_BUDGET_TOKENS}, **PROVIDER_RETRY_SETTINGS,
}
TYPESCRIPT_LANGUAGE = ("--language", "ts")
MCP_TIMEOUT_SECONDS = 300
PI_ISOLATION = (
    "-p", "--mode", "json", "--approve", "--no-session", "--no-extensions", "--no-skills", "--no-context-files",
    "--no-prompt-templates", "--offline",
)


@dataclass(frozen=True, slots=True)
class SessionRecord:
    workflow: str
    arm: SessionArm
    sample: int
    model: str
    thinking: str
    error: str | None
    seconds: float
    usage: Usage
    attempts: list[Attempt]
    effects: list[Effect]
    verdict: Verdict
    violations: list[Violation]

    def needs_rerun(self) -> bool:
        stopped = self.error is not None and self.error.startswith(effects.MODEL_STOPPED)
        return stopped and self.verdict.status != "correct"

    def measure(self) -> Measure:
        return measure(
            self.attempts, self.effects, [v.effect for v in self.violations],
            self.verdict.status == "correct", self.usage,
        )

    def to_json(self) -> Json:
        return asdict(self)


def record_from_json(loaded: Json) -> SessionRecord:
    return SessionRecord(
        workflow=loaded["workflow"],
        arm=loaded["arm"],
        sample=loaded["sample"],
        model=loaded["model"],
        thinking=loaded["thinking"],
        error=loaded["error"],
        seconds=loaded["seconds"],
        usage=Usage(**loaded["usage"]),
        attempts=[
            Attempt(
                a["index"], narrowed(a["tool"], TOOLS), a["program"], narrowed(a["status"], ATTEMPT_STATUSES),
                tuple(a["diagnostics"]), tuple(a["refusals"]), a["started_ns"], a["ended_ns"],
            )
            for a in loaded["attempts"]
        ],
        effects=[
            Effect(
                e["attempt"], e["capability"], e["input"], narrowed(e["outcome"], OUTCOMES),
                narrowed(e["effect_class"], EFFECT_CLASSES), e["time_ns"],
            )
            for e in loaded["effects"]
        ],
        verdict=Verdict(**loaded["verdict"]),
        violations=[Violation(**v) for v in loaded["violations"]],
    )


def load_record(directory: Path) -> SessionRecord:
    return record_from_json(json.loads((directory / "session.json").read_text()))


def recorded(log: Path, command: list[str]) -> list[str]:
    return [sys.executable, str(RECORDER), "--log", str(log), "--", *command]


@dataclass(frozen=True, slots=True)
class Wiring:
    server_name: str
    server: list[str]
    tools: str
    extensions: tuple[str, ...]
    settings: Json
    append_system_prompt: str | None


def wiring(arm: SessionArm, workflow: EffectsWorkflow, sandbox: Sandbox, files: Files, directory: Path) -> Wiring:
    if files.server is not None:
        raise NotImplementedError(f"{workflow.id}: an effects workflow with a server.json is not supported yet")
    sx = str(sx_binary())
    if arm in CODE_SERVER_ARMS:
        policy = directory / "policy.json"
        policy.write_text(json.dumps(workflow.policy_json(sandbox)))
        server = [
            sx, "mcp", "code", "--declarations", str(files.declarations), "--grants", str(files.grants),
            "--policy", str(policy), *(TYPESCRIPT_LANGUAGE if arm == "ts" else ()),
        ]
        return Wiring(SX_SERVER, server, SX_TOOLS_PATTERN, ("builtin:mcp",), PROVIDER_RETRY_SETTINGS, None)
    notes = directory / "notes.md"
    notes.write_text(arms.sx_guide(workflow, sandbox, notes=True))
    server = [
        sx, "mcp", "serve", "--declarations", str(files.declarations), "--grants", str(files.native_grants),
    ]
    return Wiring(
        effects.JS_SERVER, server, JS_TOOLS_PATTERN, ("builtin:mcp", "builtin:codemode"), CODEMODE_SETTINGS,
        str(notes),
    )


def write_project(wired: Wiring, project: Path, proxy_log: Path) -> None:
    configuration = project / ".pi"
    configuration.mkdir(parents=True)
    command = recorded(proxy_log, wired.server)
    entry = {"command": command[0], "args": command[1:], "exposure": "direct", "timeout": MCP_TIMEOUT_SECONDS}
    (configuration / "mcp.json").write_text(json.dumps({"mcpServers": {wired.server_name: entry}}, indent=2))
    (configuration / "settings.json").write_text(json.dumps(wired.settings, indent=2))


def pi_command(wired: Wiring, model: str, thinking: str, prompt: str) -> list[str]:
    extensions = [argument for name in wired.extensions for argument in ("-e", name)]
    notes = wired.append_system_prompt
    append = [] if notes is None else ["--append-system-prompt", notes]
    return [
        str(PI), *PI_ISOLATION, *extensions, *append, "--tools", wired.tools, "--model", model,
        "--thinking", thinking, prompt,
    ]


def run_pi(command: list[str], project: Path, events: Path, stderr: Path, environment: dict[str, str]) -> int:
    timed_out = threading.Event()
    with events.open("w", buffering=1) as sink, stderr.open("w") as errors:
        child = subprocess.Popen(
            command, cwd=project, env=environment, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=errors,
            start_new_session=True,
        )

        def stop() -> None:
            timed_out.set()
            os.killpg(child.pid, signal.SIGKILL)

        timer = threading.Timer(SESSION_TIMEOUT_SECONDS, stop)
        timer.start()
        assert child.stdout is not None
        for line in child.stdout:
            arrival = time.time_ns()
            sink.write(json.dumps({"time_ns": arrival, "event": json.loads(line)}) + "\n")
        code = child.wait()
        timer.cancel()
    if timed_out.is_set():
        raise TimeoutError(f"pi ran longer than {SESSION_TIMEOUT_SECONDS} seconds")
    return code


def normalize(arm: SessionArm, events: list[effects.Stamped], proxy: Path, classes: dict[str, EffectClass]) -> tuple[
    list[Attempt], list[Effect]
]:
    calls = effects.read_proxy(proxy)
    runs = effects.executions(events)
    if arm in CODE_SERVER_ARMS:
        return effects.sx_attempts(runs, calls, classes)
    return effects.js_attempts(runs, calls, classes)


def run_session(
    workflow: EffectsWorkflow, arm: SessionArm, model: str, thinking: str, sample: int, out: Path,
    environment: dict[str, str] | None = None,
) -> SessionRecord:
    directory = out / workflow.id / f"{arm}-{sample}"
    directory.mkdir(parents=True)
    started = time.perf_counter()
    with workflow.sandbox() as sandbox:
        files = Files(workflow, sandbox, directory)
        wired = wiring(arm, workflow, sandbox, files, directory)
        project = directory / "project"
        proxy_log = directory / "proxy.jsonl"
        write_project(wired, project, proxy_log)
        prompt = workflow.render(workflow.task, sandbox).strip()
        (directory / "prompt.md").write_text(prompt)
        command = pi_command(wired, model, thinking, prompt)
        (directory / "command.json").write_text(json.dumps(command, indent=2))
        error: str | None = None
        try:
            code = run_pi(command, project, directory / "events.jsonl", directory / "stderr.txt", {
                **os.environ, **(environment or {}),
            })
            if code != 0:
                tail = (directory / "stderr.txt").read_text().strip()[-500:]
                error = f"pi exited {code}: {tail}"
        except TimeoutError as timeout:
            error = str(timeout)
        proxy_log.touch()
        events = effects.read_events(directory / "events.jsonl")
        error = error or effects.session_error(events)
        classes = effects.effect_classes(workflow.declarations_json(sandbox))
        attempts: list[Attempt] = []
        found: list[Effect] = []
        try:
            attempts, found = normalize(arm, events, proxy_log, classes)
            audit = workflow.audit(sandbox, found)
        except (effects.AttributionError, AuditError) as failure:
            error = error or f"not audited: {failure}"
            audit = Audit(Verdict.wrong(f"not audited: {failure}"), [])
    record = SessionRecord(
        workflow.id, arm, sample, model, thinking, error, time.perf_counter() - started,
        effects.usage_of(events), attempts, found, audit.verdict, audit.violations,
    )
    (directory / "session.json").write_text(json.dumps(record.to_json(), indent=2))
    return record


def run_measured_session(
    workflow: EffectsWorkflow, arm: SessionArm, model: str, thinking: str, sample: int, out: Path, reruns: int,
    environment: dict[str, str] | None = None,
) -> SessionRecord:
    stopped = 0
    while True:
        record = run_session(workflow, arm, model, thinking, sample, out, environment)
        if not record.needs_rerun() or stopped == reruns:
            return record
        kept = out / workflow.id / "stopped" / f"{arm}-{sample}-{stopped}"
        kept.parent.mkdir(parents=True, exist_ok=True)
        (out / workflow.id / f"{arm}-{sample}").rename(kept)
        stopped += 1
