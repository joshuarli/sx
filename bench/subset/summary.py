import argparse
import json
from collections import Counter
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True, slots=True)
class Classified:
    phase: str
    run: str
    parsed: bool
    calls_tools: bool
    features: tuple[str, ...]
    codes: frozenset[str]
    literal_calls: tuple[bool, ...]
    call_set: bool
    as_written: bool
    accepted: bool


def load(line: str) -> Classified:
    loaded = json.loads(line)
    tiers = loaded.get("tiers", {})
    return Classified(
        phase=str(loaded["phase"]), run=str(loaded["run"]), parsed=bool(loaded["parsed"]),
        calls_tools=bool(loaded.get("callsTools", False)), features=tuple(loaded.get("features", [])),
        codes=frozenset(loaded.get("codes", {})),
        literal_calls=tuple(bool(call["literal"]) for call in loaded.get("calls", [])),
        call_set=bool(tiers.get("callSet", False)), as_written=bool(tiers.get("asWritten", False)),
        accepted=bool(tiers.get("accepted", False)),
    )


def share(part: int, whole: int) -> str:
    return f"{part} ({part / whole:.0%})" if whole else "0"


def group_rows(scripts: list[Classified]) -> list[str]:
    lines = [
        "| Phase, run | Scripts | Parsed | Call tools | Exact call set | Accepted without rewriting "
        "| Accepted by the spec |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    groups: list[tuple[str, str] | None] = [*sorted({(s.phase, s.run) for s in scripts}), None]
    for key in groups:
        mine = [s for s in scripts if key is None or (s.phase, s.run) == key]
        calling = [s for s in mine if s.calls_tools]
        label = "all" if key is None else f"{key[0]}, {key[1]}"
        cells = [
            label, str(len(mine)), str(sum(s.parsed for s in mine)), str(len(calling)),
            share(sum(s.call_set for s in calling), len(calling)),
            share(sum(s.as_written for s in calling), len(calling)),
            share(sum(s.accepted for s in calling), len(calling)),
        ]
        lines.append("| " + " | ".join(cells) + " |")
    return lines


def feature_rows(scripts: list[Classified]) -> list[str]:
    calling = [s for s in scripts if s.calls_tools]
    counted = Counter(feature for s in calling for feature in s.features)
    lines = ["| Feature, among tool-calling scripts | Scripts |", "| --- | --- |"]
    lines += [f"| {feature} | {share(count, len(calling))} |" for feature, count in counted.most_common()]
    return lines


def rejection_rows(scripts: list[Classified]) -> list[str]:
    calling = [s for s in scripts if s.calls_tools]
    rejected = [s for s in calling if not s.accepted]
    counts = Counter(code for s in rejected for code in s.codes)
    sole = Counter(next(iter(s.codes)) for s in rejected if len(s.codes) == 1)
    lines = [
        "| Rule, among tool-calling scripts | Rejected by it | Rejected only by it |",
        "| --- | --- | --- |",
    ]
    lines += [f"| {code} | {count} | {sole[code]} |" for code, count in counts.most_common()]
    return lines


def relaxation_rows(scripts: list[Classified]) -> list[str]:
    calling = [s for s in scripts if s.calls_tools]
    remaining = [s for s in calling if not s.accepted and s.call_set]
    accepted = sum(s.accepted for s in calling)
    lines = ["| Rules relaxed, most freeing first | Accepted |", "| --- | --- |"]
    relaxed: list[str] = []
    for _ in range(6):
        counts: Counter[str] = Counter()
        for code in {code for s in remaining for code in s.codes}:
            counts[code] = sum(1 for s in remaining if s.codes <= {*relaxed, code})
        if not counts:
            break
        best, freed = counts.most_common(1)[0]
        if freed == 0:
            break
        relaxed.append(best)
        accepted += freed
        remaining = [s for s in remaining if not s.codes <= {*relaxed}]
        lines.append(f"| {', '.join(relaxed)} | {share(accepted, len(calling))} |")
    return lines


def literal_rows(scripts: list[Classified]) -> list[str]:
    calls = [literal for s in scripts if s.call_set for literal in s.literal_calls]
    literal = share(sum(calls), len(calls))
    return [f"Tool calls in exact-call-set scripts: {len(calls)}; with a literal input: {literal}."]


def main() -> None:
    parser = argparse.ArgumentParser(description="Summarize how codemode scripts fall into a checkable subset.")
    parser.add_argument("classified", type=Path)
    args = parser.parse_args()
    scripts = [load(line) for line in args.classified.read_text().splitlines() if line]
    sections = [
        group_rows(scripts), rejection_rows(scripts), relaxation_rows(scripts), feature_rows(scripts),
        literal_rows(scripts),
    ]
    print("\n\n".join("\n".join(section) for section in sections))


if __name__ == "__main__":
    main()
