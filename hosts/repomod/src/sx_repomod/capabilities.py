from __future__ import annotations

import time
from collections.abc import Callable, Mapping
from dataclasses import dataclass
from pathlib import Path
from threading import Lock
from typing import Any

from repomod import github, policy
from repomod.catalog import CatalogError, validate_identifier
from repomod.cmd import common
from repomod.cmd import status as status_command
from repomod.git import GitError, branch_checked_out_time, oldest_unpushed_commit_time
from repomod.github import GitHubError
from repomod.policy import PolicyError
from repomod.types import Catalog, CiState, GitStatus, Repository

type Json = Any

CI_VARIANT_OF = {
    "passed": "Passed",
    "failed": "Failed",
    "pending": "Pending",
    "cancelled": "Cancelled",
    "skipped": "Skipped",
    "neutral": "Neutral",
    "none": "NoChecks",
    "unknown": "Unknown",
    "stale": "Stale",
}
UNCATALOGUED = "repository is not catalogued"


class CapabilityError(Exception):
    def __init__(self, kind: str, message: str) -> None:
        super().__init__(message)
        self.kind = kind
        self.message = message


@dataclass(slots=True)
class Environment:
    load_catalog: Callable[[], Catalog] = common._catalog
    clock: Callable[[], float] = time.time


def failure_kind(error: Exception) -> str:
    if isinstance(error, CatalogError):
        return "NotFound" if str(error).startswith(UNCATALOGUED) else "Invalid"
    if isinstance(error, GitHubError):
        return "Unavailable"
    return "Failed"


def checkout_json(status: GitStatus) -> Json:
    if not status.present:
        return "Missing"
    if not status.valid:
        if status.error is None:
            raise CapabilityError("Failed", f"{status.repository} is invalid without an error")
        return {"tag": "Invalid", "value": status.error}
    head = {
        "staged": status.staged,
        "unstaged": status.unstaged,
        "untracked": status.untracked,
        "ahead": status.ahead,
        "behind": status.behind,
        "upstream": status.upstream,
    }
    if not status.origin_valid:
        if status.error is None:
            raise CapabilityError("Failed", f"{status.repository} has a wrong origin without an error")
        return {"tag": "OriginMismatch", "value": {"branch": status.branch, "head": head, "error": status.error}}
    if status.detached:
        return {"tag": "Detached", "value": head}
    if status.branch is None:
        raise CapabilityError("Failed", f"{status.repository} is on no branch but is not detached")
    return {"tag": "OnBranch", "value": {"branch": status.branch, "head": head}}


def ci_variant(state: CiState) -> str:
    return CI_VARIANT_OF[state]


class Capabilities:
    def __init__(self, environment: Environment | None = None) -> None:
        self.environment = environment or Environment()
        self.catalog_value: Catalog | None = None
        self.catalog_lock = Lock()
        self.handlers: Mapping[str, Callable[[Json], Json]] = {
            "catalog.select": self.catalog_select,
            "catalog.owner": self.catalog_owner,
            "github.owner-repos": self.github_owner_repos,
            "clock.now": self.clock_now,
            "git.status": self.git_status,
            "git.oldest-unpushed": self.git_oldest_unpushed,
            "git.branch-since": self.git_branch_since,
            "repo.todo-items": self.repo_todo_items,
            "github.cached-ci": self.github_cached_ci,
            "github.cached-upstream": self.github_cached_upstream,
            "policy.lint": self.policy_lint,
        }

    def call(self, capability_id: str, payload: Json) -> Json:
        try:
            return self.handlers[capability_id](payload)
        except CapabilityError:
            raise
        except (CatalogError, GitHubError, GitError, PolicyError, OSError, ValueError) as error:
            raise CapabilityError(failure_kind(error), str(error)) from error

    def catalog(self) -> Catalog:
        with self.catalog_lock:
            if self.catalog_value is None:
                self.catalog_value = self.environment.load_catalog()
            return self.catalog_value

    def repository(self, repo: str) -> Repository:
        found = [r for r in self.catalog().repositories if str(r.id) == repo]
        if len(found) != 1:
            raise CapabilityError("NotFound", f"{UNCATALOGUED}: {repo}")
        return found[0]

    def checkout_path(self, repo: str) -> Path:
        catalog, repository = self.catalog(), self.repository(repo)
        personal = repository.id.owner.casefold() == catalog.personal_owner.casefold()
        parts = (repository.id.name,) if personal else (repository.id.owner, repository.id.name)
        return Path(catalog.root, *parts)

    def catalog_select(self, payload: Json) -> Json:
        selected = common._select(self.catalog(), payload["repos"], payload["tags"])
        return [{"id": str(r.id), "owner": r.id.owner} for r in selected]

    def catalog_owner(self, payload: Json) -> Json:
        return validate_identifier(payload["owner"], "owner")

    def github_owner_repos(self, payload: Json) -> Json:
        owners = {owner.casefold() for owner in payload["owners"]}
        return github._discover_owner(payload["owner"], owners)

    def clock_now(self, payload: Json) -> Json:
        return int(self.environment.clock() * 1000)

    def git_status(self, payload: Json) -> Json:
        _, status = status_command._checkout_status(self.catalog(), self.repository(payload["repo"]))
        return checkout_json(status)

    def git_oldest_unpushed(self, payload: Json) -> Json:
        return oldest_unpushed_commit_time(self.checkout_path(payload["repo"]))

    def git_branch_since(self, payload: Json) -> Json:
        return branch_checked_out_time(self.checkout_path(payload["repo"]), payload["branch"])

    def repo_todo_items(self, payload: Json) -> Json:
        return status_command._todo_item_count(self.checkout_path(payload["repo"]))

    def github_cached_ci(self, payload: Json) -> Json:
        repository = self.repository(payload["repo"])
        name = str(repository.id)
        result = github.ci_status(
            (repository,), offline=True, refresh=payload["refresh"], now=payload["now"] / 1000
        )[name]
        if not result.cached:
            return {"tag": "NotCached", "value": result.error or "no cached CI result"}
        if result.age_seconds is None:
            raise CapabilityError("Failed", f"cached CI result for {name} has no age")
        workflows = (
            None
            if result.workflows is None
            else [
                {"name": w.name, "state": ci_variant(w.state), "url": w.url} for w in result.workflows
            ]
        )
        report = {
            "sha": result.sha,
            "default-branch": result.default_branch,
            "state": ci_variant(result.state),
            "age-seconds": result.age_seconds,
            "workflows": workflows,
        }
        return {"tag": "Cached", "value": report}

    def github_cached_upstream(self, payload: Json) -> Json:
        repository = self.repository(payload["repo"])
        name = str(repository.id)
        found = github.upstream_status((repository,), offline=True, now=payload["now"] / 1000)
        upstream = found.get(name)
        if upstream is None:
            return None
        if upstream.behind_by is None or upstream.ahead_by is None or upstream.age_seconds is None:
            raise CapabilityError("Failed", f"cached upstream result for {name} is incomplete")
        return {
            "parent": upstream.parent,
            "parent-branch": upstream.parent_branch,
            "parent-sha": upstream.parent_sha,
            "behind-by": upstream.behind_by,
            "ahead-by": upstream.ahead_by,
            "state": upstream.state.capitalize(),
            "age-seconds": upstream.age_seconds,
        }

    def policy_lint(self, payload: Json) -> Json:
        repositories = tuple(self.repository(repo) for repo in payload["repos"])
        findings = policy.lint_repositories(
            self.catalog(),
            repositories,
            local_only=True,
            workers=min(32, max(1, 2 * len(repositories))),
        )
        return [policy.finding_dict(finding) for finding in findings]
