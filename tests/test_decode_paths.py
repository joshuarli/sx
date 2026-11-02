import json
from pathlib import Path

import pytest

from sx.host import FixtureHost, run
from sx.machine import Failed, FailureKind
from sx.program import compile_source
from sx.registry import Registry, load_declarations

DECLARATIONS = Path(__file__).parent.parent / "fixtures" / "run" / "declarations.json"


def failure_message(program: str, capability: str, output: object) -> str:
    registry = Registry()
    load_declarations(registry, json.loads(DECLARATIONS.read_text()))
    checked = compile_source(program, registry, allowed=frozenset(registry.capabilities))
    assert checked.program is not None, [d.message for d in checked.diagnostics]
    responses = [{"capability": capability, "input": {}, "results": [{"output": output}]}]
    outcome = run(checked.program, FixtureHost.from_responses(registry, responses)).outcome
    assert isinstance(outcome, Failed)
    assert outcome.failure.kind is FailureKind.INVALID_HOST_RESPONSE
    return outcome.failure.message


BAD_BASE64 = "expected Bytes (standard base64 with padding), found a string that is not valid base64"


@pytest.mark.parametrize(
    ("output", "problem"),
    [
        ({"a": "AAE=", "b": "AAE"}, f'$["b"]: {BAD_BASE64}'),
        ({"a": "_-8="}, f'$["a"]: {BAD_BASE64}'),
        ({"a": "AAF="}, f'$["a"]: {BAD_BASE64}'),
        ({"a": 1}, '$["a"]: expected Bytes (a base64 string), found an integer'),
    ],
    ids=["missing-padding", "url-safe", "nonzero-padding-bits", "number"],
)
def test_bytes_decode_error_names_the_path_and_the_problem(output: object, problem: str) -> None:
    assert failure_message("(call blob.list {})", "blob.list", output).endswith(problem)
