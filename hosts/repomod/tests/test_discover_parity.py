import json
from collections.abc import Sequence
from pathlib import Path

import pytest
from fake_gh import FakeGh
from fleet import build_fleet
from harness import assert_same, run_python, run_sx

LISTING = "--paginate", "--slurp"


def listing(owner: str, endpoint: str = "users") -> list[str]:
    return [*LISTING, f"{endpoint}/{owner}/repos?type=all&per_page=100"]


def repo(full_name: str, login: str | None = None, archived: bool = False) -> dict[str, object]:
    return {"full_name": full_name, "archived": archived, "owner": {"login": login or full_name.split("/")[0]}}


def responses(gh: FakeGh) -> None:
    gh.respond(
        [
            {"arguments": ["users/acme"], "stdout": {"type": "User"}},
            {"arguments": ["users/joshuarli"], "stdout": {"type": "User"}},
            {"arguments": ["users/Zed"], "stdout": {"type": "Organization"}},
            {"arguments": ["user"], "stdout": {"login": "joshuarli"}},
            {
                "arguments": listing("acme"),
                "stdout": [[repo("acme/clean"), repo("acme/Brand-New"), repo("acme/old", archived=True),
                            repo("Zed/shared", login="Zed"), repo("elsewhere/x")]],
            },
            {
                "arguments": listing("joshuarli"),
                "stdout": [[repo("joshuarli/dotfiles"), repo("joshuarli/ZZ-new")]],
            },
            {"arguments": ["--paginate", "--slurp", "user/repos?type=all&per_page=100"],
             "stdout": [[repo("joshuarli/private"), repo("joshuarli/dotfiles")]]},
            {"arguments": listing("Zed", "orgs"), "stdout": [[repo("Zed/shared"), repo("zed/lower")]]},
        ]
    )


RUNS: Sequence[Sequence[str]] = (
    (),
    ("--owner", "acme"),
    ("--owner", "joshuarli", "--owner", "acme"),
    ("--owner", "acme", "--owner", "ACME", "--owner", "Acme"),
    ("--owner", "Zed", "--owner", "acme"),
)


def test_discover_json_matches_python_on_every_fixture(tmp_path, git_environment, monkeypatch, capsys) -> None:
    fleet = build_fleet(tmp_path, monkeypatch, checkouts=False)
    responses(FakeGh(tmp_path / "gh", monkeypatch))
    for extra in RUNS:
        assert_same(["discover", "--json", *extra], fleet, monkeypatch, capsys)


def test_discover_fixture_finds_both_kinds_of_difference(tmp_path, git_environment, monkeypatch, capsys) -> None:
    fleet = build_fleet(tmp_path, monkeypatch, checkouts=False)
    responses(FakeGh(tmp_path / "gh", monkeypatch))
    result = assert_same(["discover", "--json"], fleet, monkeypatch, capsys).json
    assert isinstance(result, dict)
    assert result["owners"] == ["acme", "joshuarli"]
    assert "acme/Brand-New" in result["uncatalogued"]
    assert "acme/gone" in result["missing"]


def test_discover_text_matches_python(tmp_path, git_environment, monkeypatch, capsys) -> None:
    fleet = build_fleet(tmp_path, monkeypatch, checkouts=False)
    responses(FakeGh(tmp_path / "gh", monkeypatch))
    for extra in RUNS:
        python = run_python(["discover", *extra], fleet.catalog, monkeypatch, capsys)
        sx = run_sx(["discover", *extra], fleet.catalog, capsys)
        assert (sx.code, sx.out, sx.err) == (python.code, python.out, python.err), extra
        assert python.out


def test_discover_reports_a_matching_catalog(tmp_path, git_environment, monkeypatch, capsys) -> None:
    fleet = build_fleet(tmp_path, monkeypatch, checkouts=False)
    gh = FakeGh(tmp_path / "gh", monkeypatch)
    names = {
        owner: [str(r.id) for r in fleet.catalog.repositories if r.id.owner == owner] for owner in ("acme", "joshuarli")
    }
    gh.respond(
        [
            {"arguments": [f"users/{owner}"], "stdout": {"type": "User"}} for owner in names
        ]
        + [{"arguments": ["user"], "stdout": {"login": "someone-else"}}]
        + [
            {"arguments": listing(owner), "stdout": [[repo(name) for name in listed]]}
            for owner, listed in names.items()
        ]
    )
    python = assert_same(["discover"], fleet, monkeypatch, capsys)
    assert python.out.splitlines() == ["uncatalogued (0): none", "catalog matches GitHub"]


@pytest.mark.parametrize("owner", ["../bad", "a b", "..", ""])
def test_invalid_owner_fails_before_any_github_call(
    owner: str, tmp_path: Path, git_environment, monkeypatch, capsys
) -> None:
    fleet = build_fleet(tmp_path, monkeypatch, checkouts=False)
    gh = FakeGh(tmp_path / "gh", monkeypatch)
    python = assert_same(["discover", "--json", "--owner", "acme", "--owner", owner], fleet, monkeypatch, capsys)
    assert python.code == 2
    assert "invalid owner" in python.err
    assert gh.calls() == []


def test_a_github_failure_is_exit_status_two_with_the_same_message(
    tmp_path, git_environment, monkeypatch, capsys
) -> None:
    fleet = build_fleet(tmp_path, monkeypatch, checkouts=False)
    gh = FakeGh(tmp_path / "gh", monkeypatch)
    gh.respond([{"arguments": ["users/acme"], "stdout": {}, "returncode": 1, "stderr": "HTTP 404 Not Found"}])
    python = assert_same(["discover", "--json", "--owner", "acme"], fleet, monkeypatch, capsys)
    assert python.code == 2
    assert python.err == "repomod: HTTP 404 Not Found\n"
    assert json.loads(json.dumps(gh.calls()))[0] == ["api", "users/acme"]
