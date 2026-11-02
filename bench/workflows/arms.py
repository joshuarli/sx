import json
import shutil
import subprocess
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Literal

import paths
from definition import ToolServer, Workflow
from probe import FEEDBACK_LINES, SX_EXITS, TSC, TSCONFIG, label, show_span, sx_binary
from sandbox import SCRATCH, Sandbox
from typescript import declarations as typescript_declarations

type Json = dict[str, Any]
type Arm = Literal["sx", "js"]
type Status = Literal["completed", "failed", "rejected"]

SX_TIMEOUT = 180
BUN_TIMEOUT = 240
CALL_TIMEOUT_MS = 60_000
NO_CORES: Json = {"types": {}, "functions": []}
VIRTUAL_WAIT = "Time is virtual, so waiting costs nothing."
REAL_WAIT = "The wait takes real time."
MCP_PREFIX = "mcp-"
STANDARD_CORES = ("text", "re", "json")


class ToolchainMissingError(RuntimeError):
    pass


@dataclass(frozen=True, slots=True)
class Call:
    capability: str
    input: Any


@dataclass(frozen=True, slots=True)
class ArmRun:
    arm: Arm
    status: Status
    result: object
    detail: str
    feedback: str | None
    calls: list[Call]
    seconds: float
    program_seconds: float
    labels: list[str] = field(default_factory=list)
    explain: Json | None = None
    transcript: str = ""


def require_toolchain() -> str:
    bun = shutil.which("bun")
    if bun is None:
        raise ToolchainMissingError("bun is not installed; install Bun 1.4 or later and put it on PATH")
    if not TSC.is_file():
        raise ToolchainMissingError(f"tsc is missing at {TSC}; run `npm ci --prefix bench/codemode`")
    return bun


def sx_output(arguments: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(sx_binary()), *arguments],
        stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=SX_TIMEOUT, check=False,
    )


def imported_tools(server: ToolServer) -> Json:
    completed = sx_output(["mcp", "import", "--name", server.name, "--", *server.command])
    if completed.returncode != 0:
        raise RuntimeError(f"sx mcp import exited {completed.returncode}: {completed.stderr.strip()}")
    result: Json = json.loads(completed.stdout)
    return result


class Files:
    def __init__(self, workflow: Workflow, sandbox: Sandbox, directory: Path) -> None:
        self.directory = directory
        self.server = workflow.tool_server(sandbox)
        declarations = workflow.declarations_json(sandbox)
        declared_cores = declarations.get("cores", [])
        declarations["cores"] = [*declared_cores, *(core for core in STANDARD_CORES if core not in declared_cores)]
        self.imported: Json = imported_tools(self.server) if self.server is not None else {}
        declarations["enums"] = {**declarations.get("enums", {}), **self.imported.get("enums", {})}
        declarations["capabilities"] = [*declarations.get("capabilities", []), *self.imported.get("capabilities", [])]
        grants: Json = json.loads(workflow.render(workflow.grants, sandbox))
        self.declarations = directory / "declarations.json"
        self.grants = directory / "grants.json"
        self.native_grants = directory / "native-grants.json"
        self.declarations.write_text(json.dumps(declarations))
        self.grants.write_text(json.dumps(grants))
        native = {name: scope for name, scope in grants.items() if not name.startswith(MCP_PREFIX)}
        self.native_grants.write_text(json.dumps(native))
        self.serves_natives = bool(native)

    def program_arguments(self, source: Path) -> list[str]:
        return [str(source), "--declarations", str(self.declarations), "--grants", str(self.grants)]

    def server_arguments(self) -> list[str]:
        return ["--", *self.server.command] if self.server is not None else []


def described(files: Files) -> Json:
    imported: Json = files.imported
    served: Json = {"enums": {}, "types": {}, "capabilities": []}
    if files.serves_natives:
        completed = sx_output([
            "mcp", "describe", "--declarations", str(files.declarations), "--grants", str(files.native_grants),
        ])
        if completed.returncode != 0:
            raise RuntimeError(f"sx mcp describe exited {completed.returncode}: {completed.stderr.strip()}")
        served = json.loads(completed.stdout)
    return {
        **served,
        "enums": {**served["enums"], **imported.get("enums", {})},
        "capabilities": [
            *served["capabilities"],
            *imported.get("capabilities", []),
        ],
    }


def scratch() -> Path:
    SCRATCH.mkdir(parents=True, exist_ok=True)
    return SCRATCH


def described_for(workflow: Workflow, sandbox: Sandbox) -> Json:
    with tempfile.TemporaryDirectory(dir=scratch()) as directory:
        return described(Files(workflow, sandbox, Path(directory)))


def sx_listing(workflow: Workflow, sandbox: Sandbox) -> str:
    with tempfile.TemporaryDirectory(dir=scratch()) as directory:
        files = Files(workflow, sandbox, Path(directory))
        ids = [c["id"] for c in described(files)["capabilities"]]
        shown = [sx_output(["help", name, "--declarations", str(files.declarations)]).stdout.rstrip() for name in ids]
        cores = workflow.declarations_json(sandbox).get("cores", [])
        for core in cores:
            listing = sx_output(["help", core, "--declarations", str(files.declarations)]).stdout
            functions = listing.split("Functions:\n", 1)[1].rstrip() if "Functions:\n" in listing else ""
            shown.append(f"Functions of the {core} core:\n{functions}")
    return "\n\n".join(shown)


