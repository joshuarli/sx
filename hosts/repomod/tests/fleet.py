import json
import os
from dataclasses import dataclass
from pathlib import Path

import pytest
from git_helpers import git, make_remote
from repomod.catalog import load_catalog
from repomod.types import Catalog

NOW = 1_800_000_000
DAY = 86_400
SHA = "0123456789abcdef0123456789abcdef01234567"

CATALOG = """version = 1

[workspace]
root = "~/repos"
personal_owner = "joshuarli"

[repos."acme"]
ahead = ["tool"]
behind = ["tool"]
clean = ["tool"]
detached = ["tool"]
dirty = ["tool"]
diverged = ["tool"]
forked = ["tool", "fork"]
forked-old = ["tool", "fork"]
gone = ["tool"]
linked = ["tool"]
plain = ["library"]
stale-topic = ["tool"]
topic = ["tool"]
uncached = ["tool"]
wrong-origin = ["tool"]

[repos."joshuarli"]
dotfiles = ["config"]
"""


@dataclass(frozen=True, slots=True)
class Fleet:
    catalog: Catalog
    root: Path
    cache: Path


def commit(checkout: Path, name: str, monkeypatch: pytest.MonkeyPatch, at: int | None = None) -> None:
    (checkout / f"{name}.txt").write_text(name, encoding="utf-8")
    git("add", f"{name}.txt", cwd=checkout)
    if at is not None:
        monkeypatch.setenv("GIT_COMMITTER_DATE", f"{at} +0000")
    git("commit", "-m", name, cwd=checkout)
    monkeypatch.delenv("GIT_COMMITTER_DATE", raising=False)


def stamp_last_checkout(checkout: Path, at: int) -> None:
    reflog = checkout / ".git" / "logs" / "HEAD"
    lines = reflog.read_text(encoding="utf-8").splitlines()
    identity, _, rest = lines[-1].partition("> ")
    _, _, tail = rest.partition(" ")
    lines[-1] = f"{identity}> {at} {tail}"
    reflog.write_text("\n".join(lines) + "\n", encoding="utf-8")


def clone(tmp_path: Path, root: Path, name: str) -> tuple[Path, Path, str]:
    _, seed, url = make_remote(tmp_path, name)
    checkout = root / "acme" / name
    checkout.parent.mkdir(parents=True, exist_ok=True)
    git("clone", "--branch", "main", url, str(checkout))
    return checkout, Path(seed), str(url)


