import json
import math
import os
import platform
import statistics
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from sx_repomod.declarations import STATUS_OFFLINE

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
FIXTURES = ROOT / "fixtures" / "run"
SCRATCH = ROOT / ".work" / "tmp" / "kernel-bench"
ROUNDS = 9
BASELINES = 5
TARGET_SECONDS = 0.4
GATE_PATIENCE_SECONDS = 120
SUSPEND_CALLS = 200
WIDE_ITEMS = 160

PURE = """(let [xs [1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20
          21 22 23 24 25 26 27 28 29 30 31 32 33 34 35 36 37 38 39 40]
      total (loop {:max 3000} [i 0 acc 0]
              (if (< i 2000)
                (recur (+ i 1) (+ acc (rem (* i i) 7)))
                acc))
      listed (reduce (fn [a x] (+ a (rem (* x x) 7))) 0 (filter (fn [x] (> x 3)) (map (fn [x] (+ x 1)) xs)))]
  (+ total listed))"""

SUSPEND = f"""(loop {{:max {SUSPEND_CALLS + 100}}} [i 0 acc 0]
  (if (< i {SUSPEND_CALLS})
    (let [item (call store.get {{:id "a"}})]
      (recur (+ i 1) (+ acc (get item :size))))
    acc))"""

WIDE = (
    "(par-map {:limit 16} (fn [id] (call store.get {:id id})) ["
    + " ".join(f'"i{n}"' for n in range(WIDE_ITEMS))
    + "])"
)


@dataclass(frozen=True)
class Row:
    operation: str
    unit: str
    divisor: int
    protocol: bool = False


@dataclass
class Sample:
    seconds: list[float] = field(default_factory=list)
    discarded: int = 0


class GateDeferredError(Exception):
    pass


def bundle() -> tuple[dict[str, Any], int]:
    declarations = json.loads((FIXTURES / "declarations.json").read_text())
    corpus = []
    for path in sorted(FIXTURES.glob("*.json")):
        if path.name == "declarations.json":
            continue
        case = json.loads(path.read_text())
        if "rejected" in case["expect"] or "steps" in case:
            continue
        corpus.append(
            {
                "name": path.stem,
                "source": case["program"],
                "declarations": declarations,
                "allow": case["allow"],
                "budgets": case.get("budgets", {}),
                "inputs": {},
                "responses": case["responses"],
            }
        )
    recorded = json.loads((HERE / "status-offline.json").read_text())
    status = {
        "name": "status-offline",
        "source": STATUS_OFFLINE.source,
        "declarations": STATUS_OFFLINE.declarations,
        "allow": sorted(STATUS_OFFLINE.allow),
        "budgets": {},
        "inputs": recorded["inputs"],
        "responses": recorded["responses"],
    }

    def synthetic(name: str, source: str) -> dict[str, Any]:
        return {
            "name": name,
            "source": source,
            "declarations": declarations,
            "allow": ["store.get"],
            "budgets": {"calls": 1000, "steps": 20_000_000, "applications": 2_000_000},
            "inputs": {},
            "responses": [],
        }

    synthetics = {
        "pure": synthetic("pure", PURE),
        "suspend": synthetic("suspend", SUSPEND),
        "wide": synthetic("wide", WIDE),
    }
    return {"corpus": corpus, "status": [status], "synthetic": synthetics}, len(corpus)


def release_binaries() -> tuple[Path, Path]:
    built = subprocess.run(
        [
            "cargo", "build", "--release", "--quiet", "--message-format=json",
            "-p", "sx-cli", "--bin", "sx", "--example", "kernel_bench",
        ],
        cwd=ROOT / "rust",
        capture_output=True,
        text=True,
        check=True,
    )
    found: dict[str, Path] = {}
    for line in built.stdout.splitlines():
        artifact = json.loads(line)
        if artifact.get("reason") == "compiler-artifact" and artifact.get("executable"):
            found[artifact["target"]["name"]] = Path(artifact["executable"])
    return found["sx"], found["kernel_bench"]


def gate(arguments: list[str]) -> bool:
    try:
        done = subprocess.run(
            ["perf-gate-ready", *arguments], capture_output=True, timeout=GATE_PATIENCE_SECONDS, check=False
        )
    except subprocess.TimeoutExpired:
        return False
    return done.returncode == 0


def cpu_seconds(command: list[str], environment: dict[str, str]) -> float:
    process = subprocess.Popen(command, env=environment, stdout=subprocess.PIPE, text=True)
    assert process.stdout is not None
    _, status, usage = os.wait4(process.pid, 0)
    process.returncode = os.waitstatus_to_exitcode(status)
    if process.returncode != 0:
        raise RuntimeError(f"{command} exited with {process.returncode}")
    reported = process.stdout.read().strip()
    process.stdout.close()
    return usage.ru_utime + usage.ru_stime + (float(reported) if reported else 0.0)


