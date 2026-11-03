import importlib.util
import json
import re
from collections.abc import Callable, Generator
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Literal

from records import Effect
from sandbox import Sandbox

HERE = Path(__file__).resolve().parent
WORKFLOWS = HERE / "workflows"
SETS: dict[str, Path] = {
    "dev": WORKFLOWS, "blind": HERE / "blind", "blind2": HERE / "blind2", "effects": HERE / "effects",
    "effects-blind": HERE / "effects-blind", "effects-blind2": HERE / "effects-blind2",
}
EFFECTS_SET = "effects"
EFFECTS_SETS = (EFFECTS_SET, "effects-blind", "effects-blind2")
PLACEHOLDER = re.compile(r"\{\{([a-z][a-z0-9_]*)\}\}")
REFERENCES: dict[str, str] = {"sx": "reference.sx", "js": "reference.mts"}
EFFECT_REFERENCES: dict[str, str] = {"sx": "reference.sx", "js": "reference.js"}
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


@dataclass(frozen=True, slots=True)
class Evidence:
    program: str
    transcript: str


class AuditError(ValueError):
    pass


@dataclass(frozen=True, slots=True)
class Violation:
    detail: str
    effect: int


@dataclass(frozen=True, slots=True)
class Audit:
    verdict: Verdict
    violations: list[Violation]


type Setup = Callable[[Sandbox], None]
type Check = Callable[[Sandbox, object, Evidence], Verdict]
type Auditor = Callable[[Sandbox, list[Effect]], Audit]


@dataclass(frozen=True, slots=True)
class ToolServer:
    name: str
    command: list[str]


@dataclass(frozen=True, slots=True)
class Spec:
    id: str
    directory: Path
    task: str
    declarations: str
    grants: str
    setup: Setup
    server: str | None

    def render(self, template: str, sandbox: Sandbox) -> str:
        def substitute(found: re.Match[str]) -> str:
            name = found.group(1)
            if name not in sandbox.variables:
                raise KeyError(f"{self.id}: the placeholder {{{{{name}}}}} names no sandbox variable")
            return sandbox.variables[name]

        return PLACEHOLDER.sub(substitute, template)

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


@dataclass(frozen=True, slots=True)
class Workflow(Spec):
    check: Check

    def reference(self, arm: str) -> str:
        return (self.directory / REFERENCES[arm]).read_text()


@dataclass(frozen=True, slots=True)
class EffectsWorkflow(Spec):
    audit: Auditor
    policy: str

    def reference(self, arm: str) -> str:
        return (self.directory / EFFECT_REFERENCES[arm]).read_text()

    def policy_json(self, sandbox: Sandbox) -> Json:
        loaded: Json = json.loads(self.render(self.policy, sandbox))
        return loaded


def module_of(directory: Path, workflow_id: str) -> Any:
    if not (directory / "workflow.py").is_file():
        raise FileNotFoundError(f"no workflow {workflow_id!r}: {directory / 'workflow.py'} does not exist")
    module_name = f"workflow_{workflow_id.replace('-', '_')}"
    spec = importlib.util.spec_from_file_location(module_name, directory / "workflow.py")
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {directory / 'workflow.py'}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def spec_fields(directory: Path, workflow_id: str, module: Any) -> Json:
    server = directory / "server.json"
    return {
        "id": workflow_id,
        "directory": directory,
        "task": (directory / "task.md").read_text(),
        "declarations": (directory / "declarations.json").read_text(),
        "grants": (directory / "grants.json").read_text(),
        "setup": module.setup,
        "server": server.read_text() if server.is_file() else None,
    }


def load(workflow_id: str, root: Path = WORKFLOWS) -> Workflow:
    directory = root / workflow_id
    module = module_of(directory, workflow_id)
    return Workflow(**spec_fields(directory, workflow_id, module), check=module.check)


def load_effects(workflow_id: str, root: Path = SETS[EFFECTS_SET]) -> EffectsWorkflow:
    directory = root / workflow_id
    module = module_of(directory, workflow_id)
    return EffectsWorkflow(
        **spec_fields(directory, workflow_id, module), audit=module.audit,
        policy=(directory / "policy.json").read_text(),
    )


def available(root: Path = WORKFLOWS) -> list[str]:
    return sorted(path.parent.name for path in root.glob("*/workflow.py"))
