from __future__ import annotations

import argparse
import json
import sys
from collections.abc import Mapping, Sequence
from dataclasses import fields, is_dataclass
from pathlib import Path
from typing import Any

from sx import help as sx_help
from sx import ir
from sx.checker import CheckResult, closest
from sx.diagnostics import Diagnostic, render
from sx.host import FixtureHost, UnexpectedCallError, run
from sx.machine import Completed, Failed
from sx.program import compile_source
from sx.registry import Effect, Registry, RegistryError, load_declarations
from sx.syntax import Span
from sx.types import Type, show_type
from sx.values import DecodeError, Value, decode, show_value

EXIT_OK = 0
EXIT_REJECTED = 1
EXIT_FAILED = 2
EXIT_USAGE = 64
RUNTIME = "computed at run time"


class UsageError(Exception):
    pass


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="sx", description="Check, explain, and run Sx programs.")
    commands = parser.add_subparsers(dest="command", required=True)
    help_command = commands.add_parser("help", help="Print the guide, or the entry for one name.")
    help_command.add_argument("name", nargs="?")
    help_command.add_argument("--declarations", type=Path, help="JSON declarations that add tools and types")
    help_command.add_argument("--level", choices=sx_help.GUIDE_LEVELS, default="standard", help="Guide detail")
    for name, summary in (
        ("check", "Check a program and print its diagnostics."),
        ("explain", "Print the tools, effects, inputs, and result type of a program."),
        ("run", "Run a program against recorded tool responses."),
    ):
        command = commands.add_parser(name, help=summary)
        command.add_argument("file", type=Path)
        command.add_argument("--declarations", type=Path, help="JSON declarations of tools, types, and inputs")
        command.add_argument("--allow", action="append", default=[], metavar="ID", help="Authorize one tool")
        command.add_argument("--format", choices=["text", "json"], default="text")
        if name == "run":
            command.add_argument("--responses", type=Path, help="JSON list of recorded tool responses")
            command.add_argument("--input", action="append", default=[], metavar="NAME=JSON", help="One input value")
    args = parser.parse_args(argv)
    try:
        if args.command == "help":
            return command_help(args)
        return command_program(args)
    except UsageError as error:
        print(f"sx: {error}", file=sys.stderr)
        return EXIT_USAGE


def load_registry(path: Path | None) -> tuple[Registry, dict[str, Any]]:
    registry = Registry()
    if path is None:
        return registry, {}
    try:
        declarations = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise UsageError(f"cannot read declarations {path}: {error}") from error
    if not isinstance(declarations, dict):
        raise UsageError(f"declarations {path} must be a JSON object")
    if declarations.get("cores"):
        raise UsageError("this CLI cannot load cores; register them through the Python API")
    try:
        load_declarations(registry, declarations)
    except (RegistryError, KeyError, ValueError) as error:
        raise UsageError(f"invalid declarations {path}: {error}") from error
    return registry, declarations


def command_help(args: argparse.Namespace) -> int:
    registry, _ = load_registry(args.declarations)
    if args.name is None:
        print(sx_help.guide(args.level), end="")
        return EXIT_OK
    found = sx_help.lookup(args.name, registry)
    if found is None:
        suggestion = closest(args.name, sx_help.names(registry))
        hint = f" Did you mean {suggestion}?" if suggestion is not None else ""
        print(f"sx help: nothing is named {args.name}.{hint}", file=sys.stderr)
        return EXIT_REJECTED
    print(found, end="")
    return EXIT_OK


def declared_inputs(registry: Registry, declarations: dict[str, Any]) -> dict[str, Type]:
    raw = declarations.get("inputs", {})
    if not isinstance(raw, dict):
        raise UsageError("declarations: inputs must map names to types")
    try:
        return {str(name): registry.parse(str(t)) for name, t in raw.items()}
    except RegistryError as error:
        raise UsageError(f"declarations: {error}") from error


