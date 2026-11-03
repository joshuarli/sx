from __future__ import annotations

import json
import os
import re
import subprocess
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Literal

from .case import CaseFiles, Json

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
SHIM = HERE / "shim.ts"
SX_ENVIRONMENT = "SX_BIN"
BUN_TIMEOUT = 60
SX_TIMEOUT = 60
TRAPS = ("arithmetic", "out-of-range", "loop-limit")
KINDS = ("Timeout", "Unavailable", "RateLimited", "NotFound", "PermissionDenied", "Invalid", "Conflict", "Failed")
CALL_FAILED = re.compile(r"failed with (" + "|".join(KINDS) + r")\b")

type Status = Literal["completed", "failed", "rejected"]
type VerdictKind = Literal["agree", "agree-refined", "rejected", "disagree"]


class SxUnsupportedLanguageError(RuntimeError):
    pass


class SxProtocolError(RuntimeError):
    pass


@dataclass(frozen=True, slots=True)
class Failure:
    cls: str
    kind: str | None = None
    message: str = ""


@dataclass(frozen=True, slots=True)
class Outcome:
    side: Literal["js", "sx"]
    status: Status
    result: Json
    logs: tuple[str, ...]
    calls: tuple[tuple[str, str], ...]
    delays: tuple[int, ...]
    failure: Failure | None = None
    codes: tuple[str, ...] = ()

    @property
    def trap(self) -> str | None:
        return self.failure.cls if self.failure is not None and self.failure.cls in TRAPS else None


@dataclass(frozen=True, slots=True)
class Verdict:
    kind: VerdictKind
    reasons: tuple[str, ...] = ()


def canonical(value: Json) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"))


def call_of(capability: str, input_value: Json) -> tuple[str, str]:
    if isinstance(input_value, dict):
        input_value = {k: v for k, v in input_value.items() if v is not None}
    return capability, canonical(input_value)


def same_json(a: Json, b: Json) -> bool:
    if isinstance(a, bool) or isinstance(b, bool):
        return isinstance(a, bool) and isinstance(b, bool) and a == b
    if isinstance(a, int | float) and isinstance(b, int | float):
        return a == b
    if isinstance(a, dict) and isinstance(b, dict):
        return a.keys() == b.keys() and all(same_json(a[k], b[k]) for k in a)
    if isinstance(a, list) and isinstance(b, list):
        return len(a) == len(b) and all(same_json(x, y) for x, y in zip(a, b, strict=True))
    return bool(type(a) is type(b) and a == b)


def normalize_js(raw: Json) -> Outcome:
    error = raw.get("error")
    failure: Failure | None = None
    if error is not None:
        match error["class"]:
            case "call-failed":
                failure = Failure("call-failed", error["kind"], error["message"])
            case "trap":
                failure = Failure(error["trap"], None, error["message"])
            case other:
                failure = Failure(other, None, error.get("message", ""))
    return Outcome(
        side="js",
        status="failed" if failure is not None else "completed",
        result=None if failure is not None else raw["result"],
        logs=tuple(raw["logs"]),
        calls=tuple(call_of(c["capability"], c["input"]) for c in raw["calls"]),
        delays=tuple(int(d) for d in raw["delays"]),
        failure=failure,
    )


def normalize_sx(raw: Json) -> Outcome:
    if raw.get("ok") is False:
        codes = tuple(str(d["code"]) for d in raw["diagnostics"])
        return Outcome("sx", "rejected", None, (), (), (), None, codes)
    trace = raw.get("trace", [])
    calls = tuple(call_of(t["capability"], t["input"]) for t in trace if "request" in t)
    delays = tuple(int(t["milliseconds"]) for t in trace if "delay" in t)
    logs = tuple(raw.get("logs", ()))
    outcome = raw["outcome"]
    if outcome == "completed":
        return Outcome("sx", "completed", raw["result"], logs, calls, delays)
    if outcome == "unexpected-call":
        listed = tuple(call_of(c["capability"], c["input"]) for c in raw.get("calls", []))
        failure = Failure("unexpected-call", None, raw.get("message", ""))
        return Outcome("sx", "failed", None, logs, listed, delays, failure)
    failure = raw["failure"]
    kind = None
    if failure["kind"] == "call-failed":
        found = CALL_FAILED.search(failure["message"])
        kind = found.group(1) if found else None
    return Outcome("sx", "failed", None, logs, calls, delays, Failure(failure["kind"], kind, failure["message"]))


def shim_raw(program: Path, tools: Path, responses: Path) -> Json:
    completed = subprocess.run(
        ["bun", str(SHIM), str(program), "--tools", str(tools), "--responses", str(responses)],
        capture_output=True, text=True, timeout=BUN_TIMEOUT, check=False,
    )
    if completed.returncode != 0:
        raise RuntimeError(f"the shim exited {completed.returncode} on {program}: {completed.stderr.strip()}")
    return json.loads(completed.stdout)


def run_js(files: CaseFiles) -> Outcome:
    return normalize_js(shim_raw(files.program, files.tools, files.responses))


