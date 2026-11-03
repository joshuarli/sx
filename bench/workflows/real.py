import argparse
import json
import os
import shutil
import subprocess
import uuid
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Literal

import effects
import fstree
import gate
import repairs
import session
from probe import sx_binary
from records import CODE_SERVER_ARMS, SESSION_ARMS, AttemptStatus, Json, SessionArm, Usage

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
REQUESTS = HERE / "real" / "requests.json"
REPOMOD_CHECKOUT = ROOT.parent / "repomod"
REPOMOD_SOURCE = REPOMOD_CHECKOUT / "src"
SCRATCH_ROOT = ROOT / ".work" / "real-scratch"
USER_AGENT = Path.home() / ".pi" / "agent"
SHARED_AGENT_FILES = ("auth.json", "models-store.json")
REPOMOD = "repomod"
FILES = "files"
CORES = ["text", "re", "json", "git", "github"]
FS_CAPABILITIES = ("fs.read-text", "fs.list", "fs.stat", "fs.write-text", "fs.make-directory")
READ_ONLY_TOOLS = ("read", "grep", "find", "ls")
UNTRUSTED = "--no-approve"
MAX_WRITE_SITES = 8
type JsListing = Literal["direct", "discovered"]
JS_LISTINGS: tuple[JsListing, ...] = ("direct", "discovered")
ARMS: tuple[SessionArm, ...] = ("sx", "js")
REPORT_ARMS: tuple[SessionArm, ...] = SESSION_ARMS


@dataclass(frozen=True, slots=True)
class Request:
    id: str
    text: str


@dataclass(frozen=True, slots=True)
class Program:
    tool: str
    status: AttemptStatus
    codes: tuple[str, ...]
    refusals: tuple[str, ...]
    source: str
    calls: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class RealRecord:
    request: str
    arm: SessionArm
    sample: int
    model: str
    thinking: str
    error: str | None
    usage: Usage
    programs: list[Program]
    other_tools: list[str]
    answer: str
    scratch: list[str] = field(default_factory=list)
    looked_at: list[str] = field(default_factory=list)
    js_listing: JsListing | None = None

    @property
    def key(self) -> str:
        return f"{self.request}/{self.arm}-{self.sample}"

    def read_sx_sources(self) -> bool:
        return any(path.startswith(str(ROOT)) and not path.startswith(str(SCRATCH_ROOT)) for path in self.looked_at)


def load_requests() -> list[Request]:
    return [Request(entry["id"], entry["text"]) for entry in json.loads(REQUESTS.read_text())]


def repomod_server() -> tuple[list[str], dict[str, str]]:
    command = ["uv", "run", "--directory", str(ROOT), "sx-repomod", "mcp"]
    return command, {"PYTHONPATH": str(REPOMOD_SOURCE), "PYTHONDONTWRITEBYTECODE": "1"}


def imported_declarations(out: Path) -> Path:
    target = out / "repomod-declarations.json"
    if target.is_file():
        return target
    command, environment = repomod_server()
    imported = subprocess.run(
        [str(sx_binary()), "mcp", "import", "--name", REPOMOD, "--", *command],
        capture_output=True, text=True, check=True, env={**os.environ, **environment},
    )
    target.write_text(imported.stdout)
    return target


def write_json(path: Path, value: object) -> Path:
    path.write_text(json.dumps(value, indent=2))
    return path


def server_entry(command: list[str], environment: dict[str, str], exposure: str) -> Json:
    return {
        "command": command[0], "args": command[1:], "env": environment, "exposure": exposure,
        "timeout": session.MCP_TIMEOUT_SECONDS,
    }


