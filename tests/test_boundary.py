import re
from pathlib import Path

import pytest

ROOT = Path(__file__).parent.parent
IMPORT = re.compile(r"^\s*(?:import|from)\s+repomod\b", re.MULTILINE)
INDEPENDENT = [ROOT / "src", *sorted((ROOT / "cores").glob("*"))]


@pytest.mark.parametrize("package", INDEPENDENT, ids=[p.name for p in INDEPENDENT])
def test_kernel_and_cores_never_import_repomod(package: Path) -> None:
    offenders = [str(p.relative_to(ROOT)) for p in package.rglob("*.py") if IMPORT.search(p.read_text())]
    assert offenders == []
