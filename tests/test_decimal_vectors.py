import importlib.util
import json
from pathlib import Path
from types import ModuleType

import pytest

ROOT = Path(__file__).parent.parent
SCRIPT = ROOT / "rust" / "sx-cores" / "scripts" / "decimal_vectors.py"
VECTORS = ROOT / "fixtures" / "cores" / "decimal.json"


def load_script() -> ModuleType:
    spec = importlib.util.spec_from_file_location("decimal_vectors", SCRIPT)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_committed_vectors_match_python_decimal() -> None:
    rendered = load_script().render()
    if VECTORS.read_text(encoding="utf-8") != rendered:
        pytest.fail("fixtures/cores/decimal.json is stale; regenerate it with rust/sx-cores/scripts/decimal_vectors.py")


def test_vectors_reach_every_rounding_mode_and_both_failures() -> None:
    document = json.loads(VECTORS.read_text(encoding="utf-8"))
    vectors = document["vectors"]
    modes = {vector["args"][3] for vector in vectors if vector["function"] == "decimal.div"}
    assert modes == {"HalfEven", "HalfUp", "Down", "Up", "Floor", "Ceiling"}
    failing = {vector["function"] for vector in vectors if "fails" in vector}
    assert {"decimal.div", "decimal.mul", "decimal.add", "decimal.format", "decimal.round"} <= failing
