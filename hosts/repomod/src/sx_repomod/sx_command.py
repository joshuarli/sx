from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from sx_repomod.declarations import CORE_IDS, capability_declarations, host_declarations
from sx_repomod.kernel import locate

SX_COMMANDS = ("help", "cores", "check", "explain", "run")
DECLARATIONS_FLAG = "--declarations"


@dataclass(frozen=True, slots=True)
class Slot:
    index: int
    value: str
    inline_flag: str | None


def declarations_slot(arguments: Sequence[str]) -> Slot | None:
    found: Slot | None = None
    for index, argument in enumerate(arguments):
        name, equals, value = argument.partition("=")
        if len(name) < 3 or not DECLARATIONS_FLAG.startswith(name):
            continue
        if equals:
            found = Slot(index, value, name)
        elif index + 1 < len(arguments):
            found = Slot(index + 1, arguments[index + 1], None)
    return found


def merged_declarations(user: Mapping[str, Any]) -> dict[str, Any] | None:
    cores, capabilities = user.get("cores", []), user.get("capabilities", [])
    if not isinstance(cores, list) or not isinstance(capabilities, list):
        return None
    return {
        **user,
        "cores": [*CORE_IDS, *(core for core in cores if core not in CORE_IDS)],
        "capabilities": [*capability_declarations(), *capabilities],
    }


def with_host_declarations(arguments: Sequence[str], directory: Path) -> tuple[list[str], str | None]:
    target = directory / "declarations.json"
    slot = declarations_slot(arguments)
    if slot is None:
        target.write_text(json.dumps(host_declarations()))
        return [*arguments, DECLARATIONS_FLAG, str(target)], None
    try:
        user = json.loads(Path(slot.value).read_text())
    except (OSError, ValueError):
        return list(arguments), None
    merged = merged_declarations(user) if isinstance(user, dict) else None
    if merged is None:
        return list(arguments), None
    target.write_text(json.dumps(merged))
    replaced = list(arguments)
    replaced[slot.index] = str(target) if slot.inline_flag is None else f"{slot.inline_flag}={target}"
    return replaced, slot.value


def delegate(arguments: Sequence[str]) -> int:
    with tempfile.TemporaryDirectory(prefix="sx-repomod-") as scratch:
        rewritten, shown = with_host_declarations(arguments, Path(scratch))
        done = subprocess.run(
            [str(locate()), *rewritten],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            encoding="utf-8",
            check=False,
        )
        stderr = done.stderr if shown is None else done.stderr.replace(str(Path(scratch) / "declarations.json"), shown)
    sys.stdout.write(done.stdout)
    sys.stderr.write(stderr)
    return done.returncode


def assurance_of(help_text: str) -> tuple[str, str, dict[str, list[str]]]:
    lines = help_text.splitlines()
    core_id, version = lines[0].split("  ")[0].split(" ")
    assert lines[1] == "Assurance:", lines[1]
    assurance: dict[str, list[str]] = {}
    level: str | None = None
    for line in lines[2:]:
        if not line.startswith("  "):
            break
        if line.startswith("    "):
            assert level is not None
            assurance[level].append(line.strip())
            continue
        level, _, detail = line.strip().partition(":")
        if detail.strip() == "none":
            level = None
        else:
            assurance[level] = []
    return core_id, version, assurance


def list_cores(arguments: Sequence[str]) -> int:
    parser = argparse.ArgumentParser(prog="sx-repomod cores")
    parser.add_argument("--format", choices=("text", "json"), default="text")
    args = parser.parse_args(arguments)
    payload: dict[str, object] = {}
    lines: list[str] = []
    with tempfile.TemporaryDirectory(prefix="sx-repomod-") as scratch:
        declarations = Path(scratch) / "declarations.json"
        declarations.write_text(json.dumps(host_declarations()))
        for core in CORE_IDS:
            done = subprocess.run(
                [str(locate()), "help", DECLARATIONS_FLAG, str(declarations), core],
                stdin=subprocess.DEVNULL,
                capture_output=True,
                text=True,
                encoding="utf-8",
                check=False,
            )
            if done.returncode != 0:
                sys.stderr.write(done.stderr)
                return done.returncode
            core_id, version, assurance = assurance_of(done.stdout)
            payload[core_id] = {"version": version, "assurance": assurance}
            lines.append(f"{core_id} {version}  {', '.join(assurance)}")
    print(json.dumps(payload, indent=2) if args.format == "json" else "\n".join(lines))
    return 0
