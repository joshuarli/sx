import json
import os
import re

from definition import Evidence, Verdict
from sandbox import Sandbox

OLD = 1_600_000_000
TEXTS: dict[str, bytes] = {
    "src/main.py": b"import os\t \r\n\r\n\tdef run():\r\n\t\treturn 1  \r\n\r\n\r\n",
    "src/util.py": b"def helper():\n    return 2\n",
    "src/vendor.txt": b"vendored   \r\n\tkeep me\r\n\r\n",
    "src/notes.txt": b"first\r\nsecond \r\nthird",
    "src/blob.txt": b"caf\xe9 \xff\xfe broken   \n",
    "src/data.bin": b"\x00\x01 \r\n\x02\t",
    "src/readme.MD": b"Upper case extension   \r\n\r\n",
    "src/sub/deep.py": b"x = 1   \n\n\n",
    "docs/guide.md": b"# Guide  \n\nSome text.\t\n\n\n\n",
    "docs/CHANGELOG.md": b"## 1.0   \r\n\r\n- first   \r\n\r\n",
    "docs/empty.txt": b"  \n\n\t\n",
    "docs/zero.txt": b"",
    "docs/tabs.md": b"list:\n\t- item\twith tab\n\t\t- nested   \nplain\n",
    "docs/ok.md": b"# Fine\n\nAll good.\n",
    "docs/spaces.txt": b"keep  inner   spaces\n   indented\n",
    ".tidyignore": b"# vendored and generated\nsrc/vendor.txt\n\n  docs/CHANGELOG.md   \nsrc/missing.txt\n",
}
LINK = "docs/latest.md"
IGNORED = ("docs/CHANGELOG.md", "src/vendor.txt")
SKIPPED = ("src/blob.txt",)


def tidy(data: bytes) -> bytes:
    text = data.decode().replace("\r\n", "\n")
    lines = [re.sub(r"[ \t]+$", "", line) for line in text.split("\n")]
    lines = [re.sub(r"^\t+", lambda m: "    " * len(m.group()), line) for line in lines]
    body = "\n".join(lines).rstrip("\n")
    return (body + "\n" if body else "").encode()


def considered() -> list[str]:
    return sorted(
        path
        for path in TEXTS
        if path.count("/") == 1 and path.split("/")[0] in {"src", "docs"} and path.endswith((".md", ".txt", ".py"))
    )


def expected() -> tuple[dict[str, bytes], dict[str, object]]:
    final = dict(TEXTS)
    changed: list[str] = []
    unchanged = 0
    delta = 0
    for path in considered():
        if path in IGNORED or path in SKIPPED:
            continue
        tidied = tidy(TEXTS[path])
        if tidied == TEXTS[path]:
            unchanged += 1
        else:
            changed.append(path)
            final[path] = tidied
            delta += len(tidied) - len(TEXTS[path])
    result: dict[str, object] = {
        "changed": changed,
        "unchanged": unchanged,
        "ignored": list(IGNORED),
        "skipped": list(SKIPPED),
        "delta": delta,
    }
    return final, result


def setup(sandbox: Sandbox) -> None:
    for path, data in TEXTS.items():
        target = sandbox.root / "project" / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(data)
        os.utime(target, (OLD, OLD))
    os.symlink("guide.md", sandbox.root / "project" / LINK)


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    final, wanted = expected()
    if result != wanted:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(wanted)}")
    project = sandbox.root / "project"
    present = {
        str(item.relative_to(project)): item
        for item in project.rglob("*")
        if item.is_file() and not item.is_symlink()
    }
    if sorted(present) != sorted(final):
        return Verdict.wrong(f"the project holds the files {sorted(present)}")
    for path, item in present.items():
        if item.read_bytes() != final[path]:
            return Verdict.wrong(f"{path} holds {item.read_bytes()!r}, not {final[path]!r}")
        rewritten = final[path] != TEXTS[path]
        if not rewritten and item.stat().st_mtime_ns != OLD * 1_000_000_000:
            return Verdict.wrong(f"{path} was written although it needed no change")
    link = project / LINK
    if not link.is_symlink() or os.readlink(link) != "guide.md":
        return Verdict.wrong(f"{LINK} is no longer a symbolic link to guide.md")
    extra = sorted(item.name for item in sandbox.root.iterdir() if item.name != "project")
    if extra:
        return Verdict.wrong(f"the program left {extra} in the sandbox")
    return Verdict.correct()
