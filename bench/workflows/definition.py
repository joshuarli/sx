import importlib.util
import json
import re
from collections.abc import Callable, Generator
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Literal

from sandbox import Sandbox

HERE = Path(__file__).resolve().parent
WORKFLOWS = HERE / "workflows"
PLACEHOLDER = re.compile(r"\{\{([a-z][a-z0-9_]*)\}\}")
REFERENCES: dict[str, str] = {"sx": "reference.sx", "js": "reference.mts"}
type Json = dict[str, Any]


@dataclass(frozen=True, slots=True)
class Verdict:
    status: Literal["correct", "wrong"]
    detail: str = ""

    @staticmethod
    def correct() -> Verdict:
        return Verdict("correct")

    @staticmethod
    def wrong(detail: str) -> Verdict:
        return Verdict("wrong", detail)


type Setup = Callable[[Sandbox], None]
type Check = Callable[[Sandbox, object], Verdict]


@dataclass(frozen=True, slots=True)
class ToolServer:
    name: str
    command: list[str]


@dataclass(frozen=True, slots=True)
class Workflow:
    id: str
    directory: Path
    task: str
    declarations: str
    grants: str
    setup: Setup
    check: Check
    server: str | None = None

    def render(self, template: str, sandbox: Sandbox) -> str:
        def substitute(found: re.Match[str]) -> str:
            name = found.group(1)
            if name not in sandbox.variables:
                raise KeyError(f"{self.id}: the placeholder {{{{{name}}}}} names no sandbox variable")
            return sandbox.variables[name]

        return PLACEHOLDER.sub(substitute, template)

    def reference(self, arm: str) -> str:
        return (self.directory / REFERENCES[arm]).read_text()

    @contextmanager
    def sandbox(self) -> Generator[Sandbox]:
        with Sandbox() as sandbox:
            self.setup(sandbox)
            yield sandbox

    def tool_server(self, sandbox: Sandbox) -> ToolServer | None:
        if self.server is None:
            return None
        loaded: Json = json.loads(self.render(self.server, sandbox))
        return ToolServer(loaded["name"], list(loaded["command"]))

    def declarations_json(self, sandbox: Sandbox) -> Json:
        loaded: Json = json.loads(self.render(self.declarations, sandbox))
        return loaded


def load(workflow_id: str, root: Path = WORKFLOWS) -> Workflow:
    directory = root / workflow_id
    if not (directory / "workflow.py").is_file():
        raise FileNotFoundError(f"no workflow {workflow_id!r}: {directory / 'workflow.py'} does not exist")
    module_name = f"workflow_{workflow_id.replace('-', '_')}"
    spec = importlib.util.spec_from_file_location(module_name, directory / "workflow.py")
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {directory / 'workflow.py'}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    server = directory / "server.json"
    return Workflow(
        id=workflow_id,
        directory=directory,
        task=(directory / "task.md").read_text(),
        declarations=(directory / "declarations.json").read_text(),
        grants=(directory / "grants.json").read_text(),
        setup=module.setup,
        check=module.check,
        server=server.read_text() if server.is_file() else None,
    )


def available(root: Path = WORKFLOWS) -> list[str]:
    return sorted(path.parent.name for path in root.glob("*/workflow.py"))
