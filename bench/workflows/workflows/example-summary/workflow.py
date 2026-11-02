from definition import Evidence, Verdict
from sandbox import Sandbox

FILES = {"alpha.txt": "abc", "beta.txt": "hello world\n", "gamma.log": "x" * 100}


def setup(sandbox: Sandbox) -> None:
    for name, text in FILES.items():
        sandbox.write(f"data/{name}", text)
    sandbox.write("data/nested/inner.txt", "not counted")


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    count, size = len(FILES), sum(len(text) for text in FILES.values())
    if result != {"count": count, "bytes": size}:
        return Verdict.wrong(f"the result was {result!r}, not count {count} and bytes {size}")
    summary = sandbox.root / "summary.txt"
    if not summary.is_file():
        return Verdict.wrong("summary.txt was not written")
    wanted = f"files: {count}\nbytes: {size}\n"
    if summary.read_text() != wanted:
        return Verdict.wrong(f"summary.txt holds {summary.read_text()!r}, not {wanted!r}")
    extra = sorted(path.name for path in sandbox.root.iterdir() if path.name not in {"data", "summary.txt"})
    if extra:
        return Verdict.wrong(f"the program left {extra} in the sandbox")
    return Verdict.correct()
