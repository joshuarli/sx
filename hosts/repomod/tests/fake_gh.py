import json
import os
import sys
from collections.abc import Sequence
from pathlib import Path

import pytest

SCRIPT = """#!{python}
import json, os, sys
calls = json.loads(open(os.environ["FAKE_GH_RESPONSES"]).read())
arguments = sys.argv[1:]
with open(os.environ["FAKE_GH_LOG"], "a") as log:
    log.write(json.dumps(arguments) + "\\n")
if arguments[:1] != ["api"]:
    sys.stderr.write("fake gh only answers api calls: " + " ".join(arguments))
    sys.exit(1)
for call in calls:
    if call["arguments"] == arguments[1:]:
        sys.stdout.write(json.dumps(call["stdout"]))
        sys.stderr.write(call.get("stderr", ""))
        sys.exit(call.get("returncode", 0))
sys.stderr.write("fake gh has no response for: " + " ".join(arguments))
sys.exit(1)
"""


class FakeGh:
    def __init__(self, directory: Path, monkeypatch: pytest.MonkeyPatch) -> None:
        self.responses = directory / "responses.json"
        self.log = directory / "calls.jsonl"
        executable = directory / "bin" / "gh"
        executable.parent.mkdir(parents=True)
        executable.write_text(SCRIPT.format(python=sys.executable))
        executable.chmod(0o755)
        self.log.write_text("")
        monkeypatch.setenv("PATH", f"{executable.parent}{os.pathsep}{os.environ['PATH']}")
        monkeypatch.setenv("FAKE_GH_RESPONSES", str(self.responses))
        monkeypatch.setenv("FAKE_GH_LOG", str(self.log))
        self.respond([])

    def respond(self, calls: Sequence[dict[str, object]]) -> None:
        self.responses.write_text(json.dumps(list(calls)))

    def calls(self) -> list[list[str]]:
        return [json.loads(line) for line in self.log.read_text().splitlines()]
