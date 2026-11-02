import importlib.util
import json
import os
import sys
from pathlib import Path
from typing import Any, override

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "hosts" / "repomod" / "tests"))
CONFTEST = ROOT / "hosts" / "repomod" / "tests" / "conftest.py"
spec = importlib.util.spec_from_file_location("repomod_host_conftest", CONFTEST)
assert spec is not None
assert spec.loader is not None
host_conftest = importlib.util.module_from_spec(spec)
spec.loader.exec_module(host_conftest)

from fleet import NOW, build_fleet  # noqa: E402
from sx_repomod.capabilities import Capabilities, CapabilityError, Environment  # noqa: E402
from sx_repomod.declarations import STATUS_OFFLINE  # noqa: E402
from sx_repomod.runner import Succeeded, run_program  # noqa: E402

isolated_cache = host_conftest.isolated_cache
git_environment = host_conftest.git_environment
rust_kernel = host_conftest.rust_kernel

OUTPUT = Path(os.environ.get("SX_BENCH_STATUS", ROOT / "bench" / "kernel" / "status-offline.json"))


class Recording(Capabilities):
    def __init__(self, environment: Environment) -> None:
        super().__init__(environment)
        self.responses: list[dict[str, Any]] = []

    @override
    def call(self, capability_id: str, payload: Any) -> Any:
        try:
            output = super().call(capability_id, payload)
        except CapabilityError as failure:
            self.record(capability_id, payload, {"error": {"kind": failure.kind, "message": failure.message}})
            raise
        self.record(capability_id, payload, {"output": output})
        return output

    def record(self, capability_id: str, payload: Any, result: dict[str, Any]) -> None:
        self.responses.append({"capability": capability_id, "input": payload, "results": [result]})


def test_record_the_status_program_over_the_fleet(tmp_path: Path, git_environment: Any, monkeypatch: Any) -> None:
    fleet = build_fleet(tmp_path, monkeypatch)
    recording = Recording(Environment(lambda: fleet.catalog, lambda: float(NOW)))
    inputs = {"repos": [], "tags": [], "only-dirty": False, "only-failed": False, "refresh": False}
    outcome = run_program(STATUS_OFFLINE, inputs, recording)
    assert isinstance(outcome, Succeeded)
    OUTPUT.write_text(json.dumps({"inputs": inputs, "responses": recording.responses}, indent=1, sort_keys=True) + "\n")
