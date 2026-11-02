from __future__ import annotations

import json
import re
import subprocess
import sys
import tempfile
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path

from .case import Case, Json
from .compare import ROOT, sx_binary
from .optionals import resolve, with_optional_nulls

SCRATCH = ROOT / ".work" / "tmp" / "tsdiff"
REWRITE = Path(__file__).resolve().parent / "rewrite.mjs"
PI_SERVERS = {"16": "tools", "17": "files"}
REPOMOD_SERVER = "repomod"
WRAPPED_RECORD = re.compile(r"Record \{result: (.*)\}")


@dataclass(frozen=True, slots=True)
class Nested:
    tool: str
    args: Json
    result: Json | None


@dataclass(slots=True)
class Script:
    id: str
    phase: str
    run: str
    task: str
    session: str
    index: int
    code: str
    directory: Path
    nested: list[Nested] = field(default_factory=list)


def read_events(path: Path) -> list[Json]:
    return [json.loads(line)["event"] for line in path.read_text().splitlines() if line]


def scripts_of(directory: Path, phase: str, run: str, task: str) -> list[Script]:
    events = read_events(directory / "events.jsonl")
    scripts: dict[str, Script] = {}
    nested: dict[str, list[Nested]] = {}
    ends: dict[str, Json] = {}
    for event in events:
        if event.get("type") == "tool_execution_end":
            ends[event["toolCallId"]] = event.get("result")
    for event in events:
        if event.get("type") != "tool_execution_start":
            continue
        parent = event.get("parentToolCallId")
        if parent is None and event["toolName"] == "codemode":
            index = len(scripts)
            scripts[event["toolCallId"]] = Script(
                f"{phase}-{run}-{task}-{directory.name}-{index}", phase, run, task, directory.name, index,
                str(event["args"].get("code", "")), directory,
            )
        elif parent is not None:
            nested.setdefault(parent, []).append(Nested(event["toolName"], event.get("args") or {}, ends.get(event["toolCallId"])))
    for call_id, script in scripts.items():
        script.nested = nested.get(call_id, [])
    return list(scripts.values())


def collect(effects: Path, real: Path) -> list[Script]:
    found: list[Script] = []
    for phase, root in (("16", effects), ("17", real)):
        for events in sorted(root.glob("*/*/js-*/events.jsonl")):
            directory = events.parent
            found += scripts_of(directory, phase, directory.parents[1].name, directory.parent.name)
    return found


def classify_scripts(scripts: list[Script]) -> dict[str, Json]:
    SCRATCH.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=SCRATCH) as scratch:
        source = Path(scratch) / "scripts.jsonl"
        source.write_text("".join(json.dumps({"id": s.id, "code": s.code}) + "\n" for s in scripts))
        completed = subprocess.run(["bun", str(REWRITE), str(source)], capture_output=True, text=True, check=False)
    if completed.returncode != 0:
        raise SystemExit(f"tsdiff: the rewrite failed (run `npm ci` in bench/subset): {completed.stderr.strip()[:400]}")
    return {row["id"]: row for row in map(json.loads, completed.stdout.splitlines())}


def pi_name(server: str, tool: str) -> str:
    return f"mcp__{server}__" + re.sub(r"[^A-Za-z0-9_]", "_", tool)


@dataclass(slots=True)
class Catalogue:
    declarations: Json
    grants: Json
    tools: list[dict[str, str]]
    outputs: dict[str, str]
    inputs: dict[str, str]
    types: dict[str, str]
    names: dict[str, tuple[str, str]]
    wrapped: dict[str, str]


def fs_grants(grants: Json) -> Json:
    return {name: {**scope, "roots": ["/"]} if "roots" in scope else scope for name, scope in (grants or {}).items() if scope is not None}


def describe(declarations: Json, grants: Json) -> Json:
    with tempfile.TemporaryDirectory(dir=SCRATCH) as scratch:
        files = Path(scratch)
        (files / "declarations.json").write_text(json.dumps(declarations))
        (files / "grants.json").write_text(json.dumps(grants))
        completed = subprocess.run(
            [str(sx_binary()), "mcp", "describe", "--declarations", str(files / "declarations.json"), "--grants", str(files / "grants.json")],
            capture_output=True, text=True, check=False,
        )
    if completed.returncode != 0:
        raise SystemExit(f"tsdiff: sx mcp describe failed: {completed.stderr.strip()[:300]}")
    return json.loads(completed.stdout)


def catalogue_16(directory: Path) -> Catalogue:
    declarations = json.loads((directory / "declarations.json").read_text())
    grants = fs_grants(json.loads((directory / "native-grants.json").read_text()))
    described = describe(declarations, grants)
    capabilities = described["capabilities"]
    names = {pi_name(PI_SERVERS["16"], c["id"]): (c["id"], "") for c in capabilities}
    return Catalogue(
        declarations, grants, [{"id": c["id"], "effect": c["effect"]} for c in capabilities],
        {c["id"]: c["output"] for c in capabilities}, {c["id"]: c["input"] for c in capabilities},
        {**described["types"]}, names, {},
    )


