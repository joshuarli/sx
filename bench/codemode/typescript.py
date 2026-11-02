import re
from collections.abc import Mapping
from typing import Any

TOKEN = re.compile(r"\s*(?:([A-Za-z][A-Za-z0-9_.-]*)|(.))")
IDENTIFIER = re.compile(r"[A-Za-z_$][A-Za-z0-9_$]*")
KINDS = ("Timeout", "Unavailable", "RateLimited", "NotFound", "PermissionDenied", "Invalid", "Conflict", "Failed")


def type_name(name: str) -> str:
    return "".join(part[:1].upper() + part[1:] for part in re.split(r"[.-]", name))


def member(name: str) -> str:
    return re.sub(r"-([a-z0-9])", lambda m: m.group(1).upper(), name).removesuffix("?")


def key(name: str) -> str:
    return name if IDENTIFIER.fullmatch(name) else f'"{name}"'


class Converter:
    def __init__(self, text: str, aliases: Mapping[str, str]) -> None:
        self.tokens = [m.group(1) or m.group(2) for m in TOKEN.finditer(text) if m.group(1) or m.group(2)]
        self.at = 0
        self.aliases = aliases
        self.text = text

    def take(self, expected: str | None = None) -> str:
        if self.at >= len(self.tokens):
            raise ValueError(f"type {self.text!r} ends early")
        token = str(self.tokens[self.at])
        if expected is not None and token != expected:
            raise ValueError(f"expected {expected!r}, found {token!r} in {self.text!r}")
        self.at += 1
        return token

    def peek(self) -> str | None:
        return str(self.tokens[self.at]) if self.at < len(self.tokens) else None

    def arguments(self) -> list[str]:
        self.take("<")
        found = [self.type()]
        while self.peek() == ",":
            self.take(",")
            found.append(self.type())
        self.take(">")
        return found

    def type(self) -> str:
        name = self.take()
        match name:
            case "Bool":
                return "boolean"
            case "Int" | "Float":
                return "number"
            case "String" | "Bytes":
                return "string"
            case "Unit":
                return "null"
            case "List":
                (element,) = self.arguments()
                return f"({element})[]" if "|" in element else f"{element}[]"
            case "Option":
                (inner,) = self.arguments()
                return f"{inner} | null"
            case "Map":
                _, value = self.arguments()
                return f"Record<string, {value}>"
            case "Tuple":
                return "[" + ", ".join(self.arguments()) + "]"
            case "Record":
                if self.peek() == "open":
                    self.take("open")
                self.take("{")
                fields = []
                while self.peek() != "}":
                    field = self.take()
                    self.take(":")
                    fields.append(f"{key(field)}: {self.type()}")
                    if self.peek() == ",":
                        self.take(",")
                self.take("}")
                return "{ " + "; ".join(fields) + " }" if fields else "Record<string, never>"
        if name in self.aliases:
            return type_name(name)
        raise ValueError(f"unknown type {name!r} in {self.text!r}")


def convert(text: str, aliases: Mapping[str, str]) -> str:
    converter = Converter(text, aliases)
    result = converter.type()
    if converter.at != len(converter.tokens):
        raise ValueError(f"unexpected {converter.tokens[converter.at]!r} in {text!r}")
    return result


def declarations(task: Mapping[str, Any], cores: Mapping[str, Any]) -> str:
    core_ids = set(task.get("cores", []))
    core_types = {name: d for name, d in cores["types"].items() if name.split(".")[0] in core_ids}
    aliases: dict[str, str] = {str(k): str(v) for k, v in {**task.get("types", {}), **core_types}.items()}
    aliases.update(dict.fromkeys(task.get("enums", {}), ""))
    lines: list[str] = []
    for name, variants in task.get("enums", {}).items():
        shapes = [
            f'"{v}"' if p is None else f'{{ tag: "{v}"; value: {convert(p, aliases)} }}' for v, p in variants.items()
        ]
        lines.append(f"type {type_name(name)} = {' | '.join(shapes)};")
    for name, definition in {**task.get("types", {}), **core_types}.items():
        lines.append(f"type {type_name(name)} = {convert(definition, aliases)};")
    kinds = " | ".join(f'"{k}"' for k in KINDS)
    lines += ["", f"interface CallError extends Error {{ kind: {kinds}; message: string }}", "", "interface Api {"]
    namespaces: dict[str, list[str]] = {}
    for capability in task["capabilities"]:
        namespace, name = capability["id"].split(".", 1)
        input_type = convert(capability["input"], aliases)
        empty = capability["input"].strip() == "Record {}"
        parameter = "input?: Record<string, never>" if empty else f"input: {input_type}"
        effect = capability["effect"]
        if effect == "write":
            effect += ", idempotent" if capability["idempotent"] else ", not idempotent"
        namespaces.setdefault(namespace, []).extend([
            f"    /** {capability['description']} [{effect}] */",
            f"    {member(name)}({parameter}): Promise<{convert(capability['output'], aliases)}>;",
        ])
    for function in cores["functions"]:
        namespace, name = function["name"].split(".", 1)
        if namespace not in core_ids:
            continue
        params = ", ".join(f"{chr(97 + i)}: {convert(p, aliases)}" for i, p in enumerate(function["params"]))
        namespaces.setdefault(namespace, []).extend([
            f"    /** {function['description'].replace(' none', ' null')} */",
            f"    {member(name)}({params}): {convert(function['output'], aliases)};",
        ])
    for namespace, members in namespaces.items():
        lines += [f"  {member(namespace)}: {{", *members, "  };"]
    lines += ["  /** Waits. Time is virtual, so waiting costs nothing. */", "  sleep(ms: number): Promise<void>;", "}"]
    return "\n".join(lines) + "\n"
