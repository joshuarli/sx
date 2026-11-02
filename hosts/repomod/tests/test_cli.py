import json
import subprocess
import sys
from pathlib import Path

import pytest
from sx_git import CORE as GIT
from sx_github import CORE as GITHUB
from sx_repomod.cli import main
from sx_repomod.declarations import capability_declarations
from sx_text import CORE as TEXT

from sx import cli as frozen_cli
from sx.registry import Registry, load_declarations


def frozen_registry() -> Registry:
    registry = Registry()
    for core in (TEXT, GIT, GITHUB):
        registry.add_core(core)
    load_declarations(registry, {"capabilities": capability_declarations()})
    return registry


def both(argv: list[str], capsys: pytest.CaptureFixture[str]) -> tuple[tuple[int, str, str], tuple[int, str, str]]:
    frozen = frozen_cli.main(argv, frozen_registry)
    captured = capsys.readouterr()
    frozen_result = (frozen, captured.out, captured.err)
    host = main(argv)
    captured = capsys.readouterr()
    return frozen_result, (host, captured.out, captured.err)


def test_the_host_registry_answers_for_its_cores_and_capabilities(capsys: pytest.CaptureFixture[str]) -> None:
    assert main(["cores"]) == 0
    assert capsys.readouterr().out.splitlines() == [
        "text 0.1.0  unverified, conformance-tested",
        "git 0.1.0  unverified, conformance-tested",
        "github 0.1.0  unverified, conformance-tested",
    ]
    assert main(["help", "github"]) == 0
    assert "specified: none" in capsys.readouterr().out
    assert main(["help", "git.status"]) == 0
    assert "git.Checkout = Missing | (Invalid String)" in capsys.readouterr().out


@pytest.mark.parametrize("argv", [["cores"], ["cores", "--format", "json"]])
def test_cores_match_the_frozen_python_command(argv: list[str], capsys: pytest.CaptureFixture[str]) -> None:
    frozen, host = both(argv, capsys)
    assert host == frozen
    assert host[0] == 0


@pytest.mark.parametrize(
    "argv",
    [
        ["help"],
        ["help", "github"],
        ["help", "git.status"],
        ["help", "text.casefold"],
        ["help", "nope"],
        ["help", "retry"],
    ],
)
def test_help_matches_the_frozen_python_command(argv: list[str], capsys: pytest.CaptureFixture[str]) -> None:
    frozen, host = both(argv, capsys)
    assert host == frozen


def test_programs_check_explain_and_run_like_the_frozen_python_command(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    program = tmp_path / "age.sx"
    program.write_text('(git.age-days (call clock.now {}) (some (+ n 1)))\n')
    declarations = tmp_path / "declarations.json"
    declarations.write_text(json.dumps({"inputs": {"n": "Int"}}))
    responses = tmp_path / "responses.json"
    responses.write_text(json.dumps([{"capability": "clock.now", "input": {}, "results": [{"output": 3_000_000}]}]))
    shared = [str(program), "--declarations", str(declarations), "--allow", "clock.now"]
    runs = [
        ["check", *shared],
        ["check", *shared, "--format", "json"],
        ["check", *shared, "--emit", "ir"],
        ["explain", *shared],
        ["explain", *shared, "--format", "json"],
        ["run", *shared, "--responses", str(responses), "--input", "n=5"],
        ["run", *shared, "--responses", str(responses), "--input", "n=5", "--format", "json"],
        ["run", *shared, "--input", "n=5"],
        ["check", str(program), "--allow", "clock.now"],
        ["check", str(program), f"--declarations={declarations}", "--allow", "clock.now"],
        ["check", str(program), "--decl", str(declarations)],
    ]
    for argv in runs:
        frozen, host = both(argv, capsys)
        assert host == frozen, argv


def test_declaration_errors_name_the_file_the_user_gave(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    program = tmp_path / "one.sx"
    program.write_text("1\n")
    invalid = tmp_path / "invalid.json"
    invalid.write_text(json.dumps({"types": {"Bad": "Nope<Int>"}}))
    duplicate = tmp_path / "duplicate.json"
    duplicate.write_text(json.dumps({"capabilities": capability_declarations()[:1]}))
    unreadable = tmp_path / "unreadable.json"
    unreadable.write_text("{")
    for declared in (invalid, duplicate, unreadable, tmp_path / "missing.json"):
        frozen, host = both(["check", str(program), "--declarations", str(declared)], capsys)
        assert host == frozen, declared
        assert host[0] != 0


def test_help_and_cores_work_where_repomod_cannot_be_imported() -> None:
    script = (
        "import sys; from sx_repomod.cli import main; code = main(['cores']); "
        "sys.exit(code or bool({'repomod', 'sx'} & set(sys.modules)))"
    )
    done = subprocess.run([sys.executable, "-c", script], capture_output=True, text=True, check=False)
    assert done.returncode == 0, done.stderr
    assert "github 0.1.0" in done.stdout
