from collections.abc import Mapping
from typing import Any

from sx_semver import CORE as SEMVER
from sx_text import CORE as TEXT

from sx.registry import Core, Registry, load_declarations
from sx.types import show_definition, show_type

CORES: Mapping[str, Core] = {core.id: core for core in (TEXT, SEMVER)}

SIGNATURES: dict[str, Any] = {
    "types": {name: show_definition(t) for core in CORES.values() for name, t in core.types.items()},
    "functions": [
        {
            "name": function.name,
            "params": [show_type(param) for param in function.params],
            "output": show_type(function.output),
            "description": function.description,
            "example": [function.example.program, function.example.result] if function.example else None,
        }
        for core in CORES.values()
        for function in core.functions
    ],
}


def task_registry(task: Mapping[str, Any]) -> Registry:
    registry = Registry()
    load_declarations(registry, task)
    for core_id in task.get("cores", []):
        registry.add_core(CORES[core_id])
    return registry