def command_program(args: argparse.Namespace) -> int:
    registry, declarations = load_registry(args.declarations)
    try:
        source = args.file.read_text()
    except OSError as error:
        raise UsageError(f"cannot read {args.file}: {error}") from error
    allowed = frozenset(args.allow)
    unknown = sorted(a for a in allowed if registry.capability(a) is None)
    if unknown:
        raise UsageError(f"--allow names a tool the declarations do not define: {unknown[0]}")
    inputs = declared_inputs(registry, declarations)
    checking = frozenset(registry.capabilities) if args.command == "explain" else allowed
    checked = compile_source(source, registry, allowed=checking, inputs=inputs)
    if checked.program is None:
        report_diagnostics(args.format, source, checked)
        return EXIT_REJECTED
    program = checked.program
    if args.command == "check":
        emit(args.format, {"ok": True, "result": show_type(program.result)}, f"ok: {show_type(program.result)}")
        return EXIT_OK
    if args.command == "explain":
        calls = literal_calls(program.body)
        capabilities: list[dict[str, object]] = []
        lines = ["tools:"]
        for capability_id in sorted(program.capabilities):
            capability = registry.capability(capability_id)
            assert capability is not None
            effect = capability.effect.value
            if capability.effect is Effect.WRITE:
                effect += ", idempotent" if capability.idempotent else ", not idempotent"
            known = [i if i is not None else RUNTIME for c, i in calls if c == capability_id]
            status = "allowed" if capability_id in allowed else "not allowed"
            lines.append(f"  {capability_id}  [{effect}]  {status}")
            lines += [f"    input: {i if i == RUNTIME else json.dumps(i)}" for i in known]
            granted = capability_id in allowed
            capabilities.append({"id": capability_id, "effect": effect, "allowed": granted, "inputs": known})
        lines += [f"inputs: {', '.join(sorted(program.inputs)) or 'none'}", f"result: {show_type(program.result)}"]
        payload = {"tools": capabilities, "inputs": sorted(program.inputs), "result": show_type(program.result)}
        emit(args.format, payload, "\n".join(lines))
        return EXIT_OK
    values = input_values(args.input, inputs)
    responses = load_responses(args.responses)
    host = FixtureHost.from_responses(registry, responses)
    try:
        finished = run(program, host, values)
    except UnexpectedCallError as error:
        emit(args.format, {"outcome": "unexpected-call", "message": str(error)}, f"sx run: {error}")
        return EXIT_FAILED
    outcome = finished.outcome
    if isinstance(outcome, Completed):
        payload = {"outcome": "completed", "result": outcome.json, "trace": finished.machine.trace}
        emit(args.format, payload, show_value(outcome.value))
        return EXIT_OK
    assert isinstance(outcome, Failed)
    failure = outcome.failure
    payload = {
        "outcome": "failed",
        "failure": {"kind": failure.kind.value, "message": failure.message, "request": failure.request,
                    "span": span_json(failure.span)},
        "trace": finished.machine.trace,
    }
    where = f" at {failure.span}" if failure.span is not None else ""
    emit(args.format, payload, f"failed[{failure.kind.value}]{where}: {failure.message}")
    return EXIT_FAILED


def literal_calls(node: ir.Node) -> list[tuple[str, object | None]]:
    found: list[tuple[str, object | None]] = []
    stack: list[object] = [node]
    while stack:
        current = stack.pop()
        if isinstance(current, ir.Call):
            found.append((current.capability.id, literal(current.argument)))
        if is_dataclass(current) and not isinstance(current, type):
            stack.extend(reversed([getattr(current, f.name) for f in fields(current)]))
        elif isinstance(current, tuple):
            stack.extend(reversed(current))
    return found


def literal(node: ir.Node) -> object | None:
    match node:
        case ir.Literal(value=value) if isinstance(value, bool | int | str):
            return value
        case ir.MakeRecord(fields=parts):
            values = {name: literal(part) for name, part in parts}
            return None if any(v is None for v in values.values()) else values
        case ir.MakeList(items=items):
            listed = [literal(item) for item in items]
            return None if any(v is None for v in listed) else listed
        case _:
            return None


def input_values(pairs: Sequence[str], inputs: dict[str, Type]) -> dict[str, Value]:
    values: dict[str, Value] = {}
    for pair in pairs:
        name, separator, text = pair.partition("=")
        if not separator:
            raise UsageError(f"--input takes NAME=JSON, not {pair!r}")
        if name not in inputs:
            raise UsageError(f"--input {name} is not declared in the declarations' inputs")
        try:
            values[name] = decode(json.loads(text), inputs[name], f"${name}")
        except (json.JSONDecodeError, DecodeError) as error:
            raise UsageError(f"--input {name}: {error}") from error
    missing = sorted(set(inputs) - set(values))
    if missing:
        raise UsageError(f"the program needs a value for input {missing[0]}: --input {missing[0]}=JSON")
    return values


def load_responses(path: Path | None) -> list[dict[str, Any]]:
    if path is None:
        return []
    try:
        responses = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise UsageError(f"cannot read responses {path}: {error}") from error
    if not isinstance(responses, list):
        raise UsageError(f"responses {path} must be a JSON list")
    return responses


def span_json(span: Span | None) -> dict[str, object] | None:
    if span is None:
        return None
    return {
        "start": {"line": span.start.line, "column": span.start.column},
        "end": {"line": span.end.line, "column": span.end.column},
    }


def diagnostic_json(diagnostic: Diagnostic) -> dict[str, object]:
    return {
        "code": diagnostic.code,
        "construct": diagnostic.construct,
        "span": span_json(diagnostic.span),
        "message": diagnostic.message,
        "detail": diagnostic.detail,
        "fix": diagnostic.fix,
        "help": diagnostic.help,
    }


def report_diagnostics(output_format: str, source: str, checked: CheckResult) -> None:
    payload = {"ok": False, "diagnostics": [diagnostic_json(d) for d in checked.diagnostics]}
    emit(output_format, payload, render(source, checked.diagnostics).rstrip("\n"))


def emit(output_format: str, payload: Mapping[str, object], text: str) -> None:
    if output_format == "json":
        print(json.dumps(payload, indent=2))
    else:
        print(text)
