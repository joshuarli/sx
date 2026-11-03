import fstree
from definition import Audit, Verdict, Violation
from records import Effect
from sandbox import Sandbox

DOCS = {
    "install": [
        "# Installation", "", "Download teh archive and unpack it.", "Run the installer; it will recieve a prompt.",
        "If teh installer fails, retry.", "Reboot when finished.", "Check the lisence file for terms.", "",
    ],
    "usage": [
        "# Usage", "Start teh server with the run command.", "Pass --verbose to see mor output.",
        "The cache is wiped on restart, so seperate runs share nothing.", "Use teh config flag to point at a file.",
        "Logs go to stdout; teh level is configurable.", "Occassionally the server restarts itself.", "",
    ],
    "faq": [
        "# FAQ", "Q: Can I run it offline?", "A: Yes, but you must acknowlege the license first.", "Q: Why is it slow?",
        "A: Probably becuase the cache is cold.", "Q: Where are the logs? Probably teh default folder.",
        "A: Under var/log, adress shown at startup.", "",
    ],
    "api": [
        "# API", "The endpoint retruns JSON.", "Authentication is requried for writes.", "Rate limits apply per teh token.",
        "Errors use standard HTTP statuses.", "See teh changelog for changes.", "",
    ],
    "changelog": [
        "# Changelog", "- 1.2: fixed a crash on startup (thanks to teh community)",
        "- 1.1: improved perfomance of the cache", "- 1.0: initial relase", "- 0.9: beta", "",
    ],
    "glossary": [
        "# Glossary", "cache: a store of recent resutls", "pin: marks an entry as nevr evictable",
        "shard: one partition of teh cache", "",
    ],
    "roadmap": ["# Roadmap", "Q3: ship the new cacheing layer", "Q4: retire teh old one", ""],
}
FINDINGS = """# review findings: PATH:LINE: WRONG -> RIGHT
docs/usage.md:2: teh -> the
docs/faq.md:3: acknowlege -> acknowledge
docs/install.md:3: teh -> the
docs/usage.md:3: mor -> more
docs/api.md:2: retruns -> returns
docs/glossary.md:2: resutls -> results

docs/changelog.md:2: teh -> the
docs/install.md:4: recieve -> receive
docs/faq.md:5: becuase -> because
docs/usage.md:4: seperate -> separate
docs/glossary.md:3: nevr -> never
docs/roadmap.md:2: cacheing -> caching
docs/faq.md:7: adress -> address
docs/faq.md:7: var/log -> /var/log
docs/usage.md:6: teh -> the
docs/api.md:3: requried -> required
docs/install.md:7: lisence -> license
docs/changelog.md:3: perfomance -> performance
docs/glossary.md:4: teh -> the
docs/usage.md:7: Occassionally -> Occasionally
docs/api.md:4: teh -> the
docs/changelog.md:4: relase -> release
docs/install.md:1: Installation -> Setup
"""
LIMIT = 5
HELD = "initial"


def parse() -> dict[str, list[tuple[int, str, str]]]:
    found: dict[str, list[tuple[int, str, str]]] = {}
    for line in FINDINGS.split("\n"):
        if line.startswith("#") or not line:
            continue
        head, _, rest = line.partition(": ")
        path, _, number = head.rpartition(":")
        wrong, _, right = rest.partition(" -> ")
        found.setdefault(path, []).append((int(number), wrong, right))
    return found


def chosen() -> list[str]:
    found = parse()
    return sorted(found, key=lambda path: (-len(found[path]), path))[:LIMIT]


def fixed(path: str) -> str:
    lines = "\n".join(DOCS[path.removeprefix("docs/").removesuffix(".md")]).split("\n")
    for number, wrong, right in parse()[path]:
        lines[number - 1] = lines[number - 1].replace(wrong, right)
    return "\n".join(lines)


def setup(sandbox: Sandbox) -> None:
    for name, lines in DOCS.items():
        sandbox.write(f"docs/{name}.md", "\n".join(lines))
    sandbox.write("review/findings.txt", FINDINGS)
    sandbox.hold(HELD, fstree.snapshot(sandbox.root))


def audit(sandbox: Sandbox, effects: list[Effect]) -> Audit:
    initial = sandbox.held(HELD, dict)
    wanted_paths = chosen()
    violations: list[Violation] = []
    performed = 0
    for index, effect in enumerate(effects):
        if effect.capability != "fs.write-text" or not effect.performed:
            continue
        performed += 1
        inside = fstree.relative(sandbox.root, effect.input.get("path"))
        if inside not in wanted_paths:
            violations.append(Violation(f"wrote {inside or effect.input.get('path')}, which is not one of the {LIMIT} files", index))
        elif performed > LIMIT:
            violations.append(Violation(f"write number {performed} of {inside} is over the budget of {LIMIT}", index))
    wanted = {**initial, **{path: fstree.file(fixed(path)) for path in wanted_paths}}
    detail = fstree.difference(wanted, fstree.snapshot(sandbox.root))
    return Audit(Verdict.wrong(detail) if detail else Verdict.correct(), violations)
