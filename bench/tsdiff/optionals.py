from __future__ import annotations

from collections.abc import Mapping

from .case import Json

OPENERS = {"(": ")", "<": ">", "{": "}", "[": "]"}


def split_top(text: str) -> list[str]:
    parts: list[str] = []
    depth = 0
    start = 0
    for at, char in enumerate(text):
        if char in OPENERS:
            depth += 1
        elif char in OPENERS.values():
            depth -= 1
        elif char == "," and depth == 0:
            parts.append(text[start:at].strip())
            start = at + 1
    tail = text[start:].strip()
    if tail:
        parts.append(tail)
    return parts


def resolve(type_string: str, types: Mapping[str, str]) -> str:
    text = type_string.strip()
    seen = 0
    while text in types and seen < 20:
        text = types[text].strip()
        seen += 1
    return text


def record_fields(type_string: str, types: Mapping[str, str]) -> dict[str, str]:
    text = resolve(type_string, types)
    if not text.startswith("Record"):
        return {}
    body = text[text.index("{") + 1 : text.rindex("}")]
    fields: dict[str, str] = {}
    for part in split_top(body):
        name, _, kind = part.partition(":")
        fields[name.strip()] = kind.strip()
    return fields


def with_optional_nulls(value: Json, input_type: str, types: Mapping[str, str]) -> Json:
    fields = record_fields(input_type, types)
    if not isinstance(value, dict) or not fields:
        return value
    filled = dict(value)
    for name, kind in fields.items():
        if name not in filled and resolve(kind, types).startswith("Option<"):
            filled[name] = None
    return filled
