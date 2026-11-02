import json
import subprocess
from types import TracebackType
from typing import Any, Self

PROTOCOL_VERSION = "2025-06-18"
CALL_TIMEOUT_SECONDS = 300

type Json = dict[str, Any]


class McpError(RuntimeError):
    pass


class McpClient:
    def __init__(self, command: list[str]) -> None:
        self.child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        self.next_id = 0
        self.request("initialize", {
            "protocolVersion": PROTOCOL_VERSION, "capabilities": {},
            "clientInfo": {"name": "sx-workflows", "version": "0"},
        })
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def send(self, message: Json) -> None:
        assert self.child.stdin is not None
        self.child.stdin.write(json.dumps(message) + "\n")
        self.child.stdin.flush()

    def request(self, method: str, params: Json) -> Json:
        assert self.child.stdout is not None
        self.next_id += 1
        self.send({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params})
        for line in self.child.stdout:
            message: Json = json.loads(line)
            if message.get("id") != self.next_id:
                continue
            if "error" in message:
                raise McpError(f"{method}: {message['error']}")
            result: Json = message["result"]
            return result
        raise McpError(f"the server closed its output before answering {method}")

    def call_tool(self, name: str, arguments: Json) -> Json:
        return self.request("tools/call", {"name": name, "arguments": arguments})

    def close(self) -> None:
        assert self.child.stdin is not None
        self.child.stdin.close()
        self.child.wait(timeout=CALL_TIMEOUT_SECONDS)

    def __enter__(self) -> Self:
        return self

    def __exit__(
        self, kind: type[BaseException] | None, error: BaseException | None, trace: TracebackType | None
    ) -> None:
        self.close()
