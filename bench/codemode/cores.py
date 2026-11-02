import json
import re
from collections.abc import Callable, Mapping
from dataclasses import replace
from pathlib import Path
from typing import Any

from sx.registry import Core, Registry, load_declarations, signatures_core
from sx.values import INT_MAX, INT_MIN, NONE_VALUE, ListValue, RecordValue, SomeValue, Value

SIGNATURES = json.loads((Path(__file__).parent / "cores.json").read_text())
VERSION = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")
DECIMAL = re.compile(r"-?[0-9]+")


def strings(value: Value) -> list[str]:
    assert isinstance(value, ListValue)
    return [item for item in value.items if isinstance(item, str)]


def version_parts(value: Value) -> tuple[int, int, int]:
    assert isinstance(value, RecordValue)
    major, minor, patch = value.get("major"), value.get("minor"), value.get("patch")
    assert isinstance(major, int)
    assert isinstance(minor, int)
    assert isinstance(patch, int)
    return major, minor, patch


def parse_version(text: Value) -> Value:
    assert isinstance(text, str)
    found = VERSION.fullmatch(text)
    if found is None:
        return NONE_VALUE
    major, minor, patch = (int(part) for part in found.groups())
    return SomeValue(RecordValue((("major", major), ("minor", minor), ("patch", patch))))


def to_int(text: Value) -> Value:
    assert isinstance(text, str)
    if DECIMAL.fullmatch(text) is None or not INT_MIN <= int(text) <= INT_MAX:
        return NONE_VALUE
    return SomeValue(int(text))


def compare_versions(a: Value, b: Value) -> Value:
    x, y = version_parts(a), version_parts(b)
    return (x > y) - (x < y)


IMPLEMENTATIONS: Mapping[str, Callable[..., Value]] = {
    "text.casefold": lambda s: s.casefold(),
    "text.split": lambda s, separator: ListValue(tuple(s.split(separator))),
    "text.join": lambda separator, parts: separator.join(strings(parts)),
    "text.chars": lambda s: ListValue(tuple(s)),
    "text.trim": lambda s: s.strip(),
    "text.length": lambda s: len(s),
    "text.starts-with?": lambda s, prefix: s.startswith(prefix),
    "text.ends-with?": lambda s, suffix: s.endswith(suffix),
    "text.contains?": lambda s, part: part in s,
    "text.replace": lambda s, old, new: s.replace(old, new),
    "text.from-int": lambda n: str(n),
    "text.to-int": to_int,
    "semver.parse": parse_version,
    "semver.compare": compare_versions,
    "semver.show": lambda v: "{}.{}.{}".format(*version_parts(v)),
}


def core(core_id: str, registry: Registry) -> Core:
    signatures = signatures_core(core_id, SIGNATURES["functions"], SIGNATURES["types"], registry)
    functions = tuple(replace(f, implementation=IMPLEMENTATIONS[f.name]) for f in signatures.functions)
    return replace(signatures, functions=functions)


def task_registry(task: Mapping[str, Any]) -> Registry:
    registry = Registry()
    load_declarations(registry, task)
    for core_id in task.get("cores", []):
        registry.add_core(core(core_id, registry))
    return registry
