from __future__ import annotations

import argparse
import sys
from collections import Counter
from collections.abc import Generator
from concurrent.futures import ProcessPoolExecutor
from contextlib import contextmanager
from pathlib import Path

from .differential import ROOT, Totals, ir_classes, make_case, registry, run_case, run_chunk, save_finding, scratch
from .minimize import minimize, triage_one
from .rust import Rust


def report(totals: Totals) -> None:
    print(f"cases {totals.cases}  mutated {totals.mutated}  requests answered {totals.requests}")
    print("programs per participant", dict(sorted(totals.participants.items())))
    print("status", dict(totals.status))
    print("outcomes", dict(totals.outcomes.most_common()))
    print("rejection codes", dict(totals.rejected_codes.most_common()))
    accepted = totals.status["agreed"] + totals.status["finding"]
    print(f"coverage by IR class over {accepted} accepted programs (present / executed):")
    for name in ir_classes():
        print(f"  {name:16} {totals.nodes_static[name]:6} {totals.nodes_run[name]:6}")
    ops = sorted(totals.ops_static)
    print("prelude operators present:", len(ops), "executed:", len(totals.ops_run))
    print("  never executed:", sorted(set(totals.ops_static) - set(totals.ops_run)))


@contextmanager
def closing_rust() -> Generator[Rust]:
    rust = Rust(scratch())
    try:
        yield rust
    finally:
        rust.reset()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python -m tests.fuzz")
    parser.add_argument("--seed", default="1")
    parser.add_argument("--count", type=int, default=300)
    parser.add_argument("--start", type=int, default=0)
    parser.add_argument("--jobs", type=int, default=1)
    parser.add_argument("--mutate-rate", type=float, default=0.1)
    parser.add_argument("--keep", type=Path, default=ROOT / ".work" / "tmp" / "fuzz" / "findings")
    parser.add_argument("--fast", action="store_true", help="the deterministic slice: 300 programs, seed fast")
    parser.add_argument("--show", type=int, help="print the program for one index and run it")
    parser.add_argument("--triage", type=int, default=0, help="minimize up to this many findings per kind")
    parser.add_argument("--minimize", type=int, help="shrink the failing program for one index")
    args = parser.parse_args(argv)
    if args.fast:
        args.seed, args.count, args.start, args.jobs = "fast", 300, 0, 4
    if args.show is not None:
        known = registry()
        case = make_case(args.seed, args.show, known, args.mutate_rate)
        print(case.source)
        with closing_rust() as rust:
            result = run_case(case, known, rust)
        print(result.status, result.outcome, result.finding)
        if result.finding is not None:
            save_finding(case, result, args.keep)
        return 0
    if args.minimize is not None:
        known = registry()
        case = make_case(args.seed, args.minimize, known, args.mutate_rate)
        with closing_rust() as rust:
            small = minimize(case, known, rust)
            result = run_case(small, known, rust)
        print(small.source)
        print(result.status, result.outcome, result.finding)
        save_finding(small, result, args.keep / "minimized")
        return 0
    indexes = list(range(args.start, args.start + args.count))
    totals = Totals()
    if args.jobs <= 1:
        totals = run_chunk(args.seed, indexes, args.mutate_rate, args.keep)
    else:
        chunks = [indexes[i :: args.jobs * 4] for i in range(args.jobs * 4)]
        with ProcessPoolExecutor(args.jobs) as pool:
            for part in pool.map(
                run_chunk,
                [args.seed] * len(chunks),
                chunks,
                [args.mutate_rate] * len(chunks),
                [args.keep] * len(chunks),
            ):
                totals.merge(part)
    report(totals)
    groups: dict[str, list[int]] = {}
    for index, finding in sorted(totals.findings, key=lambda item: item[0]):
        groups.setdefault(finding.signature, []).append(index)
    for signature, found in groups.items():
        print(f"FINDING x{len(found)} first index {found[0]}: {signature}")
    print(f"{len(totals.findings)} findings, {len(groups)} distinct")
    if args.triage:
        chosen: list[int] = []
        per_kind: Counter[str] = Counter()
        for index, finding in sorted(totals.findings, key=lambda item: item[0]):
            if per_kind[finding.kind] < args.triage:
                per_kind[finding.kind] += 1
                chosen.append(index)
        with ProcessPoolExecutor(max(1, args.jobs)) as pool:
            seeds, rates = [args.seed] * len(chosen), [args.mutate_rate] * len(chosen)
            for index, text in zip(chosen, pool.map(triage_one, seeds, chosen, rates), strict=True):
                print(f"--- index {index}\n{text}")
    return 1 if totals.findings else 0


if __name__ == "__main__":
    sys.exit(main())
