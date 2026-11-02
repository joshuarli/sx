import json
import queue
import subprocess
import threading
from collections.abc import Sequence
from types import TracebackType
from typing import IO, Any, Self

WAIT = 60.0


class McpClient:
    def __init__(self, command: Sequence[str]) -> None:
        self.process = subprocess.Popen(
            list(command),
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
        )
        self.lines: queue.Queue[str] = queue.Queue()
        self.errors = ""
        threading.Thread(target=self.pump_lines, args=(self.stdout,), daemon=True).start()
        threading.Thread(target=self.collect_errors, daemon=True).start()
        self.next_id = 0

    @property
    def stdin(self) -> IO[str]:
        assert self.process.stdin is not None
        return self.process.stdin

    @property
    def stdout(self) -> IO[str]:
        assert self.process.stdout is not None
        return self.process.stdout

    def pump_lines(self, stream: IO[str]) -> None:
        for line in stream:
            self.lines.put(line)

    def collect_errors(self) -> None:
        assert self.process.stderr is not None
        self.errors = self.process.stderr.read()

    def send(self, message: dict[str, Any]) -> None:
        self.stdin.write(json.dumps(message) + "\n")
        self.stdin.flush()

    def request(self, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
        self.next_id += 1
        self.send({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params or {}})
        reply: dict[str, Any] = json.loads(self.lines.get(timeout=WAIT))
        assert reply["id"] == self.next_id, reply
        return reply

    def initialize(self) -> dict[str, Any]:
        reply = self.request(
            "initialize",
            {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}},
        )
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        return reply

    def tool(self, name: str, arguments: dict[str, Any]) -> dict[str, Any]:
        reply = self.request("tools/call", {"name": name, "arguments": arguments})
        result: dict[str, Any] = reply["result"]
        return result

    def close(self) -> int:
        self.stdin.close()
        return self.process.wait(timeout=WAIT)

    def __enter__(self) -> Self:
        return self

    def __exit__(
        self, kind: type[BaseException] | None, error: BaseException | None, traceback: TracebackType | None
    ) -> None:
        if self.process.poll() is None:
            self.process.kill()
            self.process.wait()
        for stream in (self.process.stdin, self.process.stdout, self.process.stderr):
            if stream is not None:
                stream.close()