def gated_cpu(command: list[str], environment: dict[str, str], sample: Sample) -> float | None:
    for _ in range(6):
        if not gate([]):
            sample.discarded += 1
            continue
        seconds = cpu_seconds(command, environment)
        if gate(["-1"]):
            return seconds
        sample.discarded += 1
    return None


def calibrate(command: list[str], environment: dict[str, str]) -> int:
    base = statistics.median(cpu_seconds([*command, "0"], environment) for _ in range(3))
    count = 1
    while True:
        spent = statistics.median(cpu_seconds([*command, str(count)], environment) for _ in range(3)) - base
        if spent >= 0.1 or count >= 1 << 24:
            return max(1, math.ceil(TARGET_SECONDS * count / max(spent, 1e-9)))
        count *= 4


def summarize(values: list[float]) -> str:
    if not values:
        return "deferred"
    return f"{statistics.median(values):.2f} ({min(values):.2f}-{max(values):.2f})"


def main() -> int:
    SCRATCH.mkdir(parents=True, exist_ok=True)
    data, corpus_size = bundle()
    path = SCRATCH / "bundle.json"
    path.write_text(json.dumps(data))
    kernel, bench = release_binaries()
    environment = {**os.environ, "SX_KERNEL": str(kernel), "PYTHONDONTWRITEBYTECODE": "1"}
    worker = [sys.executable, str(HERE / "worker.py"), str(path)]
    native = [str(bench), str(path)]
    rows = [
        Row("parse-corpus", f"per program, {corpus_size} fixture programs", corpus_size),
        Row("check-corpus", f"per program, {corpus_size} fixture programs", corpus_size),
        Row("run-corpus", f"per program, fixture host, {corpus_size} fixture programs", corpus_size),
        Row("parse-status", "per run, status-offline.sx", 1),
        Row("check-status", "per run, status-offline.sx", 1),
        Row("run-status", "per run, 70 recorded responses", 1),
        Row("run-pure", "per run, 59,000 Python machine steps", 1),
        Row("run-suspend", f"per suspension and resume, {SUSPEND_CALLS} sequential calls", SUSPEND_CALLS),
        Row("run-wide", f"per request, {WIDE_ITEMS} calls in par-map of 16", WIDE_ITEMS),
        Row("protocol-suspend", f"per call, {SUSPEND_CALLS} sequential calls", SUSPEND_CALLS, protocol=True),
    ]
    started = time.strftime("%F %T")
    print(f"{platform.platform()} {platform.machine()}, Python {platform.python_version()}, started {started}")
    print("| operation | unit | Python kernel us (min-max) | Rust kernel us (min-max) | ratio | samples/discarded |")
    print("| --- | --- | --- | --- | --- | --- |")
    results: dict[str, dict[str, float]] = {}
    for row in rows:
        commands = {"python": worker} if row.protocol else {"python": worker, "rust": native}
        counts = {name: calibrate([*command, row.operation], environment) for name, command in commands.items()}
        samples = {name: Sample() for name in commands}
        baselines: dict[str, float] = {}
        for name, command in commands.items():
            taken = [gated_cpu([*command, row.operation, "0"], environment, samples[name]) for _ in range(BASELINES)]
            usable = [t for t in taken if t is not None]
            if not usable:
                raise GateDeferredError(row.operation)
            baselines[name] = statistics.median(usable)
        micros: dict[str, list[float]] = {name: [] for name in commands}
        for _ in range(ROUNDS):
            for name, command in commands.items():
                seconds = gated_cpu([*command, row.operation, str(counts[name])], environment, samples[name])
                if seconds is not None:
                    micros[name].append((seconds - baselines[name]) / counts[name] / row.divisor * 1e6)
        median = {name: statistics.median(values) for name, values in micros.items() if values}
        results[row.operation] = median
        python, rust = micros["python"], micros.get("rust", [])
        ratio = f"{median['python'] / median['rust']:.0f}x" if python and rust else ""
        used = "/".join(f"{len(micros[name])}/{samples[name].discarded}" for name in commands)
        native_cell = summarize(rust) if "rust" in commands else "n/a"
        print(f"| {row.operation} | {row.unit} | {summarize(python)} | {native_cell} | {ratio} | {used} |", flush=True)
    suspend, protocol = results.get("run-suspend"), results.get("protocol-suspend")
    if suspend is not None and protocol is not None:
        print()
        print(f"Python kernel with a Python host: {suspend['python']:.1f} us per call.")
        print(f"Rust kernel in process with a Rust host: {suspend['rust']:.1f} us per call.")
        print(f"Rust kernel over the protocol with a Python host: {protocol['python']:.1f} us per call together.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
