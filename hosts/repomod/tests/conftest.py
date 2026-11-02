import importlib.util
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

sys.dont_write_bytecode = True
os.environ["PYTHONDONTWRITEBYTECODE"] = "1"


def locate_repomod() -> Path:
    configured = os.environ.get("SX_REPOMOD")
    if configured is not None:
        return Path(configured).resolve()
    for parent in Path(__file__).resolve().parents:
        candidate = parent / "repomod"
        if (candidate / "src" / "repomod" / "cli.py").is_file():
            return candidate
    raise RuntimeError("repomod is not next to this repository; set SX_REPOMOD to its checkout")


REPOMOD = locate_repomod()
sys.path[:0] = [str(REPOMOD / "src"), str(REPOMOD / "tests")]

spec = importlib.util.spec_from_file_location("repomod_conftest", REPOMOD / "tests" / "conftest.py")
assert spec is not None
assert spec.loader is not None
repomod_conftest = importlib.util.module_from_spec(spec)
spec.loader.exec_module(repomod_conftest)

git_environment = repomod_conftest.git_environment


@pytest.fixture(autouse=True)
def isolated_cache(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Path:
    cache = tmp_path / "cache"
    monkeypatch.setenv("XDG_CACHE_HOME", str(cache))
    return cache


SX_ROOT = Path(__file__).resolve().parents[3]


@pytest.fixture(scope="session", autouse=True)
def rust_kernel() -> Path:
    built = subprocess.run(
        ["cargo", "build", "--quiet", "--message-format=json", "-p", "sx-cli"],
        cwd=SX_ROOT / "rust",
        capture_output=True,
        text=True,
        check=True,
    )
    for line in reversed(built.stdout.splitlines()):
        artifact = json.loads(line)
        if artifact.get("reason") == "compiler-artifact" and artifact["target"]["name"] == "sx":
            binary = Path(artifact["executable"])
            os.environ["SX_KERNEL"] = str(binary)
            return binary
    raise RuntimeError("cargo built no sx binary")
