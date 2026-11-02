# Workflow runner

Runs a workflow in two languages against the same local sandbox: Sx through `sx run --grants`,
which performs the granted tools natively, and TypeScript on Bun, which reaches the same
performers through `sx mcp serve`. Each language is an arm: `sx` and `js`, the probe's name for
its TypeScript arm.

## Setup

```sh
cargo build --manifest-path rust/Cargo.toml -p sx-cli
npm ci --prefix bench/codemode
```

The runner uses `$SX_BIN`, else `rust/target/release/sx`, else `rust/target/debug/sx`, as the probe
does. It stops with the install command if `bun`, the pinned `tsc`, or the binary is missing.

```sh
uv run python bench/workflows/runner.py reference
uv run python bench/workflows/runner.py trials --model MODEL --samples 3 --jobs 4 --out .work/workflows/run1
uv run python bench/workflows/runner.py report --out .work/workflows/run1
uv run pytest -q tests/test_workflows.py
```

`reference` runs each workflow's two reference solutions without a model and exits 1 unless both
are judged correct. `trials` asks the model through the probe's `pi` harness (`pilot.ask`), allows
the probe's two repairs, and saves one `trial.json` per attempt series in the probe's format, so
`probe.tokens` and `probe.summarize` read them. It runs `--jobs` trials at a time, each in its own
sandbox, and saves `extras.json` beside each `trial.json`: per attempt, the model seconds, the run
seconds, and for an Sx answer the `sx explain --format json` report taken before the run. `report`
turns a trials directory into the correctness, token, and time table, the host code each workflow
needed, and a summary of what `explain` said.

## The workflows

| Workflow | Task | What it needs |
| --- | --- | --- |
| `git-ci-audit` | Inspect 100 local Git checkouts and ask a mock CI service about the clean ones | `process` running `/usr/bin/git` with four granted argument patterns, a typed `http` endpoint, `par-map`, the `text` core |
| `rest-sqlite` | Page through a REST service that sometimes answers 429 or 503, and store the rows | a typed endpoint, `retry` with stated delays, `loop`, a `sqlite` pack with declared statements |
| `yaml-manifests` | Bump versions and replicas in YAML manifests without disturbing any other byte | the `yaml` core, `fs` writes, the `text` core |
| `compiler-repair` | Run `/usr/bin/cc -fsyntax-only`, parse diagnostics, add missing semicolons, rerun, at most 6 runs | `process`, `fs`, `loop`, the `text` core |
| `object-store-index` | Build an index of Markdown files and PUT it to an object store that demands a credential | a typed `PUT` endpoint whose `authorization` header the grant injects from an environment variable |
| `mcp-inventory` | Look up items and reserve stock through a stdio MCP server | `sx mcp import`, `sx mcp run`, an imported read tool and an imported write tool with an optional enum input |

Six more workflows cover other shapes of development work:

| Workflow | Task | What it needs |
| --- | --- | --- |
| `log-triage` | Count error codes and services over many log files and write a summary file the check compares byte for byte | `fs.list`, `fs.read-text`, `fs.write-text`, the `text` core, `group-by` |
| `invoice-totals` | Total CSV-like invoices per customer under a stated rounding rule and flag invoices whose stated total is wrong | the `decimal` core; TypeScript has no equivalent and uses scaled integers |
| `deploy-rollout` | Start deployments with a non-idempotent POST, poll each until it is done or failed within a deadline, and roll back the failed ones | typed `POST` and `GET` endpoints, `retry` with `:on [RateLimited]` around a write, `wait-until` |
| `issue-notify` | Page through an issue tracker by page number, announce new critical issues in chat, and record them in SQLite | typed endpoints, declared `sqlite` statements over a database the sandbox seeds, `attempt` around a non-idempotent write |
| `calendar-mcp` | Find free slots for several people and book meetings through a stdio MCP server whose tools return structured results | `sx mcp import`, `sx mcp run`, nested output records with an optional field, an error result |
| `dependency-audit` | Ask a package registry about each distinct package in text manifests and compare versions | `fs`, a typed endpoint, `par-map` with a limit, the `semver` core |

Each `task.md` states every policy its check verifies. The checks look at the sandbox, not only the
result: the Git workflow checks which commits CI was asked about and the peak number of requests in
flight, the REST workflow checks the database rows, the order of the page requests, and the delays
between retries, the YAML workflow compares every manifest byte for byte, the compiler workflow
compares the file and recompiles it, the object store workflow checks the stored object and that the
credential is in no program, result, or run transcript, and the MCP workflow checks the server's
ledger of calls.

## A workflow

A workflow is a directory under `workflows/` named by its id.

| File | Holds |
| --- | --- |
| `task.md` | The task as the model reads it |
| `declarations.json` | Sx declarations: `packs`, and typed endpoints or other capabilities |
| `grants.json` | The grants, a JSON object from capability id to scope |
| `workflow.py` | `setup(sandbox)` and `check(sandbox, result) -> Verdict` |
| `server.json` | Optional. `{"name": NAME, "command": [...]}` for a stdio MCP server the workflow uses |
| `reference.sx` | A correct Sx program |
| `reference.mts` | A correct TypeScript module, `export default async function main(api: Api)` |

