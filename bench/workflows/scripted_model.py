import json
import threading
from dataclasses import dataclass
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any, override

PROVIDER = "scripted"
MODEL = "scripted"
MODEL_FLAG = f"{PROVIDER}/{MODEL}"


@dataclass(frozen=True, slots=True)
class Say:
    text: str


@dataclass(frozen=True, slots=True)
class Call:
    name: str
    arguments: dict[str, Any]


type Reply = Say | Call


def chunk(delta: dict[str, Any], finish: str | None = None) -> str:
    body = {"id": "scripted", "object": "chat.completion.chunk", "model": MODEL,
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}
    return f"data: {json.dumps(body)}\n\n"


def stream(reply: Reply, turn: int) -> str:
    prompt = 100 * (turn + 1)
    usage = {"id": "scripted", "object": "chat.completion.chunk", "model": MODEL, "choices": [],
             "usage": {"prompt_tokens": prompt, "completion_tokens": 10, "total_tokens": prompt + 10}}
    if isinstance(reply, Say):
        parts = [chunk({"role": "assistant", "content": reply.text}), chunk({}, "stop")]
    else:
        call = {"index": 0, "id": f"call_{turn}", "type": "function",
                "function": {"name": reply.name, "arguments": json.dumps(reply.arguments)}}
        parts = [chunk({"role": "assistant", "tool_calls": [call]}), chunk({}, "tool_calls")]
    return "".join([*parts, f"data: {json.dumps(usage)}\n\n", "data: [DONE]\n\n"])


class ScriptedModel:
    def __init__(self, replies: list[Reply], record: Path) -> None:
        self.replies = replies
        self.record = record
        self.turn = 0
        self.guard = threading.Lock()
        outer = self

        class Handler(BaseHTTPRequestHandler):
            @override
            def log_message(self, format: str, *args: object) -> None:
                return

            def do_POST(self) -> None:
                request = json.loads(self.rfile.read(int(self.headers["content-length"])))
                with outer.guard:
                    with outer.record.open("a") as file:
                        file.write(json.dumps(request) + "\n")
                    reply = outer.replies[min(outer.turn, len(outer.replies) - 1)]
                    turn = outer.turn
                    outer.turn += 1
                body = stream(reply, turn).encode()
                self.send_response(200)
                self.send_header("content-type", "text/event-stream")
                self.send_header("content-length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    @property
    def base_url(self) -> str:
        return f"http://127.0.0.1:{self.server.server_address[1]}/v1"

    def environment(self, agent_directory: Path) -> dict[str, str]:
        agent_directory.mkdir(parents=True, exist_ok=True)
        (agent_directory / "models.json").write_text(self.models_json())
        return {"PI_CODING_AGENT_DIR": str(agent_directory)}

    def models_json(self) -> str:
        provider = {
            "baseUrl": self.base_url, "api": "openai-completions", "apiKey": "scripted",
            "models": [{"id": MODEL, "reasoning": False, "contextWindow": 128000, "maxTokens": 8192,
                        "cost": {"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}}],
        }
        return json.dumps({"providers": {PROVIDER: provider}})

    def stop(self) -> None:
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()
