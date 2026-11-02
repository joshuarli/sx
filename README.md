# Sx

Sx is a small typed language for model-written programs that coordinate tools. A host sees every
tool a program can call before it runs, and the program suspends at each call for the host to
answer. [plan.md](plan.md) describes the current campaign, [docs/language.md](docs/language.md)
specifies the language, and [docs/ROADMAP.md](docs/ROADMAP.md) describes the direction.

## Get started

Use Python 3.15, [uv](https://docs.astral.sh/uv/), and Node for the benchmark checker.

```sh
uv sync
uv run sx help
uv run sx help retry
```

## Layout

This is a uv workspace.

| Path | Contents |
| --- | --- |
| `src/sx` | The frozen Python kernel: reader, checker, machine, registry, help. |
| `rust` | The Rust kernel, cores, and `sx` binary. New work lands here. |
| `lean` | The Lean reference semantics. New work lands here. |
| `cores/text`, `cores/semver` | Pure cores that register through the Python API. |
| `cores/git`, `cores/github` | Decision cores: a checkout as `Missing | Invalid | OriginMismatch | Detached | OnBranch`, and CI aggregation. |
| `hosts/repomod` | `sx-repomod`: capabilities that wrap repomod, and the `discover` and `status --offline` programs, run on the Rust binary. |
| `bench/codemode` | The model probe. |

## The Python prototype is frozen

`src/sx` and the Python cores are frozen as of commit `15b6ed6`, the parent of the commit that records
this. The Rust port passed its gates, so Rust and Lean are now ahead.

- `src/sx` stays as an executable specification. It must keep passing every unmarked fixture in
  `fixtures/`, and the Rust gates still compare Rust with live Python on all of them.
- A bug in frozen behavior is fixed in Rust, Lean, and Python together, while Python can still run the
  case. Its fixture then pins the fix in all three.
- A new feature lands in Rust and Lean only. Its fixtures are marked so Python is not asked to run them:
  `"python": false` in a `fixtures/run` case, and `; python: no` in a `fixtures/check` or
  `fixtures/read` program. See the fixture READMEs for the exact rules. The fuzzer shows a program only
  to the participants that support its features.

The kernel and the cores never import repomod. Only `hosts/repomod` does, and it finds repomod on
`PYTHONPATH`, not through a dependency, so that no tool builds or writes into `../repomod`.

## Check and run a program

Tools, types, and inputs come from a JSON declarations file. `fixtures/cli` holds an example.

```sh
uv run sx explain fixtures/cli/uptimes.sx --declarations fixtures/cli/declarations.json
uv run sx run fixtures/cli/uptimes.sx --declarations fixtures/cli/declarations.json \
  --responses fixtures/cli/responses.json --allow metrics.uptime --input 'hosts=["db1","db2"]'
```

Each command takes `--format json`.

## The Rust command line

`rust/` holds the same command line as a Rust binary, `sx`, with the same options, exit codes, and
output as the Python one. Build it with `cargo build --release --manifest-path rust/Cargo.toml -p sx-cli`.
Two things differ on purpose. The Rust binary reads `"cores": ["text", "semver", "git", "github"]` in a
declarations file and registers those built-in cores, where the Python command refuses. And it
validates declarations more strictly than Python's duck typing does, for example `"types": []`.
`rust/sx-cli/tests/cli_parity.rs` runs every `tests/test_cli.py` scenario, every `fixtures/cli`
combination, every help entry, and the argument and file error paths through both commands and compares
stdout, stderr, and exit status. Python's help for `check`, `explain`, and `run` predates `--grants`, so
the comparison substitutes the Rust help text for it.

## Grants and the fs pack

The Rust binary also grants scoped authority, which Python never had. A declarations file lists
`"packs": ["fs"]` to register the `fs` capabilities, and `--grants FILE` names a JSON object that maps
each granted capability to its scope, or to `null` for a capability that declares none. `--allow ID`
still grants a capability without a scope.

```sh
echo '{"packs": ["fs"]}' > fs.json
echo '{"fs.read-text": {"roots": ["/srv/notes"]}, "fs.list": {"roots": ["/srv/notes"]}}' > grants.json
sx check notes.sx --declarations fs.json --grants grants.json
sx explain notes.sx --declarations fs.json --grants grants.json
sx run notes.sx --declarations fs.json --grants grants.json
```

`check` rejects a call whose literal path the grant does not cover, with `out-of-scope`. `explain` shows
each call site's verdict, or `checked per request` for a computed path. `run` with `--grants` and no
`--responses` reads and writes the files for real, and refuses each out-of-scope request with
`PermissionDenied` before it touches the file system. With `--responses` it answers from the recorded
responses, after the same refusal. [docs/language.md](docs/language.md#grants-and-scopes) has the rules.

## Verify

```sh
uv run pytest
uv run ruff check
uv run ty check
node bench/codemode/check_references.mjs
```

`uv run pytest` runs the kernel and core tests. repomod is not installed in this environment, and
this checks it before the kernel suite runs:

```sh
uv run python -c "import importlib.util, sys; sys.exit(importlib.util.find_spec('repomod') is not None)" \
  && uv run pytest tests cores
```

## Run repomod programs

`sx-repomod` runs `discover` and `status --offline` as Sx programs on the Rust kernel and prints
what the Python commands print. It speaks the [host protocol](docs/language.md#host-protocol) to
`sx serve`, which it finds at `rust/target/release/sx` or `rust/target/debug/sx`, or at `SX_KERNEL`.
It imports repomod from `../repomod/src` when you put it on `PYTHONPATH`. Set
`PYTHONDONTWRITEBYTECODE=1` so nothing is written into `../repomod`. Both programs only read.

```sh
cargo build --release --manifest-path rust/Cargo.toml -p sx-cli
export PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=../repomod/src
uv run sx-repomod status --offline --json --repo acme/tool
uv run sx-repomod discover --owner acme
uv run sx-repomod cores
uv run sx-repomod help github
```

Options such as `--owner`, `--repo`, `--tag`, `--dirty`, `--failed`, and `--refresh` reach the
program as inputs. `status` takes no `--checkout` or `--details`, and `--offline` is required.

`help`, `cores`, `check`, `explain`, and `run` are the `sx` commands with the host's cores and
capabilities already declared, and they run on the same Rust binary. A `--declarations` file you give
adds to the host's declarations, and may list more built-in `cores`. The host reads each core's version
and assurance from the binary's `sx help CORE`. `sx-repomod` imports neither the Python kernel nor the
Python cores. The parity tests import them only to compare with the frozen Python commands.

The parity tests build copied catalogs, temporary Git repositories, fake `gh` responses, and a
fixed clock from repomod's own test helpers, run the Python command and the program on the same
inputs, and compare the parsed JSON, the text, and the exit status:

```sh
PYTHONDONTWRITEBYTECODE=1 uv run pytest hosts/repomod/tests -p no:cacheprovider
```

The tests build the Rust kernel with `cargo` first.

The tests find repomod next to this repository, or at `SX_REPOMOD`. To check types against
repomod's own, run
`uv run ty check --extra-search-path ../repomod/src --extra-search-path ../repomod/tests`.

repomod's own suite runs from a copy, never in place:

```sh
rsync -a --exclude .git --exclude .venv --exclude .work ../repomod/ .work/tmp/repomod/
(cd .work/tmp/repomod && PYTHONDONTWRITEBYTECODE=1 uv run --group dev pytest tests/ -p no:cacheprovider)
```

## Lean reference

`lean/` holds the Lean 4 reference semantics, pinned to `leanprover/lean4:v4.35.0`. It checks and
runs the semantic IR that `sx check FILE --emit ir` exports. The agreement check exports every
accepted run fixture, replays it with `sx-ref`, and compares outcome and trace with the fixture.
The axioms check builds the proofs and fails unless each theorem uses only Lean's standard axioms.
These commands need elan's `lake` on `PATH`, usually from `~/.elan/bin`.

```sh
uv run sx check FILE --declarations DECLARATIONS --allow ID --emit ir > program.ir.json
(cd lean && lake build)
lean/.lake/build/bin/sx-ref check program.ir.json
lean/.lake/build/bin/sx-ref program.ir.json FIXTURE.json
uv run python lean/agree.py
lean/axioms.sh
```

## Differential fuzzing

`tests/fuzz` generates random Sx programs from a typed grammar over the declarations in
`fixtures/run/declarations.json` plus a few more in `tests/fuzz/declarations.json`. It scripts random
host responses, including every call-error kind and malformed outputs, and compares three participants
on each program:

| Participant | Driven through | Compared on |
| --- | --- | --- |
| Rust | the `sx` binary: `sx serve` runs and answers the program, and `sx check --emit ir` exports its IR | the reference side: whether the program checks, the IR, the outcome, and the trace |
| Lean | `sx-ref` on the exported IR and the recorded responses | the Lean checker accepts the IR; outcome and trace equal Rust's |
| Python | the frozen `src/sx` in process | check codes and rendered diagnostics, the IR, and outcome and trace on a replay of the same responses |

The generator knows which participants support each feature it emits (`tests/fuzz/participants.py`), so a
program that uses a Rust and Lean feature is not shown to Python. Every feature the generator emits today
is a frozen-kernel feature, so Python sees every program. A Rust crash, a malformed reply, a
disagreement between participants, or a Lean rejection is a finding. A tenth of the programs are mutated
into mostly invalid source, to prove that no input crashes the reader or checker.

Runs are seeded: `--seed S --count N --start K` always produces the same programs, whatever `--jobs` is.
Findings land in `.work/tmp/fuzz/findings/SEED-INDEX/`. The run prints how many accepted programs contain
and execute each IR class, counted on Python's machine. `CoreCall` stays at zero because Lean has no
cores, and the checker settles `MakeBracket` and `MakeEmptyBrace` away and never builds `MakeVariant`.
The run needs the Lean build with `lake` on `PATH`, and a Rust `sx`: it uses `SX_KERNEL`, else
`rust/target/release/sx`, else `rust/target/debug/sx`. Rebuild the binary after changing Rust, because the
fuzzer does not.

```sh
cargo build --release --manifest-path rust/Cargo.toml -p sx-cli
(cd lean && lake build)
uv run python -m tests.fuzz --fast
uv run python -m tests.fuzz --seed 7 --count 3000 --jobs 6
uv run python -m tests.fuzz --seed 7 --show 123
uv run python -m tests.fuzz --seed 7 --minimize 123
```

`--fast` is the quick slice: 300 programs with a fixed seed, about four seconds with a built binary, exit
status 0 when nothing disagrees. `--triage N` also minimizes up to N findings per kind.

## Run the syntax pilot

The pilot sends the development tasks to a model through the `pi` CLI, then checks and runs every
answer. Raw answers go to `.work/pilot`.

```sh
uv run python bench/codemode/pilot.py --samples 2
uv run python bench/codemode/pilot.py --show --out .work/pilot/runs
```
