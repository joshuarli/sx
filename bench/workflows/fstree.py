import os
from pathlib import Path

type Node = tuple[str, str]
type Tree = dict[str, Node]

DIRECTORY: Node = ("directory", "")


def file(text: str) -> Node:
    return ("file", text)


def link(target: str) -> Node:
    return ("link", target)


def snapshot(root: Path) -> Tree:
    tree: Tree = {}
    for path in sorted(root.rglob("*")):
        name = str(path.relative_to(root))
        if path.is_symlink():
            tree[name] = link(os.readlink(path))
        elif path.is_dir():
            tree[name] = DIRECTORY
        else:
            tree[name] = file(path.read_text())
    return tree


def relative(root: Path, path: object) -> str | None:
    if not isinstance(path, str):
        return None
    inside = os.path.relpath(os.path.normpath(path), root)
    return None if inside == ".." or inside.startswith("../") else inside


def difference(wanted: Tree, found: Tree) -> str:
    missing = sorted(set(wanted) - set(found))
    extra = sorted(set(found) - set(wanted))
    changed = sorted(name for name in set(wanted) & set(found) if wanted[name] != found[name])
    if not (missing or extra or changed):
        return ""
    return f"missing {missing}, unexpected {extra}, changed {changed}"
