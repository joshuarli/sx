import argparse
import json
import signal
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import IO, Any

SHUTDOWN_SECONDS = 5


class Log:
    def __init__(self, path: Path) -> None:
        self.file = path.open("a", buffering=1)
        self.lock = threading.Lock()

    def write(self, entry: dict[str, Any]) -> None:
        line = json.dumps({**entry, "time_ns": time.time_ns()})
        with self.lock:
            self.file.write(line + "\n")


class Calls:
    def __init__(self, log: Log) -> None:
        self.log = log
        self.names: dict[str, str] = {}
        self.lock = threading.Lock()

    def request(self, message: dict[str, Any]) -> None:
        if message.get("method") != "tools/call" or "id" not in message:
            return
        params = message.get("params") or {}
        key = json.dumps(message["id"])
        with self.lock:
            self.names[key] = params.get("name", "")
        self.log.write({
            "kind": "request", "id": message["id"], "name": params.get("name"), "arguments": params.get("arguments"),
        })

    def response(self, message: dict[str, Any]) -> None:
        if "id" not in message or ("result" not in message and "error" not in message):
            return
        with self.lock:
            name = self.names.pop(json.dumps(message["id"]), None)
        if name is None:
            return
        self.log.write({
            "kind": "response", "id": message["id"], "name": name,
            **({"result": message["result"]} if "result" in message else {"error": message["error"]}),
        })


def decoded(line: bytes) -> dict[str, Any] | None:
    try:
        message = json.loads(line)
    except json.JSONDecodeError:
        return None
    return message if isinstance(message, dict) else None


def pump_server_output(server: subprocess.Popen[bytes], client: IO[bytes], calls: Calls) -> None:
    assert server.stdout is not None
    for line in server.stdout:
        message = decoded(line)
        if message is not None:
            calls.response(message)
        client.write(line)
        client.flush()


def pump_client_input(client: IO[bytes], server: subprocess.Popen[bytes], calls: Calls) -> None:
    assert server.stdin is not None
    for line in client:
        message = decoded(line)
        if message is not None:
            calls.request(message)
        server.stdin.write(line)
        server.stdin.flush()
    server.stdin.close()


def main() -> int:
    parser = argparse.ArgumentParser(description="Forward an MCP stdio server and log every tools/call.")
    parser.add_argument("--log", type=Path, required=True)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("a server command is needed after --")
    calls = Calls(Log(args.log))
    server = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE)

    def terminate(_signal: int, _frame: object) -> None:
        server.terminate()
        raise SystemExit(143)

    signal.signal(signal.SIGTERM, terminate)
    reader = threading.Thread(target=pump_server_output, args=(server, sys.stdout.buffer, calls), daemon=True)
    reader.start()
    try:
        pump_client_input(sys.stdin.buffer, server, calls)
        try:
            server.wait(SHUTDOWN_SECONDS)
        except subprocess.TimeoutExpired:
            server.terminate()
            server.wait()
        reader.join()
    finally:
        if server.poll() is None:
            server.kill()
    return server.wait()


if __name__ == "__main__":
    raise SystemExit(main())
