import subprocess
import sys

import pytest
from sx_repomod.cli import main


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


def test_help_and_cores_work_where_repomod_cannot_be_imported() -> None:
    script = (
        "import sys; from sx_repomod.cli import main; code = main(['cores']); "
        "sys.exit(code or ('repomod' in sys.modules))"
    )
    done = subprocess.run([sys.executable, "-c", script], capture_output=True, text=True, check=False)
    assert done.returncode == 0, done.stderr
    assert "github 0.1.0" in done.stdout
