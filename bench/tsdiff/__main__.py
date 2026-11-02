from __future__ import annotations

import argparse
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from tsdiff.build import SCRATCH, generated_case, mutant_case
from tsdiff.compare import SxUnsupportedLanguageError
from tsdiff.corpus import build_corpus
from tsdiff.run import case_directories, run_all, summarize


def command_generate(args: argparse.Namespace) -> int:
    out = Path(args.out)

    def write(seed: int) -> None:
        made = mutant_case(seed) if args.mutants else generated_case(seed)
        made.write(out / made.name)

    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        list(pool.map(write, range(args.seed, args.seed + args.count)))
    print(f"wrote {args.count} cases to {out}")
    return 0


def command_corpus(args: argparse.Namespace) -> int:
    summary = build_corpus(Path(args.effects), Path(args.real), Path(args.out))
    for line in summary:
        print(line)
    return 0


def command_run(args: argparse.Namespace) -> int:
    directories = case_directories(Path(p) for p in args.cases)
    try:
        binary = Path(args.sx) if args.sx else None
        report = run_all(directories, shim_only=args.shim_only, binary=binary, jobs=args.jobs)
    except SxUnsupportedLanguageError as error:
        print(f"tsdiff: {error}", file=sys.stderr)
        return 2
    for line in summarize(report):
        print(line)
    return 0 if report.clean() else 1


def main() -> int:
    parser = argparse.ArgumentParser(description="Differential tests of checkable TypeScript: Bun against sx.")
    commands = parser.add_subparsers(dest="command", required=True)
    generate = commands.add_parser("generate", help="Write generated cases, or mutants just outside the subset.")
    generate.add_argument("--count", type=int, default=50)
    generate.add_argument("--seed", type=int, default=0)
    generate.add_argument("--mutants", action="store_true")
    generate.add_argument("--jobs", type=int, default=8)
    generate.add_argument("--out", default=str(SCRATCH / "generated"))
    generate.set_defaults(handler=command_generate)
    corpus = commands.add_parser("corpus", help="Build runnable cases from the codemode corpus.")
    corpus.add_argument("--effects", required=True)
    corpus.add_argument("--real", required=True)
    corpus.add_argument("--out", default=str(SCRATCH / "corpus"))
    corpus.set_defaults(handler=command_corpus)
    run = commands.add_parser("run", help="Compare Bun and sx on case directories.")
    run.add_argument("cases", nargs="+")
    run.add_argument("--sx", help="The sx binary; defaults to $SX_BIN.")
    run.add_argument("--shim-only", action="store_true", help="Run only the Bun side and report its outcomes.")
    run.add_argument("--jobs", type=int, default=8)
    run.set_defaults(handler=command_run)
    args = parser.parse_args()
    return int(args.handler(args))


if __name__ == "__main__":
    raise SystemExit(main())