def configure(arm: SessionArm, directory: Path, scratch: Path, imported: Path, listing: JsListing) -> list[str]:
    repomod_tools: list[Json] = json.loads(imported.read_text())["capabilities"]
    repomod_ids = [tool["id"] for tool in repomod_tools]
    fs_grants = {capability: {"roots": [str(scratch.resolve())]} for capability in FS_CAPABILITIES}
    sx = str(sx_binary())
    repomod_command, repomod_environment = repomod_server()
    agent = directory / "agent"
    agent.mkdir()
    for name in SHARED_AGENT_FILES:
        (agent / name).symlink_to(USER_AGENT / name)
    if arm in CODE_SERVER_ARMS:
        declarations = write_json(
            directory / "declarations.json",
            {"packs": ["fs"], "cores": CORES, "capabilities": repomod_tools, **without_capabilities(imported)},
        )
        grants = write_json(directory / "grants.json", {**dict.fromkeys(repomod_ids), **fs_grants})
        policy = write_json(directory / "policy.json", {
            "capabilities": [*repomod_ids, *FS_CAPABILITIES],
            "computed_inputs": list(FS_CAPABILITIES),
            "max_write_sites": MAX_WRITE_SITES,
        })
        command = [
            sx, "mcp", "code", "--declarations", str(declarations), "--grants", str(grants), "--policy", str(policy),
            *(session.TYPESCRIPT_LANGUAGE if arm == "ts" else ()), "--", *repomod_command,
        ]
        servers = {session.SX_SERVER: server_entry(command, repomod_environment, "direct")}
        settings: Json = session.PROVIDER_RETRY_SETTINGS
        tools = ",".join([*READ_ONLY_TOOLS, session.SX_TOOLS_PATTERN])
        extensions = ["builtin:mcp"]
    else:
        declarations = write_json(directory / "declarations.json", {"packs": ["fs"], "cores": CORES})
        grants = write_json(directory / "grants.json", fs_grants)
        files = [sx, "mcp", "serve", "--declarations", str(declarations), "--grants", str(grants)]
        direct = listing == "direct"
        exposure = "direct" if direct else "codemode"
        servers = {
            REPOMOD: server_entry(repomod_command, repomod_environment, exposure),
            FILES: server_entry(files, {}, exposure),
        }
        settings = session.CODEMODE_SETTINGS if direct else session.PROVIDER_RETRY_SETTINGS
        served = [f"mcp__{REPOMOD}__*", f"mcp__{FILES}__*"] if direct else []
        tools = ",".join([*READ_ONLY_TOOLS, effects.CODEMODE, *served])
        extensions = ["builtin:mcp", "builtin:codemode"]
    (agent / "mcp.json").write_text(json.dumps({"mcpServers": servers}, indent=2))
    (agent / "settings.json").write_text(json.dumps(settings, indent=2))
    return [*[argument for name in extensions for argument in ("-e", name)], "--tools", tools]


def without_capabilities(imported: Path) -> Json:
    loaded: Json = json.loads(imported.read_text())
    return {key: value for key, value in loaded.items() if key not in ("capabilities", "cores", "packs")}


def server_structured(execution: effects.Execution) -> Json | None:
    if execution.result is None:
        return None
    outer = execution.result.get("structuredContent")
    if not isinstance(outer, dict):
        return None
    inner = outer.get("structuredContent")
    return inner if isinstance(inner, dict) else None


def sx_program(execution: effects.Execution) -> Program:
    tool = effects.SX_TOOLS[execution.tool]
    source = str(execution.args.get("program", ""))
    structured = server_structured(execution)
    if structured is None:
        return Program(tool, "failed", (), (), source, ())
    status, codes, refusals = effects.sx_status(tool, structured)
    calls = tuple(str(entry["capability"]) for entry in structured.get("trace", []) if "request" in entry)
    return Program(tool, status, codes, refusals, source, calls)


def js_program(script: effects.Execution, runs: list[effects.Execution]) -> Program:
    calls = tuple(run.tool for run in runs if run.parent == script.call_id)
    return Program("codemode", effects.script_status(script), (), (), str(script.args.get("code", "")), calls)


def final_answer(events: list[effects.Stamped]) -> str:
    replies = effects.replies(events)
    if not replies:
        return ""
    parts: list[Json] = replies[-1].get("content", [])
    return "\n".join(str(part["text"]) for part in parts if part.get("type") == "text")


