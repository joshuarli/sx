import ast
from pathlib import Path

import pytest
from sx_semver import CORE as SEMVER
from sx_text import CORE as TEXT

from sx.cli import EXIT_OK, main
from sx.registry import Assurance, Core, Evidence, Registry, RegistryError

ROOT = Path(__file__).parent.parent
WORKSPACE_CORES = (TEXT, SEMVER)


def bare(*evidence: Evidence) -> Core:
    return Core(id="bare", version="0.0.1", functions=(), evidence=evidence)


def test_every_core_reports_unverified_and_only_the_levels_it_names() -> None:
    assert dict(bare().assurance) == {Assurance.UNVERIFIED: ()}
    claimed = bare(Evidence(Assurance.CONFORMANCE_TESTED, ("tests/x.py::test_x",)))
    assert dict(claimed.assurance) == {
        Assurance.UNVERIFIED: (),
        Assurance.CONFORMANCE_TESTED: ("tests/x.py::test_x",),
    }


@pytest.mark.parametrize(
    "evidence",
    [
        (Evidence(Assurance.UNVERIFIED, ("registering",)),),
        (Evidence(Assurance.CONFORMANCE_TESTED, ()),),
        (
            Evidence(Assurance.CONFORMANCE_TESTED, ("a",)),
            Evidence(Assurance.CONFORMANCE_TESTED, ("b",)),
        ),
        (Evidence(Assurance.PROVED_PROPERTIES, ("Theorem.name",)),),
    ],
    ids=["unverified-declared", "no-references", "level-twice", "proved-without-model"],
)
def test_registry_rejects_evidence_that_names_nothing_or_skips_the_model(evidence: tuple[Evidence, ...]) -> None:
    with pytest.raises(RegistryError):
        Registry().add_core(bare(*evidence))


def test_a_proof_claim_needs_a_specified_model() -> None:
    registry = Registry()
    registry.add_core(
        bare(
            Evidence(Assurance.SPECIFIED, ("lean/Bare.lean",)),
            Evidence(Assurance.PROVED_PROPERTIES, ("Bare.total",)),
        )
    )
    assert list(registry.cores["bare"].assurance) == [
        Assurance.UNVERIFIED,
        Assurance.SPECIFIED,
        Assurance.PROVED_PROPERTIES,
    ]


@pytest.mark.parametrize("core", WORKSPACE_CORES, ids=[core.id for core in WORKSPACE_CORES])
def test_every_named_conformance_test_exists(core: Core) -> None:
    for reference in core.assurance[Assurance.CONFORMANCE_TESTED]:
        path, _, name = reference.partition("::")
        tree = ast.parse((ROOT / path).read_text())
        assert name in {node.name for node in tree.body if isinstance(node, ast.FunctionDef)}, reference


def registry_with_cores() -> Registry:
    registry = Registry()
    for core in WORKSPACE_CORES:
        registry.add_core(core)
    return registry


def test_help_for_a_core_lists_its_evidence_and_what_is_missing(capsys: pytest.CaptureFixture[str]) -> None:
    assert main(["help", "semver"], registry_with_cores) == EXIT_OK
    out = capsys.readouterr().out
    assert "unverified: the interface was validated" in out
    assert "conformance-tested:\n    cores/semver/tests/test_semver_core.py::" in out
    assert "specified: none" in out
    assert "proved-properties: none" in out
    assert "(semver.compare semver.Version semver.Version) -> Int" in out


def test_cores_lists_each_core_with_its_levels(capsys: pytest.CaptureFixture[str]) -> None:
    assert main(["cores"], registry_with_cores) == EXIT_OK
    assert capsys.readouterr().out.splitlines() == [
        "text 0.1.0  unverified, conformance-tested",
        "semver 0.1.0  unverified, conformance-tested",
    ]
