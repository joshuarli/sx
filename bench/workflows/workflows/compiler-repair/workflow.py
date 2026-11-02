import re
import subprocess
import tempfile
from pathlib import Path

from definition import Verdict
from sandbox import SCRATCH, Sandbox

SOURCE = (Path(__file__).parent / "main.c.txt").read_text()
MAX_RUNS = 6
ERROR_LINE = re.compile(r"^[^:\n]+:(\d+):(\d+): error: (.*)$", re.MULTILINE)
FIXABLE = "expected ';'"
type Diagnostic = dict[str, int | str]


def setup(sandbox: Sandbox) -> None:
    sandbox.write("src/main.c", SOURCE)


def compile_errors(directory: Path) -> tuple[int, list[Diagnostic]]:
    completed = subprocess.run(
        ["/usr/bin/cc", "-fsyntax-only", "main.c"], cwd=directory, env={}, stdin=subprocess.DEVNULL,
        capture_output=True, text=True, check=False,
    )
    errors: list[Diagnostic] = [
        {"line": int(line), "column": int(column), "message": message}
        for line, column, message in ERROR_LINE.findall(completed.stderr)
    ]
    return completed.returncode, errors


def repaired_text(fixed_lines: list[int]) -> str:
    lines = SOURCE.split("\n")
    for number in fixed_lines:
        lines[number - 1] += ";"
    return "\n".join(lines)


def check(sandbox: Sandbox, result: object) -> Verdict:
    directory = sandbox.root / "src"
    with tempfile.TemporaryDirectory(dir=SCRATCH) as original:
        Path(original, "main.c").write_text(SOURCE)
        first = compile_errors(Path(original))[1]
    fixed = [int(str(error["line"])) for error in first if str(error["message"]).startswith(FIXABLE)]
    final = directory / "main.c"
    wanted_text = repaired_text(fixed)
    if not final.is_file() or final.read_text() != wanted_text:
        return Verdict.wrong("src/main.c is not the original with a semicolon added to each fixed line only")
    status, diagnostics = compile_errors(directory)
    wanted = {"attempts": len(fixed) + 1, "fixed": fixed, "status": status, "diagnostics": diagnostics}
    if result != wanted:
        return Verdict.wrong(f"the result was {result!r}, not {wanted!r}")
    extra = sorted(path.name for path in directory.iterdir() if path.name != "main.c")
    if extra or sorted(path.name for path in sandbox.root.iterdir()) != ["src"]:
        return Verdict.wrong("the program left extra files in the sandbox")
    return Verdict.correct()
