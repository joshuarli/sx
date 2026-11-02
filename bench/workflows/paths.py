import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
CODEMODE = ROOT / "bench" / "codemode"
HARNESS = HERE / "harness"

for directory in (HERE, CODEMODE):
    if str(directory) not in sys.path:
        sys.path.insert(0, str(directory))
