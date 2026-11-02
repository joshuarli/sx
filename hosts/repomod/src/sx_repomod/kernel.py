from __future__ import annotations

import json
import os
import subprocess
from collections.abc import Mapping
from pathlib import Path
from types import TracebackType
from typing import IO, Any, Self

PROTOCOL_VERSION = 1
BINARY_ENVIRONMENT = "SX_KERNEL"
BUILD_HINT = "cargo build --release --manifest-path rust/Cargo.toml -p sx-cli"


class KernelError(Exception):
    def __init__(self, code: str, message: str) -> None:
        super().__init__(f"{code}: {message}")
        self.code = code
        self.message = message


def locate() -> Path:
    configured = os.environ.get(BINARY_ENVIRONMENT)
    if configured is not None:
        return Path(configured)
    for parent in Path(__file__).resolve().parents:
        for profile in ("release", "debug"):
            candidate = parent / "rust" / "target" / profile / "sx"
            if candidate.is_file():
                return candidate
    raise KernelError("kernel-missing", f"build the Rust kernel with `{BUILD_HINT}` or set {BINARY_ENVIRONMENT} to sx")


class Kernel:
    def __init__(self, binary: Path | None = None) -> None:
        self.process = subprocess.Popen(
            [str(binary or locate()), "serve"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            bufsize=1,
        )
        self.versioned = False

    @property
    def stdin(self) -> IO[str]:
        assert self.process.stdin is not None
        return self.process.stdin

    @property
    def stdout(self) -> IO[str]:
        assert self.process.stdout is not None
        return self.process.stdout

    def send(self, message: Mapping[str, Any]) -> dict[str, Any]:
        versioned = message if self.versioned else {"version": PROTOCOL_VERSION, **message}
        self.versioned = True
        try:
            self.stdin.write(json.dumps(versioned) + "\n")
            self.stdin.flush()
        except BrokenPipeError:
            pass
        line = self.stdout.readline()
        if not line:
            raise KernelError("kernel-exited", f"the kernel exited with status {self.process.wait()}")
        reply: dict[str, Any] = json.loads(line)
        if reply["type"] == "error":
            raise KernelError(reply["code"], reply["message"])
        return reply

    def close(self) -> None:
        self.stdin.close()
        self.stdout.close()
        self.process.wait()

    def __enter__(self) -> Self:
        return self

    def __exit__(
        self, kind: type[BaseException] | None, error: BaseException | None, traceback: TracebackType | None
    ) -> None:
        self.close()
