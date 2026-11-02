import json
from collections.abc import Sequence

import pytest
from fleet import build_fleet
from harness import assert_same, run_python, run_sx

JSON_RUNS: Sequence[Sequence[str]] = (
    (),
    ("--dirty",),
    ("--failed",),
    ("--dirty", "--failed"),
    ("--refresh",),
    ("--refresh", "--failed"),
    ("--repo", "acme/ahead"),
    ("--repo", "topic", "--repo", "acme/clean", "--repo", "dotfiles"),
    ("--repo", "ACME/CLEAN"),
    ("--tag", "fork"),
    ("--tag", "config", "--tag", "library"),
)


def test_status_offline_json_matches_python_on_every_fixture(
    tmp_path, git_environment, monkeypatch, capsys
) -> None:
    fleet = build_fleet(tmp_path, monkeypatch)
    for extra in JSON_RUNS:
        assert_same(["status", "--offline", "--json", *extra], fleet, monkeypatch, capsys)


def test_fixtures_cover_every_checkout_and_cache_variant(tmp_path, git_environment, monkeypatch, capsys) -> None:
    fleet = build_fleet(tmp_path, monkeypatch)
    rows = {
        row["repository"]: row
        for row in assert_same(["status", "--offline", "--json"], fleet, monkeypatch, capsys).json  # type: ignore[union-attr]
    }
    labels = {name: row["local"] for name, row in rows.items()}
    assert labels == {
        "acme/ahead": "ahead 2 (last fetch), oldest unpushed 9d",
        "acme/behind": "behind 1 (last fetch)",
        "acme/clean": "clean",
        "acme/detached": "detached",
        "acme/dirty": "dirty (3)",
        "acme/diverged": "diverged 1/1 (last fetch), oldest unpushed 1d",
        "acme/forked": "clean",
        "acme/forked-old": "clean on main",
        "acme/gone": "missing",
        "acme/linked": "invalid",
        "acme/plain": "invalid",
        "acme/stale-topic": "clean on repomod/fix/example for 12d",
        "acme/topic": "clean on topic for 3d",
        "acme/uncached": "clean on main",
        "acme/wrong-origin": "invalid origin",
        "joshuarli/dotfiles": "clean",
    }
    assert {name: row["ci_state"] for name, row in rows.items()} == {
        "acme/ahead": "pending", "acme/behind": "unknown", "acme/clean": "passed", "acme/detached": "none",
        "acme/dirty": "failed", "acme/diverged": "stale", "acme/forked": "passed", "acme/forked-old": "unknown",
        "acme/gone": "passed", "acme/linked": "unknown", "acme/plain": "unknown", "acme/stale-topic": "unknown",
        "acme/topic": "passed", "acme/uncached": "unknown", "acme/wrong-origin": "unknown",
        "joshuarli/dotfiles": "neutral",
    }
    assert rows["acme/stale-topic"]["stale_local_work"] is True
    assert rows["acme/clean"]["todo_items"] == 2
    assert rows["acme/ahead"]["ci_age_seconds"] == 99
    assert rows["acme/forked-old"]["ci_error"] == "no cached CI result"
    assert rows["acme/forked"]["upstream"]["state"] == "diverged"
    assert rows["acme/gone"]["local_error"] is None
    assert rows["acme/wrong-origin"]["local_error"] == "origin does not match the catalogued GitHub repository"
    assert any(row["lint"] and len(row["lint_findings"]) == row["lint"] for row in rows.values())


def test_status_offline_text_matches_python(tmp_path, git_environment, monkeypatch, capsys) -> None:
    fleet = build_fleet(tmp_path, monkeypatch)
    for extra in ((), ("--dirty",), ("--failed",), ("--repo", "acme/topic")):
        python = run_python(["status", "--offline", *extra], fleet.catalog, monkeypatch, capsys)
        sx = run_sx(["status", "--offline", *extra], fleet.catalog, capsys)
        assert (sx.code, sx.out, sx.err) == (python.code, python.out, python.err), extra
        assert python.out


@pytest.mark.parametrize(
    "extra",
    [
        ("--repo", "acme/nope"),
        ("--repo", "clean", "--repo", "dotfiles", "--repo", "nope"),
        ("--tag", "bogus"),
        ("--tag", "tool", "--tag", "tool"),
    ],
)
def test_status_offline_failures_match_python_exit_status_and_message(
    extra: Sequence[str], tmp_path, git_environment, monkeypatch, capsys
) -> None:
    fleet = build_fleet(tmp_path, monkeypatch, checkouts=False)
    python = assert_same(["status", "--offline", "--json", *extra], fleet, monkeypatch, capsys)
    assert python.code == 2
    assert python.out == ""
    assert python.err.startswith("repomod: ")


def test_unreadable_ci_cache_marks_every_repository_not_cached(
    tmp_path, git_environment, monkeypatch, capsys
) -> None:
    fleet = build_fleet(tmp_path, monkeypatch, checkouts=False)
    (fleet.cache / "repomod" / "ci.json").write_text("{not json", encoding="utf-8")
    python = assert_same(["status", "--offline", "--json"], fleet, monkeypatch, capsys)
    errors = {row["ci_error"] for row in json.loads(python.out)}
    assert len(errors) == 1
    assert next(iter(errors)).startswith("CI cache is unreadable")
