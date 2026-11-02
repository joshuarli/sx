import pytest
from sx_repomod.declarations import DISCOVER, STATUS_OFFLINE, Program
from sx_repomod.kernel import Kernel


@pytest.mark.parametrize("program", [DISCOVER, STATUS_OFFLINE], ids=lambda p: p.name)
def test_the_allow_list_is_exactly_the_calls_the_program_can_make_and_all_are_reads(program: Program) -> None:
    with Kernel() as kernel:
        report = kernel.send(
            {
                "type": "explain",
                "program": program.source,
                "declarations": program.declarations,
                "allow": sorted(program.allow),
            }
        )
    assert report["type"] == "explained", report
    assert {tool["id"] for tool in report["tools"]} == program.allow
    assert {tool["effect"] for tool in report["tools"]} == {"read"}
    assert all(tool["allowed"] for tool in report["tools"])


@pytest.mark.parametrize("program", [DISCOVER, STATUS_OFFLINE], ids=lambda p: p.name)
def test_a_missing_authorization_rejects_the_program_before_any_call(program: Program) -> None:
    withheld = min(program.allow)
    with Kernel() as kernel:
        reply = kernel.send(
            {
                "type": "start",
                "program": program.source,
                "declarations": program.declarations,
                "allow": sorted(program.allow - {withheld}),
                "inputs": {},
            }
        )
    assert reply["type"] == "rejected", reply
    assert [d["code"] for d in reply["diagnostics"]] == ["unauthorized"]
