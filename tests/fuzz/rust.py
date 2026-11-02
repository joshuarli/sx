from __future__ import annotations

import json
import os
import subprocess
import threading
from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import IO, Any

ROOT = Path(__file__).resolve().parent.parent.parent
BINARY_ENVIRONMENT = "SX_KERNEL"
BUILD_HINT = "cargo build --release --manifest-path rust/Cargo.toml -p sx-cli"
PROTOCOL_VERSION = 1
SERVE_TIMEOUT = 60
CHECK_TIMEOUT = 60
SCRATCH = ROOT / ".work" / "tmp" / "fuzz"
DECLARATION_FILES = (
    ROOT / "fixtures" / "run" / "declarations.json",
    Path(__file__).resolve().parent / "declarations.json",
)


class KernelGoneError(Exception):
    pass


def locate() -> Path:
    configured = os.environ.get(BINARY_ENVIRONMENT)
    if configured is not None:
        return Path(configured)
    for profile in ("release", "debug"):
        candidate = ROOT / "rust" / "target" / profile / "sx"
        if candidate.is_file():
            return candidate
    raise SystemExit(f"tests.fuzz: build the Rust sx with `{BUILD_HINT}` or set {BINARY_ENVIRONMENT} to it")


def merged_declarations() -> dict[str, Any]:
    merged: dict[str, Any] = {"enums": {}, "types": {}, "capabilities": []}
    for path in DECLARATION_FILES:
        declared = json.loads(path.read_text())
        merged["enums"].update(declared.get("enums", {}))
        merged["types"].update(declared.get("types", {}))
        merged["capabilities"].extend(declared.get("capabilities", []))
    return merged


def declarations_file(declarations: Mapping[str, Any]) -> Path:
    SCRATCH.mkdir(parents=True, exist_ok=True)
    target = SCRATCH / "declarations.json"
    staging = SCRATCH / f"declarations-{os.getpid()}.json"
    staging.write_text(json.dumps(declarations))
    staging.replace(target)
    return target


class Rust:
    def __init__(self, work: Path) -> None:
        self.binary = locate()
        self.work = work
        self.declarations = merged_declarations()
        self.declarations_path = declarations_file(self.declarations)
        self.process: subprocess.Popen[str] | None = None
        self.errors: IO[str] | None = None
        self.versioned = False
        self.busy = False
        self.timed_out = False
        work.mkdir(parents=True, exist_ok=True)

    def spawn(self) -> subprocess.Popen[str]:
        if self.process is None:
            self.errors = (self.work / "serve.stderr").open("w")
            self.process = subprocess.Popen(
                [self.binary, "serve"],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=self.errors,
                text=True,
                encoding="utf-8",
                bufsize=1,
            )
            self.versioned = False
            self.busy = False
        return self.process

    def transmit(self, message: Mapping[str, Any]) -> subprocess.Popen[str]:
        process = self.spawn()
        versioned = message if self.versioned else {"version": PROTOCOL_VERSION, **message}
        self.versioned = True
        try:
            stdin = self.pipe(process.stdin)
            stdin.write(json.dumps(versioned) + "\n")
            stdin.flush()
        except BrokenPipeError:
            pass
        return process

    def send(self, message: Mapping[str, Any]) -> dict[str, Any]:
        process = self.transmit(message)
        self.timed_out = False
        watchdog = threading.Timer(SERVE_TIMEOUT, self.expire, args=(process,))
        watchdog.start()
        try:
            line = self.pipe(process.stdout).readline()
        finally:
            watchdog.cancel()
        if not line:
            raise KernelGoneError(self.farewell(process))
        reply: dict[str, Any] = json.loads(line)
        self.busy = reply["type"] == "requests"
        return reply

    def expire(self, process: subprocess.Popen[str]) -> None:
        self.timed_out = True
        process.kill()

    @staticmethod
    def pipe(stream: IO[str] | None) -> IO[str]:
        assert stream is not None
        return stream

    def farewell(self, process: subprocess.Popen[str]) -> str:
        status = process.wait()
        self.release(process)
        stderr = (self.work / "serve.stderr").read_text().strip()
        why = f"no reply within {SERVE_TIMEOUT} s" if self.timed_out else f"exited with status {status}"
        return f"sx serve {why}\n{stderr}".strip()

    def start(self, source: str, allow: Sequence[str], budgets: Mapping[str, int]) -> dict[str, Any]:
        return self.send(
            {
                "type": "start",
                "program": source,
                "allow": list(allow),
                "declarations": self.declarations,
                "budgets": dict(budgets),
            }
        )

    def respond(self, responses: Sequence[Mapping[str, Any]]) -> dict[str, Any]:
        return self.send({"type": "respond", "responses": list(responses)})

    def answer_some_then_cancel(self, responses: Sequence[Mapping[str, Any]]) -> dict[str, Any]:
        if responses:
            self.transmit({"type": "respond", "responses": list(responses)})
        return self.cancel()

    def cancel(self) -> dict[str, Any]:
        return self.send({"type": "cancel"})

    def settle(self) -> None:
        if self.busy:
            self.reset()

    def reset(self) -> None:
        process = self.process
        if process is not None:
            process.kill()
            process.wait()
            self.release(process)

    def release(self, process: subprocess.Popen[str]) -> None:
        self.process = None
        self.busy = False
        for stream in (process.stdin, process.stdout, self.errors):
            if stream is not None:
                stream.close()
        self.errors = None

    def emit_ir(self, program: Path, allow: Sequence[str]) -> subprocess.CompletedProcess[str]:
        command = [str(self.binary), "check", str(program), "--declarations", str(self.declarations_path)]
        command += ["--emit", "ir"]
        for capability in allow:
            command += ["--allow", capability]
        return subprocess.run(command, capture_output=True, text=True, check=False, timeout=CHECK_TIMEOUT)
