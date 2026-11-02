import argparse
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import definition
import effects_reference
import effects_report
import report
import session
import trial
from arms import Arm, require_toolchain
from probe import tokens

PROVIDER_STOP_RERUNS = 2

ARMS: tuple[Arm, ...] = ("sx", "js")
DEFAULT_MODEL = "opencode-go/muse-spark-1.3-contributor"


def run_references(ids: list[str], arm_names: tuple[Arm, ...], root: Path) -> bool:
    all_correct = True
    print(f"{'workflow':24} {'arm':3} {'verdict':8} {'calls':>5} {'total s':>8} {'program s':>9}  detail")
    for workflow_id in ids:
        workflow = definition.load(workflow_id, root)
        for arm in arm_names:
            run, status, detail = trial.reference(workflow, arm)
            all_correct &= status == "correct"
            timing = f"{run.seconds:8.2f} {run.program_seconds:9.2f}"
            print(f"{workflow.id:24} {arm:3} {status:8} {len(run.calls):5} {timing}  {detail}")
    return all_correct


def run_effects_references(ids: list[str], arm_names: tuple[Arm, ...], root: Path) -> bool:
    all_correct = True
    print(f"{'workflow':20} {'arm':3} {'verdict':8} {'effects':>7} {'violations':>10}  detail")
    for workflow_id in ids:
        workflow = definition.load_effects(workflow_id, root)
        for arm in arm_names:
            run = effects_reference.run_reference(workflow, arm)
            good = run.verdict.status == "correct" and not run.violations and run.failure is None
            all_correct &= good
            detail = run.failure or run.verdict.detail
            counts = f"{len(run.effects):7} {len(run.violations):10}"
            print(f"{workflow.id:20} {arm:3} {run.verdict.status:8} {counts}  {detail}")
    return all_correct


def run_sessions(args: argparse.Namespace, ids: list[str], arm_names: tuple[Arm, ...], root: Path) -> bool:
    out = Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    clean = True
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        jobs = [
            pool.submit(
                session.run_measured_session, definition.load_effects(workflow_id, root), arm, args.model,
                args.thinking, sample, out, PROVIDER_STOP_RERUNS,
            )
            for workflow_id in ids
            for arm in arm_names
            for sample in range(args.samples)
        ]
        for job in jobs:
            record = job.result()
            clean &= record.error is None
            statuses = [f"{a.tool}:{a.status}" for a in record.attempts]
            print(
                f"{record.workflow:20} {record.arm:3} {record.sample} {record.verdict.status:8} "
                f"violations {len(record.violations)} {statuses} cost ${record.usage.cost:.4f}"
                f"{'' if record.error is None else ' ERROR ' + record.error}",
                flush=True,
            )
    return clean


def run_trials(args: argparse.Namespace, ids: list[str], arm_names: tuple[Arm, ...], root: Path) -> None:
    out = Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        jobs = [
            pool.submit(
                trial.run_trial,
                definition.load(workflow_id, root), arm, args.model, args.thinking, args.guide, sample, out,
                ts_notes=args.ts_notes,
            )
            for workflow_id in ids
            for arm in arm_names
            for sample in range(args.samples)
        ]
        for job in jobs:
            finished = job.result()
            statuses = [a.status for a in finished.attempts]
            line = f"{finished.task:24} {finished.arm:3} {finished.sample} {statuses} tokens {tokens(finished)}"
            print(line, flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(description="Run workflows in Sx and in TypeScript on Bun.")
    parser.add_argument("mode", choices=["reference", "trials", "report", "sessions", "effects-report"])
    parser.add_argument(
        "--set", choices=list(definition.SETS), default="dev", help="The development, blind, blind2, or effects set"
    )
    parser.add_argument("--workflow", action="append", help="Workflow ids; every workflow of the set by default")
    parser.add_argument("--arm", action="append", choices=[*ARMS, "both"])
    parser.add_argument("--model", default=DEFAULT_MODEL)
    parser.add_argument("--thinking", default="medium")
    parser.add_argument("--guide", default="standard")
    parser.add_argument("--ts-notes", action="store_true", help="Give the TypeScript arm the tools' behavior notes")
    parser.add_argument("--samples", type=int, default=2)
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    root = definition.SETS[args.set]
    ids = args.workflow or definition.available(root)
    arm_names = ARMS if not args.arm or "both" in args.arm else tuple(arm for arm in ARMS if arm in args.arm)
    effects_set = args.set == definition.EFFECTS_SET
    if args.mode in ("sessions", "effects-report") and not effects_set:
        parser.error(f"{args.mode} reads the {definition.EFFECTS_SET!r} set; pass --set {definition.EFFECTS_SET}")
    if args.mode in ("trials", "report") and effects_set:
        parser.error(f"the {definition.EFFECTS_SET!r} set runs with sessions and effects-report")
    if args.mode == "effects-report":
        if args.out is None:
            parser.error("effects-report needs --out")
        print(effects_report.render(Path(args.out).resolve()))
        return
    if args.mode == "report":
        if args.out is None:
            parser.error("report needs --out")
        print(report.render(Path(args.out).resolve(), root))
        return
    if "js" in arm_names and args.mode != "sessions":
        require_toolchain()
    if args.mode == "reference":
        runner = run_effects_references if effects_set else run_references
        raise SystemExit(0 if runner(ids, arm_names, root) else 1)
    if args.out is None:
        parser.error(f"{args.mode} needs --out")
    if args.mode == "sessions":
        raise SystemExit(0 if run_sessions(args, ids, arm_names, root) else 1)
    run_trials(args, ids, arm_names, root)


if __name__ == "__main__":
    main()
