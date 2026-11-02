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
    literal_calls: tuple[bool, ...]
    call_set: bool
    as_written: bool
    with_rewriting: bool


def load(line: str) -> Classified:
    loaded = json.loads(line)
    tiers = loaded.get("tiers", {})
    return Classified(
        phase=str(loaded["phase"]), run=str(loaded["run"]), parsed=bool(loaded["parsed"]),
        calls_tools=bool(loaded.get("callsTools", False)), features=tuple(loaded.get("features", [])),
        literal_calls=tuple(bool(call["literal"]) for call in loaded.get("calls", [])),
        call_set=bool(tiers.get("callSet", False)), as_written=bool(tiers.get("asWritten", False)),
        with_rewriting=bool(tiers.get("withRewriting", False)),
    )


def share(part: int, whole: int) -> str:
    return f"{part} ({part / whole:.0%})" if whole else "0"


def group_rows(scripts: list[Classified]) -> list[str]:
    lines = [
        "| Phase, run | Scripts | Parsed | Call tools | Exact call set | Lowerable as written "
        "| Lowerable with rewriting |",
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
            share(sum(s.with_rewriting for s in calling), len(calling)),
        ]
        lines.append("| " + " | ".join(cells) + " |")
    return lines


def feature_rows(scripts: list[Classified]) -> list[str]:
    calling = [s for s in scripts if s.calls_tools]
    counted = Counter(feature for s in calling for feature in s.features)
    lines = ["| Feature, among tool-calling scripts | Scripts |", "| --- | --- |"]
    lines += [f"| {feature} | {share(count, len(calling))} |" for feature, count in counted.most_common()]
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
    print("\n".join([*group_rows(scripts), "", *feature_rows(scripts), "", *literal_rows(scripts)]))


if __name__ == "__main__":
    main()