def sx_binary() -> Path:
    configured = os.environ.get(SX_ENVIRONMENT)
    if configured is not None:
        return Path(configured)
    for profile in ("debug", "release"):
        candidate = ROOT / "rust" / "target" / profile / "sx"
        if candidate.is_file():
            return candidate
    raise SystemExit(f"tsdiff: set {SX_ENVIRONMENT} to an sx binary that supports `run --language ts`")


def sx_command(files: CaseFiles, binary: Path) -> list[str]:
    command: list[str] = [
        str(binary), "run", str(files.program), "--language", "ts", "--declarations", str(files.declarations),
        "--responses", str(files.responses), "--format", "json",
    ]
    for capability in files.allow:
        command += ["--allow", capability]
    if files.grants is not None:
        command += ["--grants", str(files.grants)]
    return command


def run_sx(files: CaseFiles, binary: Path | None = None) -> Outcome:
    command = sx_command(files, binary or sx_binary())
    completed = subprocess.run(command, capture_output=True, text=True, timeout=SX_TIMEOUT, check=False)
    try:
        raw = json.loads(completed.stdout)
    except json.JSONDecodeError:
        refused = "--language" in completed.stderr or "unrecognized" in completed.stderr
        detail = completed.stderr.strip() or completed.stdout.strip()
        if refused:
            raise SxUnsupportedLanguageError(f"this sx does not accept `run --language ts`: {detail}") from None
        raise SxProtocolError(f"sx exited {completed.returncode} without JSON on {files.directory}: {detail}") from None
    if not isinstance(raw, dict) or not ("outcome" in raw or raw.get("ok") is False):
        raise SxProtocolError(f"sx printed an unrecognized report on {files.directory}: {completed.stdout[:200]}")
    return normalize_sx(raw)


def failure_differences(js: Failure, sx: Failure) -> list[str]:
    if js.cls != sx.cls:
        return [f"failure class: js {js.cls}, sx {sx.cls}"]
    found: list[str] = []
    if js.cls == "call-failed" and js.kind != sx.kind and sx.kind is not None:
        found.append(f"call error kind: js {js.kind}, sx {sx.kind}")
    if js.cls == "thrown" and js.message not in sx.message:
        found.append(f"thrown message: js {js.message!r}, sx {sx.message!r}")
    return found


def multiset_contains(whole: tuple[object, ...], part: tuple[object, ...]) -> bool:
    return not (Counter(part) - Counter(whole))


def compare(js: Outcome, sx: Outcome, *, parallel: bool, expect_trap: str | None = None) -> Verdict:
    if sx.status == "rejected":
        return Verdict("rejected", sx.codes)
    if js.failure is not None and js.failure.cls == "js-error":
        return Verdict("disagree", (f"js raised {js.failure.message!r} but sx accepted the program",))
    reasons: list[str] = []
    expected = js.trap or expect_trap
    if sx.trap is not None and sx.trap != expected:
        seen = f"trapped differently: {js.trap}" if js.trap is not None else "did not trap"
        reasons.append(f"sx trapped ({sx.trap}) where js {seen}")
    elif sx.trap is not None:
        pass
    elif js.trap is not None:
        reasons.append(f"js trapped ({js.trap}) but sx {'completed' if sx.status == 'completed' else sx.failure}")
    elif js.status != sx.status:
        reasons.append(f"outcome: js {js.status} {js.failure}, sx {sx.status} {sx.failure}")
    elif js.failure is not None and sx.failure is not None:
        reasons += failure_differences(js.failure, sx.failure)
    elif not same_json(js.result, sx.result):
        reasons.append(f"result: js {canonical(js.result)[:300]}, sx {canonical(sx.result)[:300]}")
    unrecorded = js.failure is not None and sx.failure is not None and js.failure.cls == sx.failure.cls == "unexpected-call"
    if js.logs != sx.logs and not unrecorded:
        reasons.append(f"logs: js {list(js.logs)[:5]}, sx {list(sx.logs)[:5]}")
    refined = False
    if js.calls != sx.calls:
        reordered = parallel and multiset_contains(js.calls, sx.calls) and (
            Counter(js.calls) == Counter(sx.calls) or sx.status == "failed"
        )
        if reordered:
            refined = True
        else:
            reasons.append(f"calls: js {list(js.calls)[:6]}, sx {list(sx.calls)[:6]}")
    if js.delays != sx.delays:
        if parallel and Counter(js.delays) == Counter(sx.delays):
            refined = True
        else:
            reasons.append(f"delays: js {list(js.delays)}, sx {list(sx.delays)}")
    if reasons:
        return Verdict("disagree", tuple(reasons))
    return Verdict("agree-refined" if refined else "agree")


PARALLEL = re.compile(r"\b(Promise\.all|Promise\.allSettled|parMap)\b")


def compare_case(files: CaseFiles, binary: Path | None = None) -> tuple[Outcome, Outcome, Verdict]:
    js = run_js(files)
    sx = run_sx(files, binary)
    parallel = PARALLEL.search(files.program.read_text()) is not None
    return js, sx, compare(js, sx, parallel=parallel, expect_trap=files.expect_trap)