def run_one(
    request: Request, arm: SessionArm, model: str, thinking: str, sample: int, out: Path, imported: Path,
    listing: JsListing,
) -> RealRecord:
    directory = out / request.id / f"{arm}-{sample}"
    directory.mkdir(parents=True)
    scratch = SCRATCH_ROOT / uuid.uuid4().hex
    scratch.mkdir(parents=True)
    scratch = scratch.resolve()
    flags = configure(arm, directory, scratch, imported, listing)
    prompt = request.text.replace("{{scratch}}", str(scratch))
    isolation = [UNTRUSTED if flag == "--approve" else flag for flag in session.PI_ISOLATION]
    command = [str(session.PI), *isolation, *flags, "--model", model, "--thinking", thinking, prompt]
    (directory / "command.json").write_text(json.dumps(command, indent=2))
    environment = {**os.environ, "PI_CODING_AGENT_DIR": str(directory / "agent")}
    error: str | None = None
    try:
        code = session.run_pi(
            command, REPOMOD_CHECKOUT, directory / "events.jsonl", directory / "stderr.txt", environment,
        )
        if code != 0:
            error = f"pi exited {code}: {(directory / 'stderr.txt').read_text().strip()[-500:]}"
    except TimeoutError as timeout:
        error = str(timeout)
    events = effects.read_events(directory / "events.jsonl")
    error = error or effects.session_error(events)
    runs = effects.executions(events)
    top = [run for run in runs if run.parent is None]
    if arm in CODE_SERVER_ARMS:
        programs = [sx_program(run) for run in top if run.tool in effects.SX_TOOLS]
        other = [run.tool for run in top if run.tool not in effects.SX_TOOLS]
    else:
        programs = [js_program(run, runs) for run in top if run.tool == effects.CODEMODE]
        other = [run.tool for run in top if run.tool != effects.CODEMODE]
    written = sorted(fstree.snapshot(scratch))
    shutil.move(scratch, directory / "scratch")
    record = RealRecord(
        request.id, arm, sample, model, thinking, error, effects.usage_of(events), programs, other,
        final_answer(events), written,
        [str(run.args.get("path", "")) for run in runs if run.tool in READ_ONLY_TOOLS],
        listing if arm == "js" else None,
    )
    (directory / "record.json").write_text(json.dumps(asdict(record), indent=2))
    return record


def load_record(path: Path) -> RealRecord:
    loaded: Json = json.loads(path.read_text())
    loaded["usage"] = Usage(**loaded["usage"])
    loaded.setdefault("js_listing", "discovered" if loaded["arm"] == "js" else None)
    loaded["programs"] = [
        Program(p["tool"], p["status"], tuple(p["codes"]), tuple(p["refusals"]), p["source"], tuple(p["calls"]))
        for p in loaded["programs"]
    ]
    return RealRecord(**loaded)


