from __future__ import annotations

import re
from collections.abc import Callable
from dataclasses import dataclass, replace

from sx.registry import Registry

from .differential import ROOT, Case, Finding, make_case, registry, run_case, save_finding, scratch
from .rust import Rust

TOKEN = re.compile(r'"(?:[^"\\]|\\.)*"|[()\[\]{}]|[^\s()\[\]{}"]+')
CLOSERS = {"(": ")", "[": "]", "{": "}"}
REPLACEMENTS = ("0", "1", "true", "false", "none", "[]", '""', "1.5", "{}", "Failing")


@dataclass(frozen=True, slots=True)
class Atom:
    text: str


@dataclass(frozen=True, slots=True)
class Group:
    opener: str
    items: tuple[Node, ...]


type Node = Atom | Group


def parse(source: str) -> list[Node] | None:
    stack: list[tuple[str, list[Node]]] = [("", [])]
    for token in TOKEN.findall(source):
        if token in CLOSERS:
            stack.append((token, []))
        elif token in CLOSERS.values():
            if len(stack) == 1 or CLOSERS[stack[-1][0]] != token:
                return None
            opener, items = stack.pop()
            stack[-1][1].append(Group(opener, tuple(items)))
        else:
            stack[-1][1].append(Atom(token))
    if len(stack) != 1:
        return None
    return stack[0][1]


def render(node: Node) -> str:
    if isinstance(node, Atom):
        return node.text
    return node.opener + " ".join(render(item) for item in node.items) + CLOSERS[node.opener]


def source_of(nodes: list[Node]) -> str:
    return " ".join(render(node) for node in nodes)


def variants(node: Node) -> list[Node]:
    out: list[Node] = [Atom(text) for text in REPLACEMENTS]
    if isinstance(node, Group):
        out = [*node.items, *out]
        for i in range(len(node.items)):
            out.append(Group(node.opener, node.items[:i] + node.items[i + 1 :]))
        for i in range(len(node.items) - 1):
            out.append(Group(node.opener, node.items[:i] + node.items[i + 2 :]))
    return out


def replace_at(node: Node, path: tuple[int, ...], new: Node) -> Node:
    if not path:
        return new
    assert isinstance(node, Group)
    items = list(node.items)
    items[path[0]] = replace_at(items[path[0]], path[1:], new)
    return Group(node.opener, tuple(items))


def paths(node: Node, prefix: tuple[int, ...] = ()) -> list[tuple[int, ...]]:
    out: list[tuple[int, ...]] = [prefix]
    if isinstance(node, Group):
        for i, item in enumerate(node.items):
            out += paths(item, (*prefix, i))
    return out


def node_at(node: Node, path: tuple[int, ...]) -> Node:
    for i in path:
        assert isinstance(node, Group)
        node = node.items[i]
    return node


def category(finding: Finding) -> str:
    if finding.kind == "outcome":
        labels = [str(label) for label in re.findall(r'"(?:completed|failed)": ?(?:"[a-z-]+")?', finding.detail)]
        return finding.kind + ":" + "/".join(labels)
    return finding.kind


def minimize(case: Case, known: Registry, rust: Rust, log: Callable[[str], None] = lambda _: None) -> Case:
    first = run_case(case, known, rust)
    assert first.finding is not None
    wanted = category(first.finding)

    def still_fails(source: str) -> bool:
        result = run_case(replace(case, source=source), known, rust)
        return result.finding is not None and category(result.finding) == wanted

    parsed = parse(case.source)
    assert parsed is not None
    nodes = parsed
    if len(nodes) != 1:
        return case
    root = nodes[0]
    progress = True
    while progress:
        progress = False
        for path in paths(root):
            try:
                current = node_at(root, path)
            except (AssertionError, IndexError):
                continue
            size = len(render(current))
            for candidate in variants(current):
                if len(render(candidate)) >= size:
                    continue
                trial = replace_at(root, path, candidate)
                if still_fails(render(trial)):
                    root = trial
                    progress = True
                    log(render(root))
                    break
    return replace(case, source=render(root))


def triage_one(seed: str, index: int, mutate_rate: float) -> str:
    known = registry()
    case = make_case(seed, index, known, mutate_rate)
    rust = Rust(scratch())
    try:
        small = minimize(case, known, rust)
        result = run_case(small, known, rust)
    finally:
        rust.reset()
    save_finding(small, result, ROOT / ".work" / "tmp" / "fuzz" / "minimized")
    if result.finding is None:
        return f"{small.source}\n{result.status}"
    return f"{small.source}\n{result.finding.kind}: {result.finding.detail[:400]}"
