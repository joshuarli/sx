import csv
import io
import json
from typing import Any

from definition import Evidence, Verdict
from sandbox import Sandbox

PACKAGES: dict[str, str | None] = {
    "alpha": '{"name": "alpha", "version": "1.0.0", "license": "MIT",\n'
    ' "author": "Ada Lovelace <ada@example.com>",\n'
    ' "dependencies": {"a": "1", "b": "^2"}, "devDependencies": {"c": "1"}}',
    "beta": '{"name": "beta", "version": "0.4.1", "license": {"type": "ISC", "url": "https://isc.example"},\n'
    ' "author": {"name": "Doe, Jane", "email": "jane@example.com"}}',
    "gamma": '{"name": "gamma", "version": "2.3.0", "licenses": [{"type": "BSD-3-Clause"}, {"type": "MIT"}],\n'
    ' "author": "Zo\\u00eb Quinn", "dependencies": {"x": "1"}}',
    "delta": '{"name": "delta", "version": "1.0.0", "private": true, "license": "MIT"}',
    "epsilon": '{"name": "epsilon", "version": "1.0.0", "license": "MIT",}',
    "zeta": '{"version": "3.1.4", "license": "MIT", "dependencies": {}, "private": false}',
    "eta-core": '{"name": "Eta", "version": "1.2.3", "license": "MIT",\n'
    ' "author": "Rob \\"Bob\\" Smith <rob@example.org>"}',
    "theta-a": '{"name": "theta", "version": "2.0.0", "license": "Apache-2.0", "author": "Thea"}',
    "theta-b": '{"name": "theta", "version": "1.0.0", "license": "Apache-2.0", "author": null}',
    "iota": '{"name": "iota", "version": "0.0.1", "licenses": [], "private": false}',
    "kappa": None,
    "lambda": "",
    "mu": '{"name": "mu", "version": "0.1.0", "license": "MIT", "author": "Mu <mu@example.org> (https://mu.example)",\n'
    ' "dependencies": {"p": "1", "q": "2", "r": "3"}, "devDependencies": {"s": "1", "t": "2"}}',
    "nu": '{"name": "nu", "version": "5.0.0", "license": null, "licenses": [{"type": "Zlib"}],\n'
    ' "author": "  Nu  <n@e.org>"}',
    "omega": '{"name": "omega", "version": "1.0',
    "psi": '{"name": "psi", "version": "9.9.9", "license": "MIT", "author": "Psi", "private": true}',
    "rho": '{"name": "Rho", "version": "1.0.0", "license": "Apache-2.0", "author": {"name": "R. Ho"},'
    ' "devDependencies": {"a": "1"}}',
}


def setup(sandbox: Sandbox) -> None:
    sandbox.write("packages/README.md", "Packages live here.\n")
    for directory, text in PACKAGES.items():
        if text is None:
            sandbox.directory("packages", directory)
        else:
            sandbox.write(f"packages/{directory}/package.json", text)


def license_of(manifest: dict[str, Any]) -> str:
    value = manifest.get("license")
    if isinstance(value, str):
        return value
    if isinstance(value, dict):
        return str(value["type"])
    licenses = manifest.get("licenses")
    if isinstance(licenses, list) and licenses:
        return str(licenses[0]["type"])
    return "UNLICENSED"


def author_of(manifest: dict[str, Any]) -> str:
    value = manifest.get("author")
    if isinstance(value, str):
        return str(value.split(" <")[0]).strip()
    if isinstance(value, dict):
        return str(value["name"])
    return "unknown"


def expected() -> tuple[str, dict[str, object]]:
    rows: list[tuple[str, str, str, str, str, int, int]] = []
    states: dict[str, list[str]] = {"private": [], "invalid": [], "no_manifest": []}
    for directory, text in PACKAGES.items():
        if text is None:
            states["no_manifest"].append(directory)
            continue
        try:
            manifest = json.loads(text)
        except json.JSONDecodeError:
            states["invalid"].append(directory)
            continue
        if manifest.get("private") is True:
            states["private"].append(directory)
            continue
        rows.append((
            manifest.get("name") or directory, directory, manifest.get("version", ""), license_of(manifest),
            author_of(manifest), len(manifest.get("dependencies") or {}), len(manifest.get("devDependencies") or {}),
        ))
    rows.sort(key=lambda row: (row[0], row[1]))
    buffer = io.StringIO()
    writer = csv.writer(buffer, lineterminator="\n")
    writer.writerow(["name", "version", "license", "author", "deps", "dev_deps"])
    for name, _, version, licence, author, deps, dev in rows:
        writer.writerow([name, version, licence, author, deps, dev])
    counts: dict[str, int] = {}
    for row in rows:
        counts[row[3]] = counts.get(row[3], 0) + 1
    ranked = sorted(counts.items(), key=lambda item: (-item[1], item[0]))
    result: dict[str, object] = {
        "rows": len(rows),
        **{key: sorted(value) for key, value in states.items()},
        "licenses": [{"license": name, "count": count} for name, count in ranked],
    }
    return buffer.getvalue(), result


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    text, wanted = expected()
    if result != wanted:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(wanted)}")
    report = sandbox.root / "report.csv"
    if not report.is_file():
        return Verdict.wrong("report.csv was not written")
    if report.read_bytes() != text.encode():
        return Verdict.wrong(f"report.csv holds {report.read_bytes()!r}, not {text.encode()!r}")
    names = sorted(path.name for path in sandbox.root.iterdir())
    if names != ["packages", "report.csv"]:
        return Verdict.wrong(f"the sandbox holds {names}")
    for directory, original in PACKAGES.items():
        manifest = sandbox.root / "packages" / directory / "package.json"
        if (manifest.read_text() if manifest.is_file() else None) != original:
            return Verdict.wrong(f"the package {directory} was changed")
    return Verdict.correct()
