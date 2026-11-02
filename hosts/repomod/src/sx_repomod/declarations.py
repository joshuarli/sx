from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from importlib.resources import files

from sx_git import CORE as GIT
from sx_github import CORE as GITHUB
from sx_text import CORE as TEXT

from sx.registry import Capability, Effect, Registry

FINDING = (
    "Record {repository: String, rule: String, file: Option<String>, message: String, severity: String, "
    "fixable: Bool, kind: String, line: Option<Int>, column: Option<Int>, code: Option<String>}"
)
REPOSITORY = "Record {id: String, owner: String}"

CAPABILITIES: tuple[tuple[str, str, str, str], ...] = (
    ("catalog.select", "Record {repos: List<String>, tags: List<String>}", f"List<{REPOSITORY}>",
     "Catalogued repositories matching every named repository and any named tag, in catalog order. "
     "Names that are not catalogued, are ambiguous, or are unknown tags fail."),
    ("catalog.owner", "Record {owner: String}", "String",
     "The owner name, after checking it against the catalog's identifier rules."),
    ("github.owner-repos", "Record {owner: String, owners: List<String>}", "List<String>",
     "Names of the active repositories GitHub lists for one owner that belong to any of the named owners."),
    ("clock.now", "Record {}", "Int", "The current Unix time in milliseconds."),
    ("git.status", "Record {repo: String}", "git.Checkout",
     "What the local checkout of a catalogued repository looks like, without fetching."),
    ("git.oldest-unpushed", "Record {repo: String}", "Option<Int>",
     "Unix time in seconds of the oldest commit not yet on the upstream branch."),
    ("git.branch-since", "Record {repo: String, branch: String}", "Option<Int>",
     "Unix time in seconds when the checkout last switched to the branch, from the reflog."),
    ("repo.todo-items", "Record {repo: String}", "Int", "Number of items in the checkout's TODO.md."),
    ("github.cached-ci", "Record {repo: String, refresh: Bool, now: Int}", "github.CiCache",
     "The cached CI result for a repository, aged against now in milliseconds, or why there is none."),
    ("github.cached-upstream", "Record {repo: String, now: Int}", "Option<github.Upstream>",
     "The cached comparison of a fork with its parent, aged against now in milliseconds."),
    ("policy.lint", "Record {repos: List<String>}", f"List<{FINDING}>",
     "Local policy findings for the named repositories, sorted as repomod sorts them."),
)


CORE_IDS = tuple(core.id for core in (TEXT, GIT, GITHUB))


def new_registry() -> Registry:
    registry = Registry()
    for core in (TEXT, GIT, GITHUB):
        registry.add_core(core)
    for capability_id, input_type, output_type, description in CAPABILITIES:
        registry.add_capability(
            Capability(
                id=capability_id,
                effect=Effect.READ,
                input=registry.parse(input_type),
                output=registry.parse(output_type),
                description=description,
            )
        )
    return registry


@dataclass(frozen=True, slots=True)
class Program:
    name: str
    inputs: Mapping[str, str]
    allow: frozenset[str]

    @property
    def source(self) -> str:
        return files("sx_repomod").joinpath("programs", f"{self.name}.sx").read_text()

    @property
    def declarations(self) -> dict[str, object]:
        return {
            "cores": list(CORE_IDS),
            "inputs": dict(self.inputs),
            "capabilities": [
                {"id": capability_id, "effect": "read", "input": input_type, "output": output_type, "description": text}
                for capability_id, input_type, output_type, text in CAPABILITIES
            ],
        }


DISCOVER = Program(
    "discover",
    {"owners": "List<String>"},
    frozenset({"catalog.select", "catalog.owner", "github.owner-repos"}),
)
STATUS_OFFLINE = Program(
    "status-offline",
    {
        "repos": "List<String>",
        "tags": "List<String>",
        "only-dirty": "Bool",
        "only-failed": "Bool",
        "refresh": "Bool",
    },
    frozenset(
        {
            "catalog.select",
            "clock.now",
            "policy.lint",
            "git.status",
            "git.oldest-unpushed",
            "git.branch-since",
            "repo.todo-items",
            "github.cached-ci",
            "github.cached-upstream",
        }
    ),
)
