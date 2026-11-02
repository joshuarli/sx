import json
import time
from collections.abc import Callable, Sequence
from dataclasses import dataclass
from types import SimpleNamespace
from typing import Any

import pytest
import repomod.cli
from fleet import NOW, Fleet
from repomod import github
from repomod.cmd import status as status_command
from repomod.types import Catalog
from sx_repomod.capabilities import Environment
from sx_repomod.cli import main as sx_main


@dataclass(frozen=True, slots=True)
class Run:
    code: int
    out: str
    err: str

    @property
    def json(self) -> Any:
        return json.loads(self.out)


def freeze_python_clock(monkeypatch: pytest.MonkeyPatch, now: int = NOW) -> None:
    frozen = SimpleNamespace(time=lambda: float(now), sleep=time.sleep)
    monkeypatch.setattr(status_command, "time", frozen)
    monkeypatch.setattr(github, "time", frozen)


def run_python(
    argv: Sequence[str], catalog: Catalog, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> Run:
    monkeypatch.setattr("repomod.cmd.common.default_catalog", lambda: catalog)
    freeze_python_clock(monkeypatch)
    code = repomod.cli.main(list(argv))
    captured = capsys.readouterr()
    return Run(code, captured.out, captured.err)


def run_sx(
    argv: Sequence[str],
    catalog: Catalog,
    capsys: pytest.CaptureFixture[str],
    load_catalog: Callable[[], Catalog] | None = None,
) -> Run:
    environment = Environment(load_catalog or (lambda: catalog), lambda: float(NOW))
    code = sx_main(list(argv), environment)
    captured = capsys.readouterr()
    return Run(code, captured.out, captured.err)


def assert_same(
    argv: Sequence[str], fleet: Fleet, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> Run:
    python = run_python(argv, fleet.catalog, monkeypatch, capsys)
    sx = run_sx(argv, fleet.catalog, capsys)
    assert sx.code == python.code, (argv, sx.err, python.err)
    assert sx.err == python.err, argv
    if "--json" in argv and python.code == 0:
        assert sx.json == python.json, argv
    else:
        assert sx.out == python.out, argv
    return python