def sx_guide(workflow: Workflow, sandbox: Sandbox, *, notes: bool) -> str:
    with tempfile.TemporaryDirectory(dir=scratch()) as directory:
        files = Files(workflow, sandbox, Path(directory))
        arguments = ["guide", "--declarations", str(files.declarations), "--grants", str(files.grants)]
        completed = sx_output([*arguments, "--notes"] if notes else arguments)
    if completed.returncode != 0:
        raise RuntimeError(f"sx guide exited {completed.returncode}: {completed.stderr.strip()}")
    return completed.stdout


def run_sx(workflow: Workflow, sandbox: Sandbox, program: str) -> ArmRun:
    with tempfile.TemporaryDirectory(dir=scratch()) as directory:
        files = Files(workflow, sandbox, Path(directory))
        source = files.directory / "program.sx"
        source.write_text(program)
        arguments = files.program_arguments(source)
        explained = sx_output(["explain", *arguments, "--format", "json"])
        explain: Json | None = json.loads(explained.stdout) if explained.returncode == SX_EXITS["ok"] else None
        verb = ["run"] if files.server is None else ["mcp", "run"]
        started = time.perf_counter()
        completed = sx_output([*verb, *arguments, "--format", "json", *files.server_arguments()])
        seconds = time.perf_counter() - started
        if completed.returncode == SX_EXITS["rejected"]:
            diagnostics = json.loads(completed.stdout)["diagnostics"]
            rendered = sx_output(["check", *arguments]).stdout
            labels = [label(d) for d in diagnostics]
            return ArmRun(
                "sx", "rejected", None, rendered, rendered, [], seconds, seconds, labels, explain, completed.stdout
            )
    if completed.returncode not in (SX_EXITS["ok"], SX_EXITS["failed"]):
        raise RuntimeError(f"sx run exited {completed.returncode}: {completed.stderr.strip()}")
    report: Json = json.loads(completed.stdout)
    calls = [Call(e["capability"], e["input"]) for e in report["trace"] if "request" in e]
    if report["outcome"] == "failed":
        failure = report["failure"]
        where = f" at {show_span(failure['span'])}" if failure["span"] is not None else ""
        message = f"The program failed{where} with {failure['kind']}: {failure['message']}"
        return ArmRun(
            "sx", "failed", None, message, message, calls, seconds, seconds,
            explain=explain, transcript=completed.stdout,
        )
    return ArmRun(
        "sx", "completed", report["result"], "", None, calls, seconds, seconds, explain=explain,
        transcript=completed.stdout,
    )


def typecheck(directory: Path) -> tuple[str, list[str]] | None:
    (directory / "tsconfig.json").write_text(json.dumps(TSCONFIG))
    typed = subprocess.run(
        [str(TSC), "-p", str(directory)],
        stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=120, check=False,
    )
    if typed.returncode == 0:
        return None
    errors = "\n".join((typed.stdout + typed.stderr).replace(str(directory) + "/", "").splitlines()[:FEEDBACK_LINES])
    codes = sorted({line.split("error ")[1].split(":")[0] for line in errors.splitlines() if " error TS" in line})
    return errors, codes


def run_js(workflow: Workflow, sandbox: Sandbox, program: str) -> ArmRun:
    bun = require_toolchain()
    with tempfile.TemporaryDirectory(dir=scratch()) as directory:
        files = Files(workflow, sandbox, Path(directory))
        api = typescript_declarations({**described(files), "cores": []}, NO_CORES).replace(VIRTUAL_WAIT, REAL_WAIT)
        (files.directory / "api.d.ts").write_text(api)
        solution = files.directory / "solution.mts"
        solution.write_text(program)
        failed_typecheck = typecheck(files.directory)
        if failed_typecheck is not None:
            errors, codes = failed_typecheck
            return ArmRun("js", "rejected", None, errors, errors, [], 0.0, 0.0, codes)
        serve = [
            str(sx_binary()), "mcp", "serve", "--declarations", str(files.declarations),
            "--grants", str(files.native_grants),
        ]
        imported = [] if files.server is None else [
            "--imported-command", json.dumps(files.server.command),
            "--imported-names", json.dumps({c["mcp"]["tool"]: c["id"] for c in files.imported["capabilities"]}),
        ]
        command = [
            bun, "run", str(paths.HARNESS / "run.ts"), "--solution", str(solution),
            "--timeout-ms", str(CALL_TIMEOUT_MS), *imported, *(["--", *serve] if files.serves_natives else []),
        ]
        started = time.perf_counter()
        completed = subprocess.run(
            command, cwd=files.directory, stdin=subprocess.DEVNULL, capture_output=True, text=True,
            timeout=BUN_TIMEOUT, check=False,
        )
        seconds = time.perf_counter() - started
    if not completed.stdout.startswith("{"):
        raise RuntimeError(f"the Bun harness exited {completed.returncode}: {completed.stderr.strip()[-2000:]}")
    report: Json = json.loads(completed.stdout)
    calls = [Call(c["capability"], c["input"]) for c in report["calls"]]
    program_seconds = float(report["elapsedMs"]) / 1000
    if report["outcome"] == "failed":
        message = str(report["error"])
        feedback = (
            f"A tool call failed because the tool rejected its input: {message}"
            if report["kind"] == "host"
            else f"The module threw: {message}"
        )
        return ArmRun(
            "js", "failed", None, message, feedback, calls, seconds, program_seconds, transcript=completed.stdout
        )
    return ArmRun(
        "js", "completed", report["result"], "", None, calls, seconds, program_seconds, transcript=completed.stdout
    )


def run_arm(arm: Arm, workflow: Workflow, sandbox: Sandbox, program: str) -> ArmRun:
    return run_sx(workflow, sandbox, program) if arm == "sx" else run_js(workflow, sandbox, program)
