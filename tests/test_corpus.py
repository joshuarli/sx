import subprocess
from pathlib import Path

BENCH = Path(__file__).parent.parent / "bench" / "codemode"


def test_every_reference_solution_produces_its_expected_value() -> None:
    result = subprocess.run(
        ["node", str(BENCH / "check_references.mjs")], capture_output=True, text=True, check=False
    )
    assert result.returncode == 0, result.stdout + result.stderr
