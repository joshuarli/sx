# Sx plan

Sx is a small typed orchestration language for model-generated programs. A program is one
S-expression that composes deterministic functions from pure cores with typed calls to host
capabilities. The host decides which capabilities a program may use before anything runs.

The hypothesis is that models coordinate tools more reliably and more cheaply in Sx than in
JavaScript Code Mode. This is an open empirical question. The plan tests it before investing in
formal semantics or a fast runtime.

The language itself is specified in [docs/language.md](docs/language.md). This document covers
goals, decisions, and the order of work.

## What success looks like

The prototype succeeds if it shows all of the following:

1. Models write correct Sx at least as often as they write correct JavaScript for the same tasks,
   at lower total token cost per solved task.
2. A program's full set of possible capability calls is known and authorized before execution.
3. A malformed or unauthorized program never triggers a capability call.
4. A new core registers without any change to kernel source.
5. `repomod discover` and `repomod status --offline` run as Sx programs with output identical to
   the Python commands on the same inputs.

If the model probe in Phase 2 fails its gate, the project stops or changes the language before
any further phase starts.

## Decisions

### Sx lives in its own repository

Sx is developed here, not inside repomod. repomod's `AGENTS.md` forbids generic workflow
frameworks, and Sx is one. The kernel package never imports repomod. Only the repomod host
package depends on both.

### The prototype is Python

Phase 2 asks whether models can write the language, and the interpreter's implementation language
does not affect that answer. Python lets the syntax and type rules change quickly while that
question is open. Python also matches repomod, so repomod's existing `git.py` and `github.py`
functions become capabilities in the same process, without a wire protocol.

The kernel uses Python 3.15, uv, ruff, ty, and pytest, the same toolchain as repomod. The kernel
has no third-party runtime dependencies. An S-expression reader and a type checker of this size
do not need them.

Python numbers for latency, startup, CPU, and memory say nothing about a Rust Sx. Do not report
them as Sx performance.

### The evaluator is an explicit machine

The evaluator is a step function over a machine state held as plain data. It does not use Python
recursion, generators, or `async` to hold evaluation state across capability calls. When a
program needs capability results, the machine returns `Suspended(requests, continuation)`, and
the host resumes it with responses keyed by request ID.

This shape matches the small-step semantics Lean will define in Phase 4, and it makes the Rust
port in Phase 5 a translation instead of a redesign. It is also the shape an external Code Mode
host needs.

### Diagnostics and help are part of the language

No model has seen Sx before. The guide it reads up front and the errors it gets back are the only
ways to teach it, so they decide the repair rate as much as the syntax does. Errors and help are
built in Phase 1, alongside the checker, not added afterward.

Help comes in two tiers, because always-loaded text costs tokens on every task:

- **Guide.** A short overview from `sx help`: the forms, the prelude, and a few examples. Every
  task loads it, and the probe counts its tokens.
- **Diagnostics.** Each error carries the help for the thing that went wrong: the signature it
  violated, the fields that exist, or the form it expected. A model learns the rest of the
  language only when it needs it.

