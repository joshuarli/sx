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

## Check and run a program

Tools, types, and inputs come from a JSON declarations file. `fixtures/cli` holds an example.

```sh
uv run sx explain fixtures/cli/uptimes.sx --declarations fixtures/cli/declarations.json
uv run sx run fixtures/cli/uptimes.sx --declarations fixtures/cli/declarations.json \
  --responses fixtures/cli/responses.json --allow metrics.uptime --input 'hosts=["db1","db2"]'
```

Each command takes `--format json`.

## Verify

```sh
uv run pytest
uv run ruff check
uv run ty check
node bench/codemode/check_references.mjs
```

## Lean reference

`lean/` holds the Lean 4 reference semantics, pinned to `leanprover/lean4:v4.35.0`. It checks and
runs the semantic IR that `sx check FILE --emit ir` exports. The agreement check exports every
accepted run fixture, replays it with `sx-ref`, and compares outcome and trace with the fixture.
The axioms check builds the proofs and fails unless each theorem uses only Lean's standard axioms.

```sh
uv run sx check FILE --declarations DECLARATIONS --allow ID --emit ir > program.ir.json
(cd lean && lake build)
lean/.lake/build/bin/sx-ref check program.ir.json
lean/.lake/build/bin/sx-ref program.ir.json FIXTURE.json
uv run python lean/agree.py
lean/axioms.sh
```

## Run the syntax pilot

The pilot sends the development tasks to a model through the `pi` CLI, then checks and runs every
answer. Raw answers go to `.work/pilot`.

```sh
uv run python bench/codemode/pilot.py --samples 2
uv run python bench/codemode/pilot.py --show --out .work/pilot/runs
```
