import shutil
import subprocess
import tempfile
import threading
import time
from collections.abc import Callable, Mapping
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from types import TracebackType
from typing import Self, override

ROOT = Path(__file__).resolve().parent.parent.parent
SCRATCH = ROOT / ".work" / "tmp"
GIT_IDENTITY = {
    "GIT_AUTHOR_NAME": "Sandbox",
    "GIT_AUTHOR_EMAIL": "sandbox@example.invalid",
    "GIT_COMMITTER_NAME": "Sandbox",
    "GIT_COMMITTER_EMAIL": "sandbox@example.invalid",
    "GIT_AUTHOR_DATE": "2026-01-01T00:00:00+00:00",
    "GIT_COMMITTER_DATE": "2026-01-01T00:00:00+00:00",
}
GIT_PATH = "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin"
GIT_CONFIG = ("-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main")


@dataclass(frozen=True, slots=True)
class Request:
    method: str
    path: str
    headers: Mapping[str, str]
    body: bytes
    received: float = 0.0


@dataclass(frozen=True, slots=True)
class Response:
    status: int = 200
    body: bytes = b""
    headers: Mapping[str, str] = field(default_factory=dict)

    @staticmethod
    def json(text: str, status: int = 200) -> Response:
        return Response(status, text.encode(), {"content-type": "application/json"})


type Handler = Callable[[Request], Response]


class Service:
    def __init__(self, handler: Handler, latency: float = 0.0) -> None:
        self.requests: list[Request] = []
        self.handler = handler
        self.latency = latency
        self.peak = 0
        self.active = 0
        self.guard = threading.Lock()
        outer = self

        class Dispatch(BaseHTTPRequestHandler):
            @override
            def log_message(self, format: str, *args: object) -> None:
                return

            def respond(self) -> None:
                length = int(self.headers.get("content-length") or 0)
                request = Request(
                    self.command, self.path, dict(self.headers.items()), self.rfile.read(length), time.monotonic()
                )
                with outer.guard:
                    outer.requests.append(request)
                    outer.active += 1
                    outer.peak = max(outer.peak, outer.active)
                try:
                    time.sleep(outer.latency)
                    response = outer.handler(request)
                finally:
                    with outer.guard:
                        outer.active -= 1
                self.send_response(response.status)
                for name, value in {"content-length": str(len(response.body)), **response.headers}.items():
                    self.send_header(name, value)
                self.end_headers()
                self.wfile.write(response.body)

        for verb in ("GET", "POST", "PUT", "PATCH", "DELETE"):
            setattr(Dispatch, f"do_{verb}", Dispatch.respond)

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Dispatch)
        self.server.daemon_threads = True
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    @property
    def origin(self) -> str:
        return f"http://127.0.0.1:{self.server.server_address[1]}"

    def stop(self) -> None:
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()


def run_git(path: Path, home: Path, *arguments: str) -> str:
    environment = {**GIT_IDENTITY, "PATH": GIT_PATH, "HOME": str(home)}
    completed = subprocess.run(
        ["git", *GIT_CONFIG, *arguments],
        cwd=path, env=environment, stdin=subprocess.DEVNULL, capture_output=True, text=True, check=True,
    )
    return completed.stdout


class Sandbox:
    def __init__(self, scratch: Path = SCRATCH) -> None:
        scratch.mkdir(parents=True, exist_ok=True)
        self.root = Path(tempfile.mkdtemp(prefix="workflow-", dir=scratch)).resolve()
        self.variables: dict[str, str] = {}
        self.services: dict[str, Service] = {}
        self.reset()

    def reset(self) -> None:
        for child in self.root.iterdir():
            if child.is_dir() and not child.is_symlink():
                shutil.rmtree(child)
            else:
                child.unlink()
        self.variables = {"root": str(self.root)}
        for name, service in self.services.items():
            service.requests.clear()
            service.peak = 0
            self.variables[f"{name}_origin"] = service.origin

    def define(self, name: str, value: str) -> None:
        if name in self.variables:
            raise ValueError(f"the sandbox variable {name!r} is already defined")
        self.variables[name] = value

    def directory(self, *parts: str) -> Path:
        path = self.root.joinpath(*parts)
        path.mkdir(parents=True, exist_ok=True)
        return path

    def write(self, relative: str, text: str) -> Path:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        return path

    def git_repository(self, relative: str, commits: list[Mapping[str, str]]) -> Path:
        path = self.directory(relative)
        self.git(path, "init", "--quiet")
        for number, files in enumerate(commits, start=1):
            for name, text in files.items():
                target = path / name
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(text)
            self.git(path, "add", "--all")
            self.git(path, "commit", "--quiet", "--message", f"commit {number}")
        return path

    def git(self, path: Path, *arguments: str) -> str:
        return run_git(path, self.root, *arguments)

    def serve(self, name: str, handler: Handler, latency: float = 0.0) -> Service:
        service = self.services.get(name)
        if service is None:
            service = self.services[name] = Service(handler, latency)
            self.define(f"{name}_origin", service.origin)
        service.handler = handler
        service.latency = latency
        return service

    def copy_tree(self, source: Path, relative: str) -> Path:
        target = self.root / relative
        shutil.copytree(source, target, symlinks=True)
        return target

    def close(self) -> None:
        for service in self.services.values():
            service.stop()
        shutil.rmtree(self.root)

    def __enter__(self) -> Self:
        return self

    def __exit__(
        self, kind: type[BaseException] | None, error: BaseException | None, trace: TracebackType | None
    ) -> None:
        self.close()
