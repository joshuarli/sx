from __future__ import annotations

import argparse
import sys
from collections.abc import Sequence
from typing import TYPE_CHECKING, Any

from sx import cli as sx_cli
from sx_repomod.declarations import DISCOVER, STATUS_OFFLINE, Program, new_registry

if TYPE_CHECKING:
    from sx_repomod.capabilities import Environment

EXIT_FAILED = 2
SX_COMMANDS = ("help", "cores", "check", "explain", "run")


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(
        prog="sx-repomod",
        description="Run repomod's discover and status --offline as Sx programs. "
        f"{', '.join(SX_COMMANDS)} are the sx commands, with this host's registry.",
    )
    commands = root.add_subparsers(dest="command", required=True)
    discover = commands.add_parser("discover", help="compare GitHub owner repositories against catalogued entries")
    discover.add_argument("--owner", action="append", default=[])
    discover.add_argument("--json", action="store_true")
    status = commands.add_parser("status", help="show failures and lint findings from cached data")
    status.add_argument("--offline", action="store_true", required=True)
    status.add_argument("--repo", action="append", default=[])
    status.add_argument("--tag", action="append", default=[])
    status.add_argument("--dirty", action="store_true")
    status.add_argument("--failed", action="store_true", help="only error-level failures")
    status.add_argument("--refresh", action="store_true")
    status.add_argument("--json", action="store_true")
    return root


def execute(program: Program, inputs: dict[str, object], environment: Environment | None) -> Any:
    from sx_repomod.capabilities import Capabilities
    from sx_repomod.runner import Errored, RejectedProgramError, Succeeded, run_program

    try:
        outcome = run_program(program, inputs, Capabilities(environment))
    except RejectedProgramError as rejected:
        print(f"sx-repomod: {program.name}.sx was rejected:\n{rejected}", file=sys.stderr)
        return None
    if isinstance(outcome, Errored):
        print(f"repomod: {outcome.message}", file=sys.stderr)
        return None
    assert isinstance(outcome, Succeeded)
    return outcome.result


def main(argv: Sequence[str] | None = None, environment: Environment | None = None) -> int:
    arguments = list(sys.argv[1:] if argv is None else argv)
    if arguments and arguments[0] in SX_COMMANDS:
        return sx_cli.main(arguments, new_registry)
    args = parser().parse_args(arguments)
    from sx_repomod import render

    if args.command == "discover":
        result = execute(DISCOVER, {"owners": args.owner}, environment)
        if result is None:
            return EXIT_FAILED
        (render.discover_json if args.json else render.discover_text)(result)
        return 0
    inputs: dict[str, object] = {
        "repos": args.repo,
        "tags": args.tag,
        "only-dirty": args.dirty,
        "only-failed": args.failed,
        "refresh": args.refresh,
    }
    rows = execute(STATUS_OFFLINE, inputs, environment)
    if rows is None:
        return EXIT_FAILED
    (render.status_json if args.json else render.status_text)(rows)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
