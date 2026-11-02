from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

Json = Any

PROGRAM = "program.ts"
DECLARATIONS = "declarations.json"
RESPONSES = "responses.json"
TOOLS = "tools.json"
META = "case.json"


@dataclass(frozen=True, slots=True)
class Case:
    name: str
    program: str
    declarations: Json
    responses: list[Json]
    tools: list[dict[str, str]]
    allow: tuple[str, ...] = ()
    grants: Json = None
    expect_trap: str | None = None
    meta: dict[str, Json] = field(default_factory=dict)

    def write(self, directory: Path) -> Path:
        directory.mkdir(parents=True, exist_ok=True)
        (directory / PROGRAM).write_text(self.program)
        (directory / DECLARATIONS).write_text(json.dumps(self.declarations, indent=1))
        (directory / RESPONSES).write_text(json.dumps(self.responses))
        (directory / TOOLS).write_text(json.dumps(self.tools))
        about = {"name": self.name, "allow": list(self.allow), "expect_trap": self.expect_trap, **self.meta}
        if self.grants is not None:
            (directory / "grants.json").write_text(json.dumps(self.grants))
            about["grants"] = "grants.json"
        (directory / META).write_text(json.dumps(about, indent=1))
        return directory


@dataclass(frozen=True, slots=True)
class CaseFiles:
    directory: Path
    name: str
    allow: tuple[str, ...]
    grants: Path | None
    expect_trap: str | None
    meta: dict[str, Json]

    @property
    def program(self) -> Path:
        return self.directory / PROGRAM

    @property
    def declarations(self) -> Path:
        return self.directory / DECLARATIONS

    @property
    def responses(self) -> Path:
        return self.directory / RESPONSES

    @property
    def tools(self) -> Path:
        return self.directory / TOOLS

    @classmethod
    def read(cls, directory: Path) -> CaseFiles:
        about = json.loads((directory / META).read_text())
        grants = directory / "grants.json" if about.get("grants") else None
        allow = tuple(about.get("allow", []))
        return cls(directory, str(about["name"]), allow, grants, about.get("expect_trap"), about)
