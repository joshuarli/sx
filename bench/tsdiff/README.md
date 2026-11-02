# tsdiff: Bun against sx on checkable TypeScript

Phase 19's proof. A case is a directory holding a TypeScript program, the declarations, the recorded responses
(the format `sx run --responses` reads), the tool list, and `case.json`. `shim.ts` runs the program in Bun with
JavaScript meaning. `sx run --language ts` runs it as lowered Sx. The comparator normalizes both to result, logs,
ordered tool calls, delays, and outcome, and reports `agree`, `agree-refined`, `rejected` (by sx, with its
diagnostic codes), or `disagree`. A trap agrees only when both sides trap the same way or the case expects it.

```sh
export SX_BIN=rust/target/debug/sx
cd bench/tsdiff && npm ci && cd ../subset && npm ci && cd ../..

uv run python bench/tsdiff generate --count 1000 --seed 0 --out .work/tmp/tsdiff/generated
uv run python bench/tsdiff generate --count 150 --seed 5000 --mutants --out .work/tmp/tsdiff/mutants
uv run python bench/tsdiff corpus --effects .work/effects --real .work/real --out .work/tmp/tsdiff/corpus

uv run python bench/tsdiff run .work/tmp/tsdiff/generated .work/tmp/tsdiff/corpus .work/tmp/tsdiff/mutants
uv run python bench/tsdiff run --shim-only .work/tmp/tsdiff/corpus

bun test --cwd bench/tsdiff
uv run pytest bench/tsdiff/tests
```

`run` exits nonzero on any disagreement or any mutant that sx accepts. It stops with an error when the sx binary
does not accept `run --language ts`. `--shim-only` needs no sx and prints what the JavaScript side did.

A mutant is a generated program with one construct just outside the subset prepended; sx must reject it, and the
report names any mutant it rejects with another code than the spec's. The corpus cases are the tool-calling codemode
scripts after `bench/subset`'s API rewrite, with responses recovered from the tool results nested in each session's
events; scripts that call pi's own tools are skipped because no declaration covers them.
