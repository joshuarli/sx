import json
import random
from collections import Counter

from definition import Evidence, Verdict
from sandbox import Sandbox

SERVICES = ("auth", "billing", "checkout", "gateway", "inventory", "search")
CODES = ("E1042", "E1100", "E2001", "E3307", "E4004", "E5150", "E9000")
OTHER_LEVELS = ("INFO", "WARN", "DEBUG")
FILE_COUNT = 14
CRLF_FILES = frozenset({3, 8})
type Count = dict[str, str | int]


def log_files() -> dict[str, str]:
    generator = random.Random(2026)
    files: dict[str, str] = {}
    for number in range(FILE_COUNT):
        newline = "\r\n" if number in CRLF_FILES else "\n"
        lines: list[str] = []
        for minute in range(30 + number * 3):
            stamp = f"2026-03-{number + 1:02d}T10:{minute % 60:02d}:{minute * 7 % 60:02d}Z"
            service = generator.choice(SERVICES)
            roll = generator.random()
            if roll < 0.4:
                code = generator.choice(CODES[: 3 + number % 4])
                detail = "" if minute % 2 else f" request failed after {minute % 5} retries"
                lines.append(f"{stamp} ERROR {service} {code}{detail}")
                if roll < 0.1:
                    lines.append("    at handler.run (server.js:88)")
                    lines.append("    at ERROR.wrap (server.js:12)")
            elif roll < 0.5:
                lines.append(f"{stamp} INFO {service} recovered from ERROR {generator.choice(CODES)}")
            elif roll < 0.55:
                lines.append("")
            elif roll < 0.6:
                lines.append(f"{stamp} ERROR {service}")
            else:
                lines.append(f"{stamp} {generator.choice(OTHER_LEVELS)} {service} handled request {minute}")
        files[f"service-{number:02d}.log"] = newline.join(lines) + newline
    return files


def other_files() -> dict[str, str]:
    return {
        "service-00.log.1": "2026-02-28T23:59:59Z ERROR auth E9999 rotated away\n",
        "notes.txt": "2026-03-01T00:00:00Z ERROR gateway E9998 not a log\n",
        "archive/old.log": "2026-01-01T00:00:00Z ERROR billing E9997 in a subdirectory\n",
    }


def tally(files: dict[str, str]) -> tuple[Counter[str], Counter[str], int]:
    codes: Counter[str] = Counter()
    services: Counter[str] = Counter()
    for text in files.values():
        for line in text.replace("\r", "").split("\n"):
            fields = line.split(" ")
            if len(fields) >= 4 and fields[1] == "ERROR":
                codes[fields[3]] += 1
                services[fields[2]] += 1
    return codes, services, sum(codes.values())


def ordered(counter: Counter[str]) -> list[tuple[str, int]]:
    return sorted(counter.items(), key=lambda item: (-item[1], item[0]))


def expected() -> tuple[str, object]:
    codes, services, errors = tally(log_files())
    lines = [f"files: {FILE_COUNT}", f"errors: {errors}", "by code:"]
    lines += [f"{name}: {count}" for name, count in ordered(codes)]
    lines.append("by service:")
    lines += [f"{name}: {count}" for name, count in ordered(services)]
    result = {
        "files": FILE_COUNT,
        "errors": errors,
        "by_code": [{"code": name, "count": count} for name, count in ordered(codes)],
        "by_service": [{"service": name, "count": count} for name, count in ordered(services)],
    }
    return "\n".join(lines) + "\n", result


def setup(sandbox: Sandbox) -> None:
    for name, text in {**log_files(), **other_files()}.items():
        path = sandbox.root / "logs" / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(text.encode())


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    summary, wanted = expected()
    if result != wanted:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(wanted)}")
    path = sandbox.root / "summary.txt"
    if not path.is_file():
        return Verdict.wrong("summary.txt was not written")
    if path.read_bytes() != summary.encode():
        return Verdict.wrong(f"summary.txt holds {path.read_text()!r}, not {summary!r}")
    present = {
        str(item.relative_to(sandbox.root / "logs")): item.read_bytes().decode()
        for item in (sandbox.root / "logs").rglob("*")
        if item.is_file()
    }
    if present != {**log_files(), **other_files()}:
        return Verdict.wrong("the program changed the files under logs")
    extra = sorted(item.name for item in sandbox.root.iterdir() if item.name not in {"logs", "summary.txt"})
    if extra:
        return Verdict.wrong(f"the program left {extra} in the sandbox")
    return Verdict.correct()
