import argparse
from pathlib import Path

import definition
import trial
from arms import Arm, require_toolchain
from probe import tokens

ARMS: tuple[Arm, ...] = ("sx", "js")
DEFAULT_MODEL = "opencode-go/muse-spark-1.3-contributor"


def run_references(ids: list[str], arm_names: tuple[Arm, ...]) -> bool:
    all_correct = True
    print(f"{'workflow':24} {'arm':3} {'verdict':8} {'calls':>5} {'total s':>8} {'program s':>9}  detail")
    for workflow_id in ids:
        workflow = definition.load(workflow_id)
        for arm in arm_names:
            run, status, detail = trial.reference(workflow, arm)
            all_correct &= status == "correct"
            timing = f"{run.seconds:8.2f} {run.program_seconds:9.2f}"
            print(f"{workflow.id:24} {arm:3} {status:8} {len(run.calls):5} {timing}  {detail}")
    return all_correct


def run_trials(args: argparse.Namespace, ids: list[str], arm_names: tuple[Arm, ...]) -> None:
    out = Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    for workflow_id in ids:
        workflow = definition.load(workflow_id)
        for arm in arm_names:
            for sample in range(args.samples):
                finished = trial.run_trial(workflow, arm, args.model, args.thinking, args.guide, sample, out)
                statuses = [a.status for a in finished.attempts]
                print(f"{workflow.id:24} {arm:3} {sample} {statuses} tokens {tokens(finished)}", flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(description="Run workflows in Sx and in TypeScript on Bun.")
    parser.add_argument("mode", choices=["reference", "trials"])
    parser.add_argument("--workflow", action="append", help="Workflow ids; every workflow by default")
    parser.add_argument("--arm", action="append", choices=list(ARMS))
    parser.add_argument("--model", default=DEFAULT_MODEL)
    parser.add_argument("--thinking", default="medium")
    parser.add_argument("--guide", default="standard")
    parser.add_argument("--samples", type=int, default=2)
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()
    ids = args.workflow or definition.available()
    arm_names = ARMS if not args.arm else tuple(arm for arm in ARMS if arm in args.arm)
    if "js" in arm_names:
        require_toolchain()
    if args.mode == "reference":
        raise SystemExit(0 if run_references(ids, arm_names) else 1)
    if args.out is None:
        parser.error("trials need --out")
    run_trials(args, ids, arm_names)


if __name__ == "__main__":
    main()