A diagnostic never fails silently, never shows a Python traceback, and never stops at the first
error when the checker found more. The format is in
[docs/language.md](docs/language.md#diagnostics).

### The kernel knows no application domain

The kernel owns syntax, types, effects, evaluation, budgets, and the suspension protocol. It
defines no Git, GitHub, repository, or filesystem concepts. Nominal types such as `RepoId` are
declared by cores. Text operations beyond equality and code-point ordering, including Unicode
case folding, belong to a `text` core.

### Formal semantics follow the probe, then precede Rust

Lean 4 is installed at `~/.elan/bin` (`leanprover/lean4:v4.35.0`). Lean work starts in Phase 4,
once the probe has passed and the language has stopped changing. Lean then defines the
semantics, the Python fixtures become the shared conformance corpus, and the Rust port is
checked against both.

Until Phase 4, `docs/language.md` is the specification and the fixture corpus is the evidence.

### The prototype is read-only

No phase in this plan performs Git or GitHub writes. Capabilities carry an effect class (`read`
or `write`) so the type system can reject parallel writes, but no `write` capability is
implemented against a real system.

## Architecture

Sx has four layers.

- **Kernel.** Parses, checks, and evaluates programs. Resolves names against the registry,
  infers types and effects, enforces budgets, and suspends for capability calls.
- **Pure cores.** Versioned packages of deterministic functions and the types they use. A core
  cannot read the clock, filesystem, network, environment, or randomness. Time and other
  observations reach a core only as argument values.
- **Bridges.** Pure cores that depend on two or more other cores and translate between their
  types. A bridge is registered like any other core and gets no extra powers.
- **Host capabilities.** Typed external operations, each with a stable ID, an input type, an
  output type, and an effect class. The host implements them, authorizes them per execution, and
  rechecks real-world preconditions when it runs them. Every capability also has a fixture
  implementation for tests.

## Repository layout

```
sx/
  pyproject.toml          uv workspace root
  src/sx/                 kernel: reader, checker, machine, registry, prelude
  cores/text/             text core (casefold, split, join)
  cores/semver/           unrelated sample core used to prove extensibility
  hosts/repomod/          sx-repomod: capabilities wrapping repomod, Sx programs, parity tests
  bench/codemode/         Phase 2 probe: task corpus, Sx and JS arms, scoring
  fixtures/               semantic fixtures shared by Python, Lean, and Rust
  docs/language.md        language specification
```

`hosts/repomod` depends on `../repomod` by path. The kernel and the cores do not. The kernel test
suite runs in an environment where repomod is not installed, which proves the boundary.

## Phases

Each phase ends at a gate. Do not start a phase until the previous gate passes.

### Phase 0: Skeleton

Create the uv workspace, ruff and ty configuration, and an empty kernel package with a test
runner.

**Gate:** `uv run pytest`, `uv run ruff check`, and `uv run ty check` pass.

### Phase 1: Python interpreter

Implement the language in `docs/language.md` in this order:

1. Reader: source text to a syntax tree with source positions. Errors recover, so one bad form
   does not hide the errors after it.
2. Checker: name resolution, bidirectional type inference, effect inference, and authorization
   against a capability allow-list. It reports every error it finds in the
   [diagnostic format](docs/language.md#diagnostics).
3. Machine: pure evaluation with budgets.
4. Suspension: `call`, `par`, `par-map`, and `attempt`, with resumption by request ID.
5. Registry: core and capability registration through a public API, with signatures readable
   without invoking anything. Every entry has a one-line description and at least one example.
6. Habit detectors: recognize forms from Clojure, Scheme, and JavaScript that are not Sx, such as
   `defn`, `nil`, `#(...)`, `->`, `(:key r)`, and reversed `filter` arguments. Each one says what
   to write instead.
7. CLI:
   - `sx help` prints the guide.
   - `sx help NAME` prints the full entry for a form, prelude function, core function, type, or
     capability.
   - `sx check FILE` prints diagnostics.
   - `sx explain FILE` prints the capability set and effect classes.
   - `sx run FILE --responses FILE` runs against scripted responses.

   Every command takes `--format json` for hosts. Text output is plain, with no color codes.

Write semantic fixtures as you go. Each fixture holds a program, the registered signatures, an
allow-list, budgets, scripted capability responses, and the expected outcome and logical trace.
Cover ill-typed programs, unauthorized capabilities, budget exhaustion, missing, duplicate, and
unknown response IDs, invalid host results, and responses delivered out of order.

**Gate:**
- All fixtures pass.
- A test shows that a program rejected by the checker, or a program that names an unauthorized
  capability, produces no capability request.
- Every diagnostic code has a fixture that pins its full message.
- Every example in the guide and in `sx help NAME` runs as a test and produces its documented
  result.

### Phase 2: Model probe against JavaScript

Build `bench/codemode` to compare Sx against JavaScript Code Mode on identical tasks.

- Write 20 to 30 tasks. Each has a natural-language description, a set of capabilities, fixture
  responses, and a checker for the final value. Include filtering and grouping, pagination,
  parallel reads, conditional calls, partial failure handled with `attempt`, schema mistakes,
  composition of two cores, and tasks unrelated to repomod.
- Use repomod-shaped fixture capabilities (`catalog.list`, `git.status`, `github.ci`) so the
  tasks are realistic, but do not call repomod.
- The Sx arm gets the `sx help` guide and the task's capability entries. Repairs feed back
  `sx check` or runtime diagnostics.
- The JS arm gets TypeScript declarations for the same tools and runs them as async functions in
  Node, with the same fixture responses. Repairs feed back `tsc` type errors before running and
  runtime errors after. JS gets its best realistic tooling, so a win comes from the language and
  not from weaker feedback.
- Run `opencode-go/muse-spark-1.3-contributor` through `opencode run --format json`. The probe
  uses a weak model on purpose: a language a weak model writes correctly is easy enough for a
  strong one.
- Also run `opencode-go/iceman-contributor-free`, which is free, weaker still, and from a
  different model family. It runs the full corpus. Muse Spark runs as much of the corpus as the
  OpenCode Go weekly limit allows, and Iceman is the fallback when that limit is reached. Both
  runs in a pair use the same model. Report results per model.
- Define an opencode agent for the probe with every tool disabled, so the model answers with a
  program instead of running commands. Both arms use the same agent.
- Read token counts from the `step_finish` events in opencode's JSON output. opencode adds its
  own prompt to every request: a one-line prompt to the default agent reported 9,209 input
  tokens. Measure the probe agent with an empty task and subtract that baseline, so the reported
  tokens are the task, the language guide, and the answer.
- Run each task several times per arm. Pair the runs. Allow a fixed number of repair attempts,
  feeding back the checker or runtime error.
- Before the full run, measure the tokens of one paired task and size the corpus and sample count
  to fit the remaining OpenCode Go quota.
- Record per run: first-attempt correctness, final correctness, input and output tokens, tokens
  spent on the language guide, repair attempts, static rejections, runtime failures, and
  capability calls, including duplicates.
- Record each diagnostic code a model received and whether the next attempt fixed it. A code
  that models repeatedly fail to fix needs a better message. A mistake that appears often with no
  habit detector needs a new detector.

**Gate:** On each model, over the pairs it completed, Sx first-attempt correctness is no more
than 5 percentage points below JavaScript, and Sx is better on either correctness or total tokens per solved task. If the gate
fails, revise the language at most once and rerun. If it fails again, stop the project.

### Phase 3: Cores and the repomod host

Build the cores and the read-only repomod integration.

1. `text` core: `text.casefold` matching Python's `str.casefold`, plus the few string
   functions the repomod programs need.
2. `semver` core with no repomod dependency, and one program that composes it with `text`. This
   proves a core registers without kernel changes.
3. `git` core. Model a checkout as an enum, not as repomod's `GitStatus` flags. Its shape is
   `Missing | Invalid | OriginMismatch | Detached | OnBranch`, where only the variants that have a
   HEAD carry one. Never read a missing observation as `false`.
4. `github` core: CI aggregation that treats missing, pending, stale, and unknown checks
   explicitly, ported from `_state_for_checks` in `repomod/github.py`.
5. `sx-repomod` host: capabilities that wrap repomod's catalog, Git, and cached GitHub reads.
   Policy rules stay in Python behind one read-only `policy.lint` capability. The current time
   reaches programs as a capability result, so tests fix it.
6. Programs: `discover.sx` and `status-offline.sx`.

Test parity by running the Python command and the Sx program on the same inputs and comparing
JSON output and exit status. Build the inputs from repomod's own test helpers: copied catalogs,
temporary Git repositories, fake `gh` responses, and a fixed clock.

**Gate:** Both programs match the Python commands on every parity fixture. The kernel test suite
passes without repomod installed. repomod's own test suite is unchanged and passes.

### Phase 4: Lean semantics

Create a Lean project pinned to `leanprover/lean4:v4.35.0`. Write the reference semantics
independently of the Python code: syntax, typing, the machine, effects, budgets, and traces.
Make the evaluator executable and run it on the fixture corpus.

Prove, in this order:

1. Type preservation and progress for the pure fragment, up to the modeled terminal errors.
2. Effect soundness: every capability a program can request appears in its inferred effect set.
3. Pure core application never produces a capability request.
4. Budget counters never exceed their limits.
5. `par` results are assembled in source order regardless of response order.

A theorem counts only if it builds with no `sorry` and `#print axioms` lists only Lean's
standard axioms. That check is the proof status. Do not keep a separate ledger.

Specify the `git` and `github` decision functions only where a small theorem is worth having. An
example is: CI aggregation never reports `passing` when a required check failed.

**Gate:** The Lean evaluator agrees with Python on every fixture, and theorems 1 to 3 are proved.

### Phase 5: Rust port

Port the kernel to Rust once Phases 2 to 4 hold. Check it against the same fixtures, compared
with both Python and Lean. Only then measure parsing, evaluation, suspension, and host dispatch
cost.

Plan this phase in detail when Phase 4 ends.

## repomod safety rules

These rules apply to every phase.

- Never run `fix`, `push`, `merge`, `gc`, `topics apply`, or `sync` from this project.
- Never modify `../repomod`. If repomod needs a change to support the host, propose it
  separately.
- Run parity and mutation-style tests only on copied catalogs and temporary repositories.
- Running `repomod discover` and `repomod status --offline` against the live catalog for a
  shadow comparison is allowed, because both only read.
- Canonical checkouts under `~/repos` are user data. Read them only.

## Assurance levels

The registry reports what evidence exists for each core and never collapses it into one
"verified" flag. The categories are:

- **Unverified:** the interface was validated.
- **Conformance-tested:** the core passes named fixture and parity tests.
- **Specified:** a Lean model of the core exists.
- **Proved-properties:** named theorems about that model pass the check in Phase 4.

A Lean theorem is about the Lean model. It says nothing about the Python or Rust code, or about
real Git and GitHub behavior, without separate conformance evidence.

## Out of scope

- Writes to Git, GitHub, or the filesystem, including guarded sync, push, fix, and merge.
- Package manifests, version resolution, content-addressed identity, and dynamic loading. Cores
  register through the Python API until a second externally written core exists.
- WASM, sandboxed third-party cores, and a stable binary ABI.
- Macros, recursion, user-defined top-level functions, mutable state, and floats.
- CI workflows.

## Open questions

- Whether `par` should allow writes to disjoint resources. Revisit only after write capabilities
  exist.
- Whether models do better with direct AST generation (JSON) than with S-expression text. Add it
  as a third probe arm if Phase 2 results are close.
