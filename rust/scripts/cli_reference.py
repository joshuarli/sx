import contextlib
import io
import json
import os
import sys
from pathlib import Path

from sx_git import CORE as GIT
from sx_github import CORE as GITHUB
from sx_semver import CORE as SEMVER
from sx_text import CORE as TEXT

from sx.cli import main
from sx.registry import Core, Registry

CORES: dict[str, Core] = {core.id: core for core in (TEXT, SEMVER, GIT, GITHUB)}


def registry_with(core_ids: list[str]) -> Registry:
    registry = Registry()
    for core_id in core_ids:
        registry.add_core(CORES[core_id])
    return registry


def run_case(case: dict[str, object]) -> dict[str, object]:
    core_ids = [str(c) for c in case["cores"]]
    out, err = io.StringIO(), io.StringIO()
    code = 0
    try:
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = main([str(a) for a in case["argv"]], lambda: registry_with(core_ids))
    except SystemExit as exit_:
        code = exit_.code if isinstance(exit_.code, int) else 0
    except Exception as crash:
        return {"crash": type(crash).__name__}
    return {"stdout": out.getvalue(), "stderr": err.getvalue(), "code": code}


def main_script(manifest: Path, target: Path) -> None:
    os.environ.pop("COLUMNS", None)
    cases = json.loads(manifest.read_text())
    target.write_text(json.dumps([run_case(case) for case in cases]))


if __name__ == "__main__":
    main_script(Path(sys.argv[1]), Path(sys.argv[2]))
