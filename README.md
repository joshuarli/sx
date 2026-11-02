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
| `src/sx` | The kernel: reader, checker, machine, registry, help. |
| `cores/text`, `cores/semver` | Pure cores that register through the Python API. |
| `cores/git`, `cores/github` | Decision cores: a checkout as `Missing | Invalid | OriginMismatch | Detached | OnBranch`, and CI aggregation. |
| `hosts/repomod` | `sx-repomod`: capabilities that wrap repomod, and the `discover` and `status --offline` programs. |
| `bench/codemode` | The model probe. |

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

`sx-repomod` runs `discover` and `status --offline` as Sx programs and prints what the Python
commands print. It imports repomod from `../repomod/src` when you put it on `PYTHONPATH`. Set
`PYTHONDONTWRITEBYTECODE=1` so nothing is written into `../repomod`. Both programs only read.

```sh
export PYTHONDONTWRITEBYTECODE=1 PYTHONPATH=../repomod/src
uv run sx-repomod status --offline --json --repo acme/tool
uv run sx-repomod discover --owner acme
uv run sx-repomod cores
uv run sx-repomod help github
```

Options such as `--owner`, `--repo`, `--tag`, `--dirty`, `--failed`, and `--refresh` reach the
program as inputs. `status` takes no `--checkout` or `--details`, and `--offline` is required.

The parity tests build copied catalogs, temporary Git repositories, fake `gh` responses, and a
fixed clock from repomod's own test helpers, run the Python command and the program on the same
inputs, and compare the parsed JSON, the text, and the exit status:

```sh
PYTHONDONTWRITEBYTECODE=1 uv run pytest hosts/repomod/tests -p no:cacheprovider
```

The tests find repomod next to this repository, or at `SX_REPOMOD`. To check types against
repomod's own, run
`uv run ty check --extra-search-path ../repomod/src --extra-search-path ../repomod/tests`.

repomod's own suite runs from a copy, never in place:

```sh
rsync -a --exclude .git --exclude .venv --exclude .work ../repomod/ .work/tmp/repomod/
(cd .work/tmp/repomod && PYTHONDONTWRITEBYTECODE=1 uv run --group dev pytest tests/ -p no:cacheprovider)
```

## Run the syntax pilot

The pilot sends the development tasks to a model through the `pi` CLI, then checks and runs every
answer. Raw answers go to `.work/pilot`.

```sh
uv run python bench/codemode/pilot.py --samples 2
uv run python bench/codemode/pilot.py --show --out .work/pilot/runs
```