def build_checkouts(tmp_path: Path, root: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    clean, _, _ = clone(tmp_path, root, "clean")
    todo = "### ABC-1 · P1 · first\n### ABC-2 · P2 · second\nnot an item\n"
    (clean / "TODO.md").write_text(todo, encoding="utf-8")
    git("add", "TODO.md", cwd=clean)
    git("commit", "-m", "todo", cwd=clean)
    git("push", "origin", "main", cwd=clean)

    dirty, _, _ = clone(tmp_path, root, "dirty")
    (dirty / "README.md").write_text("changed\n", encoding="utf-8")
    (dirty / "staged.txt").write_text("staged\n", encoding="utf-8")
    git("add", "staged.txt", cwd=dirty)
    (dirty / "untracked.txt").write_text("untracked\n", encoding="utf-8")

    ahead, _, _ = clone(tmp_path, root, "ahead")
    commit(ahead, "old", monkeypatch, NOW - 9 * DAY)
    commit(ahead, "new", monkeypatch, NOW - 2 * DAY)

    behind, seed, _ = clone(tmp_path, root, "behind")
    commit(seed, "remote", monkeypatch)
    git("push", "origin", "main", cwd=seed)
    git("fetch", "origin", cwd=behind)

    diverged, seed, _ = clone(tmp_path, root, "diverged")
    commit(diverged, "local", monkeypatch, NOW - 1 * DAY)
    commit(seed, "remote", monkeypatch)
    git("push", "origin", "main", cwd=seed)
    git("fetch", "origin", cwd=diverged)

    topic, _, _ = clone(tmp_path, root, "topic")
    git("checkout", "-b", "topic", cwd=topic)
    stamp_last_checkout(topic, NOW - 3 * DAY)
    stale, _, _ = clone(tmp_path, root, "stale-topic")
    git("checkout", "-b", "repomod/fix/example", cwd=stale)
    stamp_last_checkout(stale, NOW - 12 * DAY)
    git("checkout", "main", cwd=stale)
    git("checkout", "repomod/fix/example", cwd=stale)
    stamp_last_checkout(stale, NOW - 12 * DAY)

    detached, _, _ = clone(tmp_path, root, "detached")
    git("checkout", "--detach", cwd=detached)
    wrong, _, _ = clone(tmp_path, root, "wrong-origin")
    git("remote", "set-url", "origin", "https://github.com/acme/other.git", cwd=wrong)
    clone(tmp_path, root, "forked")
    clone(tmp_path, root, "forked-old")
    clone(tmp_path, root, "uncached")

    plain = root / "acme" / "plain"
    plain.mkdir(parents=True)
    (plain / "README.md").write_text("not a checkout\n", encoding="utf-8")
    elsewhere = tmp_path / "elsewhere"
    elsewhere.mkdir()
    (root / "acme" / "linked").symlink_to(elsewhere)

    dotfiles = root / "dotfiles"
    git("init", "--initial-branch=main", str(dotfiles))
    (dotfiles / "README.md").write_text("dots\n", encoding="utf-8")
    git("add", "README.md", cwd=dotfiles)
    git("commit", "-m", "initial", cwd=dotfiles)
    git("remote", "add", "origin", "https://github.com/joshuarli/dotfiles.git", cwd=dotfiles)


def workflow(name: str, state: str, url: str | None = None) -> dict[str, object]:
    return {"name": name, "state": state, "url": url, "path": f".github/workflows/{name.lower()}.yml"}


def ci_entry(
    state: object,
    workflows: list[dict[str, object]] | None,
    fetched_at: float = NOW - 100,
    fork_checks: bool = False,
) -> dict[str, object]:
    entry: dict[str, object] = {
        "default_branch": "main",
        "sha": SHA,
        "state": state,
        "checks": [{"name": "CI", "state": "success", "url": None, "source": None, "updated_at": None}],
        "fetched_at": fetched_at,
    }
    if workflows is not None:
        entry |= {"workflow_source": "actions-runs-v3", "workflows": workflows}
    if fork_checks:
        entry["fork_check_source"] = "fork-workflows-v1"
    return entry


def write_caches(cache: Path) -> None:
    ci = {
        "acme/clean": ci_entry("passed", [workflow("CI", "passed", "https://ci/1"), workflow("Lint", "skipped")]),
        "acme/dirty": ci_entry("failed", [workflow("CI", "failed", "https://ci/2")]),
        "acme/ahead": ci_entry("pending", [workflow("CI", "pending")], fetched_at=NOW - 99.5),
        "acme/behind": ci_entry("bogus", [workflow("CI", "passed"), workflow("Odd", "weird")]),
        "acme/diverged": ci_entry("stale", None),
        "acme/topic": ci_entry("passed", [workflow("Release", "cancelled", "https://ci/3")]),
        "acme/detached": ci_entry("none", []),
        "acme/gone": ci_entry("passed", [workflow("CI", "passed")]),
        "acme/forked": ci_entry(
            "passed",
            [workflow("CI", "passed"), workflow("Lint", "passed"), workflow("Test", "failed")],
            fork_checks=True,
        ),
        "acme/forked-old": ci_entry("passed", [workflow("CI", "passed")]),
        "joshuarli/dotfiles": ci_entry("neutral", [workflow("CI", "neutral")]),
    }
    entries = {f"{name}@{SHA}": entry for name, entry in ci.items()}
    entries[f"acme/wrong-origin@{SHA}"] = ci_entry("unknown", [workflow("CI", "passed")])
    write(cache / "repomod" / "ci.json", {"version": 1, "entries": entries})
    upstream = {
        "acme/forked": {
            "parent": "up/stream", "parent_branch": "main", "parent_sha": SHA, "fork_sha": SHA,
            "behind_by": 3, "ahead_by": 1, "state": "diverged", "fetched_at": NOW - 50,
        },
        "acme/clean": {
            "parent": "up/clean", "parent_branch": "trunk", "parent_sha": None, "fork_sha": None,
            "behind_by": 0, "ahead_by": 0, "state": "identical", "fetched_at": NOW - 7.5,
        },
        "acme/dirty": {
            "parent": "up/dirty", "behind_by": 0, "ahead_by": 2, "state": "ahead", "fetched_at": NOW - 5,
        },
        "acme/behind": {"parent": "", "behind_by": 1, "ahead_by": 0, "state": "behind", "fetched_at": NOW},
        "acme/topic": {
            "parent": "up/topic", "behind_by": 4, "ahead_by": 0, "state": "unknown", "fetched_at": NOW + 10,
        },
    }
    write(
        cache / "repomod" / "upstream.json",
        {"version": 1, "entries": {f"{name}@{SHA}": entry for name, entry in upstream.items()}},
    )


def write(path: Path, value: object) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value), encoding="utf-8")


def build_fleet(tmp_path: Path, monkeypatch: pytest.MonkeyPatch, *, checkouts: bool = True) -> Fleet:
    root = tmp_path / "fleet"
    root.mkdir()
    if checkouts:
        build_checkouts(tmp_path, root, monkeypatch)
    catalog_path = tmp_path / "catalog.toml"
    catalog_path.write_text(CATALOG, encoding="utf-8")
    catalog = load_catalog(catalog_path, {"HOME": str(tmp_path), "REPOMOD_ROOT": str(root)})
    cache = Path(os.environ["XDG_CACHE_HOME"])
    write_caches(cache)
    return Fleet(catalog, root, cache)
