import json
import re
from pathlib import Path

from definition import Evidence, Verdict
from sandbox import Sandbox

CUTOFF = "2025-12-10"
REPORTS: dict[str, str] = {
    "sales-2025-10-02.csv": "id,total\n1,10\n",
    "sales-2025-11-03.csv": "id,total\n2,20\n",
    "ops-2025-10-31.log": "started\nstopped\n",
    "ops-2025-11-03.log": "new ops text\n",
    "ops-2025-12-09.log": "late ops\n",
    "ops-2025-12-10.log": "boundary\n",
    "sales-2026-01-15.csv": "id,total\n3,30\n",
    "audit-2024-12-31.txt": "audit\n",
    "sales-2025-11-3.csv": "short day\n",
    "sales-2025-13-01.csv": "no such month\n",
    "notes.txt": "remember\n",
    "README": "reports\n",
    "drafts/old-2020-01-01.txt": "inside a directory\n",
}
ARCHIVE: dict[str, str] = {
    "INDEX.txt": "# archive index\nold-2024-01-01.txt -> 2024/01/old-2024-01-01.txt\n",
    "2024/01/old-2024-01-01.txt": "old\n",
    "2025/10/ops-2025-10-31.log": "started\nstopped\n",
    "2025/11/sales-2025-11-03.csv": "id,total\n2,20\n",
    "2025/11/ops-2025-11-03.log": "earlier ops text\n",
    "2025/11/ops-2025-11-03.1.log": "another earlier text\n",
}


def setup(sandbox: Sandbox) -> None:
    for name, text in REPORTS.items():
        sandbox.write(f"reports/{name}", text)
    for name, text in ARCHIVE.items():
        sandbox.write(f"archive/{name}", text)


def expected() -> tuple[dict[str, object], dict[str, str]]:
    pattern = re.compile(r"^([a-z]+)-(\d{4})-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])\.([a-z]+)$")
    tree = {f"reports/{name}": text for name, text in REPORTS.items()}
    tree.update({f"archive/{name}": text for name, text in ARCHIVE.items()})
    reports = sorted(name for name in REPORTS if "/" not in name)
    found = [(name, pattern.match(name)) for name in reports]
    matched = [(name, match.groups()) for name, match in found if match is not None]
    ignored = sorted(name for name, match in found if match is None)
    old = sorted(((f"{y}-{m}-{d}", name, (y, m, ext)) for name, (_, y, m, d, ext) in matched
                  if f"{y}-{m}-{d}" < CUTOFF), key=lambda item: (item[0], item[1]))
    archived: list[dict[str, str]] = []
    duplicates: list[str] = []
    lines = ""
    for _, name, (year, month, ext) in old:
        stem = name.rsplit(".", 1)[0]
        source = f"reports/{name}"
        target = f"archive/{year}/{month}/{name}"
        if target not in tree:
            final = name
        elif tree[target] == tree[source]:
            del tree[source]
            duplicates.append(name)
            lines += f"{name} (duplicate)\n"
            continue
        else:
            number = 1
            while f"archive/{year}/{month}/{stem}.{number}.{ext}" in tree:
                number += 1
            final = f"{stem}.{number}.{ext}"
        tree[f"archive/{year}/{month}/{final}"] = tree.pop(source)
        archived.append({"from": name, "to": f"{year}/{month}/{final}"})
        lines += f"{name} -> {year}/{month}/{final}\n"
    tree["archive/INDEX.txt"] += lines
    result: dict[str, object] = {
        "archived": archived,
        "duplicates": duplicates,
        "kept": len(matched) - len(old),
        "ignored": ignored,
    }
    return result, tree


def snapshot(root: Path) -> tuple[dict[str, str], set[str]]:
    files = {str(path.relative_to(root)): path.read_text() for path in root.rglob("*") if path.is_file()}
    directories = {str(path.relative_to(root)) for path in root.rglob("*") if path.is_dir()}
    return files, directories


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    wanted, tree = expected()
    if result != wanted:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(wanted)}")
    files, directories = snapshot(sandbox.root)
    if files != tree:
        missing = sorted(set(tree) - set(files))
        extra = sorted(set(files) - set(tree))
        changed = sorted(name for name in set(tree) & set(files) if tree[name] != files[name])
        return Verdict.wrong(f"the files differ: missing {missing}, unexpected {extra}, changed {changed}")
    wanted_directories = {str(parent) for name in tree for parent in map(str, Path(name).parents) if parent != "."}
    if directories != wanted_directories:
        return Verdict.wrong(f"the directories were {sorted(directories)}, not {sorted(wanted_directories)}")
    return Verdict.correct()
