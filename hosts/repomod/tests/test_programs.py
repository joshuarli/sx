import pytest
from sx_repomod.declarations import DISCOVER, STATUS_OFFLINE, Program, new_registry

from sx.program import compile_source
from sx.registry import Effect


@pytest.mark.parametrize("program", [DISCOVER, STATUS_OFFLINE], ids=lambda p: p.name)
def test_the_allow_list_is_exactly_the_calls_the_program_can_make_and_all_are_reads(program: Program) -> None:
    registry = new_registry()
    checked = compile_source(program.source, registry, allowed=program.allow, inputs=program.input_types(registry))
    assert checked.program is not None, [d.message for d in checked.diagnostics]
    assert checked.program.capabilities == program.allow
    effects = {capability.effect for c in program.allow if (capability := registry.capability(c)) is not None}
    assert effects == {Effect.READ}


@pytest.mark.parametrize("program", [DISCOVER, STATUS_OFFLINE], ids=lambda p: p.name)
def test_a_missing_authorization_rejects_the_program_before_any_call(program: Program) -> None:
    registry = new_registry()
    withheld = min(program.allow)
    checked = compile_source(
        program.source, registry, allowed=program.allow - {withheld}, inputs=program.input_types(registry)
    )
    assert checked.program is None
    assert [d.code for d in checked.diagnostics] == ["unauthorized"]
