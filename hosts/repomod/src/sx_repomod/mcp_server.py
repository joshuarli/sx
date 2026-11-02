from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from collections.abc import Mapping
from dataclasses import dataclass
from enum import StrEnum
from pathlib import Path
from typing import IO, Any

from sx_repomod.capabilities import Capabilities, CapabilityError
from sx_repomod.declarations import host_declarations
from sx_repomod.kernel import KernelError, locate

PROTOCOL_VERSION = "2025-06-18"
ACCEPTED_VERSIONS = (PROTOCOL_VERSION, "2025-03-26", "2024-11-05")
WRAPPED_KEY = "result"

PARSE_ERROR = -32700
INVALID_REQUEST = -32600
METHOD_NOT_FOUND = -32601
INVALID_PARAMS = -32602


class Shape(StrEnum):
    TEXT = "text"
    STRUCTURED = "structured"
    WRAPPED = "wrapped"


@dataclass(frozen=True, slots=True)
class Listing:
    tools: tuple[dict[str, Any], ...]
    shapes: Mapping[str, Shape]


def export_listing() -> Listing:
    with tempfile.TemporaryDirectory(prefix="sx-repomod-") as scratch:
        declarations = Path(scratch) / "declarations.json"
        declarations.write_text(json.dumps(host_declarations()))
        done = subprocess.run(
            [str(locate()), "mcp", "export", "--declarations", str(declarations)],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            encoding="utf-8",
            check=False,
        )
    sys.stderr.write(done.stderr)
    if done.returncode != 0:
        raise KernelError("export-failed", f"sx mcp export exited {done.returncode}")
    exported = json.loads(done.stdout)
    return Listing(
        tuple(exported["tools"]),
        {name: Shape(shape) for name, shape in exported["shapes"].items()},
    )


def text_block(text: str) -> dict[str, str]:
    return {"type": "text", "text": text}


def error_result(kind: str, message: str) -> dict[str, Any]:
    return {
        "content": [text_block(f"{kind}: {message}")],
        "structuredContent": {"kind": kind, "message": message},
        "isError": True,
    }


def structured(content: Any) -> dict[str, Any]:
    return {"content": [text_block(json.dumps(content))], "structuredContent": content, "isError": False}


def success_result(shape: Shape, output: Any) -> dict[str, Any]:
    match shape:
        case Shape.TEXT if isinstance(output, str):
            return {"content": [text_block(output)], "isError": False}
        case Shape.TEXT:
            return error_result("Failed", f"a text result was {json.dumps(output)}")
        case Shape.STRUCTURED:
            return structured(output)
        case Shape.WRAPPED:
            return structured({WRAPPED_KEY: output})


def mismatch(value: Any, schema: Mapping[str, Any], path: str) -> str | None:
    match schema.get("type"):
        case "string" if isinstance(value, str):
            return None
        case "boolean" if isinstance(value, bool):
            return None
        case "integer" if isinstance(value, int) and not isinstance(value, bool):
            return None
        case "array" if isinstance(value, list):
            for index, item in enumerate(value):
                if (problem := mismatch(item, schema["items"], f"{path}[{index}]")) is not None:
                    return problem
            return None
        case "object" if isinstance(value, dict):
            properties: Mapping[str, Any] = schema["properties"]
            for name in schema["required"]:
                if name not in value:
                    return f"{path}: missing field {name}"
            for name, item in value.items():
                if name not in properties:
                    return f"{path}: unknown field {name}"
                if (problem := mismatch(item, properties[name], f"{path}.{name}")) is not None:
                    return problem
            return None
        case "string" | "boolean" | "integer" | "array" | "object" as expected:
            return f"{path}: expected {expected}"
        case other:
            raise ValueError(f"{path}: the schema type {other!r} is not one the repomod capabilities use")


class RequestRejectedError(Exception):
    def __init__(self, code: int, message: str) -> None:
        super().__init__(message)
        self.code = code
        self.message = message


class Server:
    def __init__(self, capabilities: Capabilities, listing: Listing) -> None:
        self.capabilities = capabilities
        self.listing = listing
        self.tools = {tool["name"]: tool for tool in listing.tools}

    def initialize(self, params: Mapping[str, Any]) -> dict[str, Any]:
        asked = params.get("protocolVersion")
        return {
            "protocolVersion": asked if asked in ACCEPTED_VERSIONS else PROTOCOL_VERSION,
            "capabilities": {"tools": {"listChanged": False}},
            "serverInfo": {"name": "sx-repomod", "version": "0.1.0"},
        }

    def call(self, params: Mapping[str, Any]) -> dict[str, Any]:
        name = params.get("name")
        if not isinstance(name, str):
            raise RequestRejectedError(INVALID_PARAMS, "tools/call needs a string name")
        if name not in self.tools:
            raise RequestRejectedError(INVALID_PARAMS, f"unknown tool {name}")
        arguments = params.get("arguments")
        if arguments is None:
            arguments = {}
        if not isinstance(arguments, dict):
            raise RequestRejectedError(INVALID_PARAMS, "arguments must be an object")
        if (problem := mismatch(arguments, self.tools[name]["inputSchema"], "$")) is not None:
            return error_result("Invalid", f"{name}: the arguments do not match the input type: {problem}")
        try:
            output = self.capabilities.call(name, arguments)
        except CapabilityError as failure:
            return error_result(failure.kind, failure.message)
        except Exception as failure:
            return error_result("Failed", f"performing {name} raised {type(failure).__name__}: {failure}")
        return success_result(self.listing.shapes[name], output)

    def answer(self, method: str, params: Mapping[str, Any]) -> Any:
        match method:
            case "initialize":
                return self.initialize(params)
            case "ping":
                return {}
            case "tools/list":
                if params.get("cursor") is not None:
                    raise RequestRejectedError(INVALID_PARAMS, "this server issues no cursors")
                return {"tools": list(self.listing.tools)}
            case "tools/call":
                return self.call(params)
            case other:
                raise RequestRejectedError(METHOD_NOT_FOUND, f"sx-repomod does not implement {other}")

    def handle(self, message: Any) -> dict[str, Any] | None:
        if not isinstance(message, dict):
            return rpc_error(None, INVALID_REQUEST, "a message must be a JSON object")
        request_id = message.get("id")
        method = message.get("method")
        if not isinstance(method, str):
            if request_id is None or "result" in message or "error" in message:
                return None
            return rpc_error(request_id, INVALID_REQUEST, "a request needs a method")
        params = message.get("params")
        try:
            result = self.answer(method, params if isinstance(params, dict) else {})
        except RequestRejectedError as rejected:
            return None if request_id is None else rpc_error(request_id, rejected.code, rejected.message)
        return None if request_id is None else {"jsonrpc": "2.0", "id": request_id, "result": result}


def rpc_error(request_id: Any, code: int, message: str) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": request_id, "error": {"code": code, "message": message}}


def serve(server: Server, lines: IO[str], output: IO[str]) -> None:
    for line in lines:
        if not line.strip():
            continue
        try:
            message = json.loads(line)
        except ValueError as failure:
            reply: dict[str, Any] | None = rpc_error(None, PARSE_ERROR, f"a line is not JSON: {failure}")
        else:
            reply = server.handle(message)
        if reply is not None:
            output.write(json.dumps(reply) + "\n")
            output.flush()