def summary(records: list[RealRecord]) -> list[str]:
    lines = [
        "| Arm | Sessions | Errored | Programs | By status | Codes and refusals | Other tools | Read Sx sources "
        "| Turns | Output | Cost |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for arm in REPORT_ARMS:
        mine = [r for r in records if r.arm == arm]
        if not mine:
            continue
        statuses = Counter(f"{p.tool}:{p.status}" for r in mine for p in r.programs)
        reasons = Counter(code for r in mine for p in r.programs for code in (*p.codes, *p.refusals))
        other = Counter(tool for r in mine for tool in r.other_tools)
        n = len(mine)
        listings = sorted({r.js_listing for r in mine if r.js_listing is not None})
        shown_arm = f"{arm} ({', '.join(listings)})" if listings else arm
        cells = [
            shown_arm, str(n), str(sum(r.error is not None for r in mine)), str(sum(len(r.programs) for r in mine)),
            ", ".join(f"{k} {v}" for k, v in sorted(statuses.items())) or "none",
            ", ".join(f"{k} {v}" for k, v in sorted(reasons.items())) or "none",
            ", ".join(f"{k} {v}" for k, v in sorted(other.items())) or "none",
            str(sum(r.read_sx_sources() for r in mine)),
            f"{sum(r.usage.turns for r in mine) / n:.1f}", f"{sum(r.usage.output for r in mine) / n:,.0f}",
            f"${sum(r.usage.cost for r in mine) / n:.4f}",
        ]
        lines.append("| " + " | ".join(cells) + " |")
    return lines


def judged(records: list[RealRecord], judgments: dict[str, bool]) -> list[gate.Scored]:
    return [gate.Scored(r.model, r.arm, judgments[r.key], r.usage.cost) for r in records if r.key in judgments]


def gate_lines(records: list[RealRecord], judgments: dict[str, bool]) -> list[str]:
    unjudged = [r.key for r in records if r.key not in judgments]
    lines = gate.lines(judged(records, judgments), REPORT_ARMS)
    if unjudged:
        lines += ["", f"{len(unjudged)} of {len(records)} sessions have no judgment and are left out: "
                      + ", ".join(sorted(unjudged))]
    return lines


def repair_lines(records: list[RealRecord], arm: str) -> list[str]:
    sessions = [
        [repairs.Tried(p.status, (*p.codes, *p.refusals)) for p in r.programs] for r in records if r.arm == arm
    ]
    return repairs.lines(repairs.tally(sessions))


def sessions_table(records: list[RealRecord]) -> list[str]:
    lines = [
        "| Request | Arm | Sample | Programs | Other tools | Scratch | Error |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    for r in sorted(records, key=lambda r: (r.request, r.arm, r.sample)):
        programs = ", ".join(f"{p.tool}:{p.status}" for p in r.programs) or "none"
        cells = [r.request, r.arm, str(r.sample), programs, ", ".join(r.other_tools) or "none",
                 ", ".join(r.scratch) or "empty", r.error or ""]
        lines.append("| " + " | ".join(cells) + " |")
    return lines


def main() -> None:
    parser = argparse.ArgumentParser(description="Run everyday requests through Sx code mode and pi codemode.")
    parser.add_argument("mode", choices=["sessions", "report"])
    parser.add_argument("--request", action="append", help="Request ids; every request by default")
    parser.add_argument("--arm", action="append", choices=[*SESSION_ARMS, "both"], help="both is sx and js")
    parser.add_argument(
        "--js-listing", choices=JS_LISTINGS, default="direct",
        help="direct lists the tools in codemode's description; discovered is Phase 17's",
    )
    parser.add_argument("--judgments", type=Path, help="JSON of REQUEST/ARM-SAMPLE to true or false for the report")
    parser.add_argument("--model", default="opencode-go/muse-spark-1.3-contributor")
    parser.add_argument("--thinking", default="medium")
    parser.add_argument("--samples", type=int, default=1)
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--out", required=True)
    args = parser.parse_args()
    out = Path(args.out).resolve()
    if args.mode == "report":
        records = [load_record(path) for path in sorted(out.glob("*/*/record.json"))]
        judgments: dict[str, bool] = json.loads(args.judgments.read_text()) if args.judgments else {}
        sections = ["## Real sessions", "", *summary(records), ""]
        if judgments:
            sections += ["## Gate: correctness and cost per session", "", *gate_lines(records, judgments), ""]
        for arm in REPORT_ARMS:
            if arm in CODE_SERVER_ARMS and any(r.arm == arm for r in records):
                sections += [f"## Programs stopped in the {arm} arm, by diagnostic, refusal, or trap", "",
                             *repair_lines(records, arm), ""]
        print("\n".join([*sections, *sessions_table(records)]))
        return
    out.mkdir(parents=True, exist_ok=True)
    imported = imported_declarations(out)
    requests = [r for r in load_requests() if args.request is None or r.id in args.request]
    arms = ARMS if not args.arm or "both" in args.arm else tuple(arm for arm in SESSION_ARMS if arm in args.arm)
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        jobs = [
            pool.submit(run_one, request, arm, args.model, args.thinking, sample, out, imported, args.js_listing)
            for request in requests for arm in arms for sample in range(args.samples)
        ]
        for job in jobs:
            r = job.result()
            programs = [f"{p.tool}:{p.status}" for p in r.programs]
            print(f"{r.request:24} {r.arm} {r.sample} {programs} other {r.other_tools} ${r.usage.cost:.4f}"
                  f"{'' if r.error is None else ' ERROR ' + r.error}", flush=True)


if __name__ == "__main__":
    main()