def catalogue_17(directory: Path, repomod: Json) -> Catalogue:
    grants = fs_grants(json.loads((directory / "grants.json").read_text()))
    fs_declarations: Json = {"packs": ["fs"]}
    described = describe(fs_declarations, grants)
    renamed: list[Json] = []
    names: dict[str, tuple[str, str]] = {}
    wrapped: dict[str, str] = {}
    for capability in repomod["capabilities"]:
        tool = capability["mcp"]["tool"]
        plain = {k: v for k, v in capability.items() if k != "mcp"}
        plain["id"] = tool
        unwrapped = WRAPPED_RECORD.fullmatch(capability["output"].strip())
        if unwrapped is not None:
            plain["output"] = unwrapped.group(1)
        renamed.append(plain)
        mode = "wrapped" if unwrapped is not None else capability["mcp"]["result"]
        names[pi_name(REPOMOD_SERVER, tool)] = (tool, mode)
        wrapped[tool] = mode
    for capability in described["capabilities"]:
        names[pi_name(PI_SERVERS["17"], capability["id"])] = (capability["id"], "")
    declarations: Json = {"packs": ["fs"], "enums": repomod.get("enums", {}), "capabilities": renamed}
    if repomod.get("types"):
        declarations["types"] = repomod["types"]
    capabilities = [*described["capabilities"], *renamed]
    return Catalogue(
        declarations, grants, [{"id": c["id"], "effect": c["effect"]} for c in capabilities],
        {c["id"]: c["output"] for c in capabilities}, {c["id"]: c["input"] for c in capabilities},
        {**described["types"], **repomod.get("types", {})}, names, wrapped,
    )


def decode_result(result: Json, output_type: str, mode: str, types: dict[str, str]) -> tuple[str, Json]:
    if result is None:
        return "missing", None
    while isinstance(result.get("structuredContent"), dict) and "content" in result["structuredContent"] and "isError" in result["structuredContent"]:
        result = result["structuredContent"]
    if result.get("isError"):
        found = result
        for _ in range(4):
            structured = found.get("structuredContent")
            if isinstance(structured, dict) and isinstance(structured.get("kind"), str) and isinstance(structured.get("message"), str):
                return "error", {"kind": structured["kind"], "message": structured["message"]}
            if not isinstance(structured, dict):
                break
            found = structured
        text = "".join(block.get("text", "") for block in result.get("content", []) if block.get("type") == "text")
        return "error", {"kind": "Failed", "message": text}
    structured = result.get("structuredContent")
    if mode == "text" or (mode == "" and structured is None):
        return "ok", "".join(block.get("text", "") for block in result.get("content", []) if block.get("type") == "text")
    if structured is None:
        return "missing", None
    if mode == "structured" or (mode != "wrapped" and resolve(output_type, types).startswith("Record")):
        return "ok", structured
    return "ok", structured.get("result") if isinstance(structured, dict) and set(structured) == {"result"} else structured


def responses_of(script: Script, catalogue: Catalogue) -> tuple[list[Json] | None, str]:
    grouped: dict[tuple[str, str], Json] = {}
    for call in script.nested:
        if call.tool not in catalogue.names:
            return None, f"calls {call.tool}, which the catalogue does not declare"
        capability, mode = catalogue.names[call.tool]
        kind, value = decode_result(call.result, catalogue.outputs[capability], mode, catalogue.types)
        if kind == "missing":
            continue
        shaped = with_optional_nulls(call.args, catalogue.inputs[capability], catalogue.types)
        key = (capability, json.dumps({k: v for k, v in shaped.items() if v is not None}, sort_keys=True))
        entry = grouped.setdefault(key, {"capability": capability, "input": shaped, "results": []})
        entry["results"].append({"error": value} if kind == "error" else {"output": value})
    return list(grouped.values()), ""


def build_corpus(effects: Path, real: Path, out: Path) -> list[str]:
    scripts = collect(effects, real)
    classified = classify_scripts(scripts)
    repomod = next(iter(sorted(real.glob("*/repomod-declarations.json"))), None)
    if repomod is None:
        raise SystemExit(f"tsdiff: no repomod-declarations.json under {real}")
    repomod_declarations = json.loads(repomod.read_text())
    catalogues: dict[Path, Catalogue] = {}
    skipped: Counter[str] = Counter()
    built: Counter[str] = Counter()
    for script in scripts:
        row = classified[script.id]
        if not row["parsed"]:
            skipped["does not parse"] += 1
            continue
        if not row["callsTools"]:
            skipped["calls no tool"] += 1
            continue
        if "tools.pi." in row["rewritten"]:
            skipped["uses pi's own tools, which no declaration covers"] += 1
            continue
        if script.directory not in catalogues:
            catalogues[script.directory] = (
                catalogue_16(script.directory) if script.phase == "16" else catalogue_17(script.directory, repomod_declarations)
            )
        catalogue = catalogues[script.directory]
        responses, why = responses_of(script, catalogue)
        if responses is None:
            skipped[why.split(",")[0].split(" calls ")[0] + " (undeclared tool)"] += 1
            continue
        allow = tuple(c["id"] for c in catalogue.declarations.get("capabilities", []))
        Case(
            name=script.id, program=row["rewritten"], declarations=catalogue.declarations, responses=responses,
            tools=catalogue.tools, allow=allow, grants=catalogue.grants or None,
            meta={
                "source": "corpus", "phase": script.phase, "run": script.run, "task": script.task,
                "session": script.session, "classifier_accepted": row["accepted"], "classifier_codes": row["codes"],
            },
        ).write(out / script.id)
        built[f"phase {script.phase}"] += 1
    lines = [f"scripts {len(scripts)}", f"cases built {sum(built.values())} {dict(built)}"]
    lines += [f"skipped {count}: {why}" for why, count in skipped.most_common()]
    print(f"written under {out}", file=sys.stderr)
    return lines