`task.md`, `declarations.json`, `grants.json`, and the two references are templates. `{{name}}`
becomes the sandbox variable `name`, and a name with no variable is an error. A sandbox defines
`root` (the sandbox directory), `NAME_origin` for each service started with `sandbox.serve(NAME,
handler)`, and any variable `setup` defines with `sandbox.define`.

The sandbox is the whole state a workflow acts on:

- `sandbox.root` is a fresh directory under `.work/tmp`. `sandbox.write(path, text)` and
  `sandbox.directory(...)` fill it.
- `sandbox.git_repository(path, commits)` makes a local repository whose commits have fixed
  identities and dates, so its hashes are the same on every run.
- `sandbox.copy_tree(source, path)` copies a directory tree into the sandbox, so a workflow
  builds one deterministic Git repository once and copies it for the other ninety-nine.
- `sandbox.serve(name, handler, latency=0.0)` starts a local HTTP service on `127.0.0.1` and
  records its requests with their arrival time and the peak number in flight. `handler` maps a
  `Request` to a `Response`.
- `sandbox.reset()` removes everything under the root and clears service request logs. The root
  path and the service ports stay, so a model's answer that names them still applies. The
  runner calls `setup` again after each reset, once per attempt, so every attempt and both arms
  start from the same state.

`check(sandbox, result, evidence)` inspects the sandbox, the result, and the evidence (`program`, the
text of the answer, and `transcript`, the arm's raw run report) and returns `Verdict.correct()` or
`Verdict.wrong(reason)`. It runs only when the program completed. A rejected or failed program is
reported with the feedback a real host would show: diagnostics or a type error, or the runtime
failure.

## The arms

A workflow with a `server.json` uses a stdio MCP server beside the native packs. The runner starts it
with `sx mcp import --name NAME -- COMMAND` and merges the imported capabilities into the
declarations; the grants name them as `mcp-NAME.TOOL`. The Sx arm then runs `sx mcp run ... -- COMMAND`.
The TypeScript arm connects the Bun harness to the same command (`--imported-command`,
`--imported-names`) in addition to `sx mcp serve`, which serves only the native capabilities, and
its tools get the names the importer would give them, `api.mcpInventory.lookupItem`. The harness
turns an `isError` result of such a server into a `CallError` of kind `Failed`, as `sx mcp run` does,
and leaves out a top-level `null` input property, as Sx leaves out a `none`.

**Sx.** The runner renders the declarations and grants into a scratch directory and runs
`sx run FILE --declarations D --grants G --format json`. Exit 1 is a rejection, such as
`unauthorized` or `out-of-scope`, found before any tool runs. Exit 2 is a runtime failure.

**TypeScript.** `sx mcp describe` prints the served capabilities with the named types they use,
and `bench/codemode/typescript.py` turns that into `api.d.ts`, the same generator the probe uses.
The module is checked with the pinned `tsc` and the probe's compiler options, then run by
`bun run harness/run.ts` against `sx mcp serve` with the same declarations and grants. A tool that
is not granted is not in `api.d.ts`, and the server would not list it.

`harness/mcp.ts` is a minimal MCP client over stdio: JSON-RPC ids, concurrent requests, a timeout
per request that sends `notifications/cancelled`, and a shutdown that closes the server's input.
`harness/api.ts` builds `api` from `tools/list`: tool `NS.NAME` becomes `api.ns.name`, kebab case
becomes camel case, and `api.sleep(ms)` waits for real. A call is a `tools/call` with the input
as `arguments`, and the result is read like this:

| Result | Becomes |
| --- | --- |
| `isError` with `structuredContent.kind` | throws `CallError` with `kind` and `message` |
| a protocol error, such as an unknown tool | throws `HostError` |
| no `outputSchema` | the concatenated text, a `String` |
| `_meta["sx/result"]` is `"wrapped"` | `structuredContent.result` |
| otherwise | `structuredContent` |

The run report holds the result or the error, every call with its time and outcome, `startupMs`
for connecting, and `elapsedMs` for the program. The runner also times the whole `bun` process.

## Wall time

Each arm reports two numbers. `total` is the whole command: for Sx, `sx run`; for TypeScript, the
`bun` process including starting `sx mcp serve`, but not the type check. `program` is the part
that runs the workflow: for Sx the same as `total`, because the checker and the run are one
process, and for TypeScript `elapsedMs` of the harness.

## Checking the harness

`uv run pytest -q tests/test_workflows.py` runs the example workflow in both arms, the sandbox,
the rejection and failure paths, the missing-toolchain errors, a type check of the harness itself,
and the model loop with a stand-in for the model.

## Result

The Phase 9 comparison of Muse Spark on the six workflows, 3 samples per arm, is recorded in plan.md under Phase 9c, with its host code counts and what `sx explain` reported.

Findings that shape how to read a result:

- `process.run` is a `write`, and `par-map` allows only reads, so the Sx reference of `git-ci-audit` runs its Git commands one after another and only the CI queries are parallel. The TypeScript reference runs the Git commands together.
- The TypeScript arm has no `yaml` core, so `yaml-manifests` makes it scan lines by hand.
- A `yaml` refusal is not caught by `attempt`, so the YAML workflow names the file the core would refuse in its task instead of expecting a program to recover.
- Wall time is measured with `--jobs` trials at once. A model call dominates it.
