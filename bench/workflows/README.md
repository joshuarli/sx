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
uv run python bench/workflows/runner.py trials --model MODEL --samples 2 --out .work/workflows/run1
uv run pytest -q tests/test_workflows.py
```

`reference` runs each workflow's two reference solutions without a model and exits 1 unless both
are judged correct. `trials` asks the model through the probe's `pi` harness (`pilot.ask`), allows
the probe's two repairs, and saves one `trial.json` per attempt series in the probe's format, so
`probe.tokens` and `probe.summarize` read them.

## A workflow

A workflow is a directory under `workflows/` named by its id.

| File | Holds |
| --- | --- |
| `task.md` | The task as the model reads it |
| `declarations.json` | Sx declarations: `packs`, and typed endpoints or other capabilities |
| `grants.json` | The grants, a JSON object from capability id to scope |
| `workflow.py` | `setup(sandbox)` and `check(sandbox, result) -> Verdict` |
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
- `sandbox.serve(name, handler)` starts a local HTTP service on `127.0.0.1` and records its
  requests. `handler` maps a `Request` to a `Response`.
- `sandbox.reset()` removes everything under the root and clears service request logs. The root
  path and the service ports stay, so a model's answer that names them still applies. The
  runner calls `setup` again after each reset, once per attempt, so every attempt and both arms
  start from the same state.

`check` inspects the sandbox and the result and returns `Verdict.correct()` or
`Verdict.wrong(reason)`. It runs only when the program completed. A rejected or failed program is
reported with the feedback a real host would show: diagnostics or a type error, or the runtime
failure.

## The arms

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
