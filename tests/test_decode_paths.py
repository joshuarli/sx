import json
from pathlib import Path

import pytest

from sx.host import FixtureHost, run
from sx.machine import Failed, FailureKind
from sx.program import compile_source
from sx.registry import Registry, RegistryError, load_declarations

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


@pytest.mark.parametrize(
    ("capability", "output", "problem"),
    [
        (
            "profile.get",
            {"id": "p1", "address": {"city": "Oslo", "zip": "0150"}},
            "$.address.zip: expected no field, because the record type does not declare it, found a field",
        ),
        ("accounts.get", {"id": "a1", "plan": "pro"}, "$.name: expected String, found no field"),
        ("accounts.get", {"id": "a1", "name": 7}, "$.name: expected String, found an integer"),
        (
            "listing.get",
            {"owner": {"id": "o1"}, "count": 2, "page": 1},
            "$.page: expected no field, because the record type does not declare it, found a field",
        ),
    ],
    ids=["closed-inside-open", "open-missing-field", "open-wrong-type", "closed-around-open"],
)
def test_open_record_still_checks_what_it_names(capability: str, output: object, problem: str) -> None:
    program = f"(call {capability} {{}})"
    assert failure_message(program, capability, output).endswith(problem)


@pytest.mark.parametrize(
    "text",
    ["Record open", "Record open String", "open Record {id: String}", "Record open open {id: String}"],
)
def test_open_is_declared_between_record_and_its_fields(text: str) -> None:
    with pytest.raises(RegistryError):
        Registry().parse(text)


@pytest.mark.parametrize(
    ("capability", "output", "problem"),
    [
        ("store.list", ["a", "\ud800"], "$[1]: expected String, found a string with a lone surrogate"),
        ("totals.get", {"\udc00": 1}, '$["\\udc00"]: expected a key that is Unicode text, found a lone surrogate'),
    ],
    ids=["string", "map-key"],
)
def test_a_lone_surrogate_is_a_mismatch(capability: str, output: object, problem: str) -> None:
    program = f"(call {capability} {{}})"
    assert failure_message(program, capability, output).endswith(problem)
