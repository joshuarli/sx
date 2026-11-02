import importlib.util
import json
from pathlib import Path
from types import ModuleType
from typing import cast

import pytest
from sx_git import CORE as GIT
from sx_github import CORE as GITHUB
from sx_semver import CORE as SEMVER
from sx_text import CORE as TEXT

from sx.registry import Core

ROOT = Path(__file__).parent.parent
SCRIPTS = ROOT / "rust" / "sx-cores" / "scripts"
VECTORS = ROOT / "fixtures" / "cores"
TABLES = ROOT / "rust" / "sx-cores" / "src" / "tables.rs"
CORES = {"text": TEXT, "semver": SEMVER, "git": GIT, "github": GITHUB}
CI_STATES = {"Passed", "Failed", "Pending", "Cancelled", "Skipped", "Neutral", "NoChecks", "Unknown", "Stale"}


def load_script(name: str) -> ModuleType:
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / f"{name}.py")
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def vectors_of(core: Core) -> list[dict[str, object]]:
    document = json.loads((VECTORS / f"{core.id}.json").read_text(encoding="utf-8"))
    assert document["core"] == core.id
    assert document["version"] == core.version
    vectors = document["vectors"]
    assert isinstance(vectors, list)
    return cast("list[dict[str, object]]", vectors)


@pytest.mark.parametrize("core", CORES)
def test_committed_vectors_match_the_python_core(core: str) -> None:
    rendered = load_script("core_vectors").render()[core]
    if (VECTORS / f"{core}.json").read_text(encoding="utf-8") != rendered:
        pytest.fail(f"fixtures/cores/{core}.json is stale: run `uv run python rust/sx-cores/scripts/core_vectors.py`")


def test_committed_casefold_table_matches_the_running_python() -> None:
    rendered = load_script("casefold_table").render()
    if TABLES.read_text(encoding="utf-8") != rendered:
        pytest.fail("rust/sx-cores/src/tables.rs is stale: run `uv run python rust/sx-cores/scripts/casefold_table.py`")


@pytest.mark.parametrize("core", CORES.values(), ids=list(CORES))
def test_every_function_has_vectors(core: Core) -> None:
    named = {vector["function"] for vector in vectors_of(core)}
    assert named == {function.name for function in core.functions}


def test_vectors_reach_every_ci_state_and_the_one_failure() -> None:
    reached = {vector["result"] for vector in vectors_of(GITHUB) if vector["function"] == "github.ci-state"}
    assert reached == CI_STATES
    failures = [vector for vector in vectors_of(TEXT) if "fails" in vector]
    assert failures
    assert {vector["function"] for vector in failures} == {"text.split"}
