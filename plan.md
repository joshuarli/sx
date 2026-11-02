# Sx plan

Sx is a small typed orchestration language for model-generated programs. A program is one
S-expression that composes deterministic functions from pure cores with typed calls to host
capabilities. The host decides which capabilities a program may use before anything runs.

The hypothesis is that models coordinate tools more reliably and more cheaply in Sx than in
JavaScript Code Mode. This is an open empirical question. The plan tests it before investing in
formal semantics or a fast runtime.

The language itself is specified in [docs/language.md](docs/language.md). This document covers
the current campaign: its goals, decisions, and order of work. The longer direction, and what the
current campaign must keep possible, is in [docs/ROADMAP.md](docs/ROADMAP.md).

## What success looks like

The prototype succeeds if it shows all of the following:

1. Models write correct Sx at least as often as they write correct JavaScript for the same tasks,
   at lower total token cost per solved task.
2. A program's full set of possible capability calls is known and authorized before execution.
3. A malformed or unauthorized program never triggers a capability call.
4. A new core registers without any change to kernel source.
5. `repomod discover` and `repomod status --offline` run as Sx programs whose results match the
   Python commands' `--json` output and exit status on the same inputs.

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

### Retries, polling, and loops are bounded kernel forms

Agents retry flaky calls, poll for state changes, and follow pagination cursors constantly, and
JavaScript makes all three easy to get wrong. Sx takes its forms from xsh: `retry` with an explicit delay list or a capped
deterministic backoff, `wait-until` with a required deadline, and retry selection by error kind
instead of by message text. Every option is a literal, so the checker computes the maximum number
of attempts and programs still always terminate. Sx adds two rules for agents: the default
`retry` covers only transient kinds, and `retry` refuses to repeat a write that is not declared
idempotent. Delays are host requests, so the machine never reads a clock and fixture runs take no
time. A `loop` with `recur` and a required literal `:max` covers pagination and other stateful
repetition, using the Clojure form models already know, without general recursion.

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
  cores/git/              checkout enum and local status decisions
  cores/github/           CI and upstream states and check aggregation
  hosts/repomod/          sx-repomod: capabilities wrapping repomod, Sx programs, parity tests
  bench/codemode/         Phase 2 probe: task corpus, Sx and JS arms, scoring
  fixtures/               semantic fixtures shared by Python, Lean, and Rust
  docs/language.md        language specification
```

`hosts/repomod` depends on `../repomod` by path. The kernel and the cores do not. The kernel test
suite runs in an environment where repomod is not installed, which proves the boundary.

## Status

Phase 0 and Phase 1 have passed their gates. Phase 2 failed twice on the first held-out set, the
second time after its one allowed revision, and then passed on a fresh blind held-out set. The
owner decides at the review before the Rust port whether that pass is enough to continue.

**Phase 0.** The syntax pilot ran on the development set and drove one language revision.

**Phase 1.** The Python checker, machine, CLI, and help are built. The checker lowers source into a
typed semantic IR, and the machine executes only that IR, so surface syntax stays separate from
semantics. Two decisions differ from the phase text:

- The `sx help` guide stays hand-written, because the pilot tuned its wording. Tests require it to
  name every form and prelude function, and require each of its examples to produce the result it
  shows.
- The semantic fixtures live in `fixtures/run` as data, for the Lean evaluator and the Rust port to
  replay in Phases 4 and 5.

With the checker and machine in place, the pilot scores answers exactly instead of by hand. On the
15 development tasks:

| Run | Correct result | Type-checks |
| --- | --- | --- |
| Iceman, round 1, against the revised spec | 44 of 60 | 45 of 60 |
| Iceman, round 2 | 50 of 60 | 50 of 60 |
| Muse Spark at medium thinking, through pi | 28 of 30 | 30 of 30 |

Both Muse misses came from an ambiguous task description, since corrected. These are development
results and say nothing about the held-out set.

**Phase 2 guide level.** The development sweep ran every guide level and TypeScript, with two
samples of all 15 tasks on each model. By the selection rule, the held-out run uses `standard` for
both models: it ties `full` on first-attempt correctness at fewer tokens per solved task, and
`minimal` falls more than 5 points behind. On the development set the standard guide trails
TypeScript by 6.7 points on Muse Spark and 26.7 points on Iceman at first attempt, so both fail
the gate there.

**Phase 2 held-out result.** The standard guide against TypeScript, two samples of the 24
expressible held-out tasks on each model, with up to two repairs, rescored under the zero-wait
rule below:

| Model | Arm | First attempt | Final | Attempts | First prompt | Reasoning per trial | Tokens per solved task |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Muse Spark | TypeScript | 97.9% | 100.0% | 1.02 | 526 | 3,157 | 4,292 |
| Muse Spark | Sx | 77.1% | 95.8% | 1.31 | 1,995 | 4,266 | 8,037 |
| Iceman | TypeScript | 95.8% | 95.8% | 1.00 | 524 | 597 | 1,538 |
| Iceman | Sx | 22.9% | 56.2% | 2.06 | 2,009 | 1,237 | 12,573 |

Sx minus TypeScript at first attempt is -20.8 points on Muse Spark (95% interval -37.5 to -6.2)
and -72.9 on Iceman (-87.5 to -56.2). At final it is -4.2 (-10.4 to 0.0) and -39.6 (-58.3 to
-22.9). Sx also costs 1.9 and 8.2 times the tokens per solved task, so it fails every part of the
gate on both models. The expressiveness gap is 6 of 30 held-out tasks.

- Muse Spark fell from 93.3% on the development set to 77.1% here, and reasoned more in Sx than in
  TypeScript, the reverse of the development sweep. The development set was written alongside the
  language and overstates how well it transfers.
- Muse Spark repaired all 20 diagnostics it received. Iceman repaired most type diagnostics but
  only 0 of 7 `unclosed` and 1 of 5 `unexpected-close`, so balancing brackets is its hardest
  problem.
- As first scored, two final Sx trials on each model failed only because they waited `[0, 0]`
  where the reference waited nothing. A `0ms` retry delay reaches the host as a wait, while an
  immediate TypeScript retry calls no `sleep`. Rescoring under the zero-wait rule raised Muse
  Spark from 72.9% to 77.1% at first attempt and from 91.7% to 95.8% at final, and Iceman from
  20.8% to 22.9% and 52.1% to 56.2%. The verdict did not change.

**Phase 2 revision.** Almost every first-attempt failure on the development set is a static
rejection, and most come from Sx being stricter than the habits models bring from TypeScript and
Clojure, not from the S-expression surface. The revision accepts those habits where they have one
safe meaning. Each change cites the development failures behind it:

1. **An optional name narrows in a condition.** `(if next (recur next ids) ids)` tests whether
   `next` is present and sees its value in the then-branch. A `match` on a name narrows it in a
   final `_` clause when the clauses before it handled `none`. An `Option<Bool>` condition stays an
   error, because presence and truth would disagree. Evidence: `(> ahead 0)` on an `Option<Int>`
   in both Muse Spark failures of `dev-dirty-or-ahead`, `(if next ...)` in `dev-events-pages`, and
   `(match acc none v _ (semver.compare v acc))` in `dev-latest-version`.
2. **Defaults read like Clojure.** `(get r :ahead 0)` reads an optional field with a fallback.
   Evidence: Muse Spark hedged with `(match (get s :ahead) (some a) a none 0 _ (get s :ahead))`
   twice.
3. **A nullary enum joins with `String`.** Nullary variants already encode as their names, so a
   branch returning `"missing"` beside a `ci.State` makes the join `String`. Evidence: three
   `branch-mismatch` rejections on `dev-retry-ci`.
4. **A lowercase name is a catch-all `match` pattern** that binds the value, under the rules for
   `_`. Evidence: `c {:repo repo ...}` on `dev-retry-ci` and the Phase 0 watch item.
5. **A bracket literal takes its shape from its use.** A homogeneous literal that a tuple pattern
   later takes apart is a tuple, not a list. Evidence: `[(get s :repo) b]` destructured by
   `(fn [[repo branch]] ...)` on Muse Spark's `dev-stale-branches`.
6. **`cond` and `identity` exist.** `cond` must end with `:else`. Evidence: `dev-ticket-actions`
   and `dev-order-totals`.
7. **A record literal may omit an `Option` field**, which is then `none`. Evidence: three
   `missing-field` rejections at the minimal level and the Phase 0 watch item.
8. **`unclosed` and `unexpected-close` say where the form should close**, from the indentation of
   the lines after the opener. Evidence: four bracket rejections at the minimal level.
9. **A final `_` that can never match does not widen the `match`.** Muse Spark hedged with
   `(match (get s :ahead) (some a) a none 0 _ (get s :ahead))`, and the unreachable `_` made the
   result `Option<Int>`, which then failed `(> ahead 0)`. Item 1 cited this failure but does not
   fix it. Re-scoring the development answers under items 1-8 showed the cause.
10. **`none` is a valid `get` default, and an optional whose inner type is not yet known is a
    presence test.** `(get r :f none)` keeps the field optional. A presence test on a value whose
    type later resolves to `Option<Bool>` is still rejected. Evidence: all four failed first
    attempts in the development rerun after items 1-9 used `(get r :f none)`, and one also tested
    a loop variable bound to `none`.

The held-out rerun ran against commit 295af7b, on the Sx arm only. The TypeScript arm did not
change, so its first held-out trials pair with the new Sx trials.

**Phase 2 second held-out result.** Muse Spark, standard guide, 24 expressible tasks, two
samples:

| Arm | First attempt | Final | Attempts | First prompt | Reasoning per trial | Tokens per solved task |
| --- | --- | --- | --- | --- | --- | --- |
| TypeScript | 97.9% | 100.0% | 1.02 | 526 | 3,157 | 4,292 |
| Sx after the revision | 70.8% | 93.8% | 1.35 | 2,182 | 4,439 | 8,723 |

Sx minus TypeScript is -27.1 points at first attempt (95% interval -45.8 to -10.4) and -6.2 at
final (-16.7 to 0.0). The gate fails again. First attempts did not improve on the first run's
77.1%; the difference is inside the noise of 48 trials. The revision fixed what the development
set showed, and the development set does not predict the held-out tasks: after the revision the
same development answers pass at 100%.

**After the gate.** The first held-out set has spent its one revision, so it no longer works as a
gate. Its answers and failures are now development evidence. A fresh held-out set, written by an
agent whose tasks are never read by the language designer, replaces it for any later gate, under
the same freeze rule.

**Changes after the gate.** With the first held-out set as development evidence:
- A checker bug: branch joins picked one branch's type instead of a least upper bound, so records
  needing different fields promoted failed. Every `held-room-booking` answer hit it.
- `str` writes integers, booleans, and enum values. The machine's `str` had silently dropped
  non-strings.
- Records with different fields join, with one-sided fields optional; `retry` of a
  non-idempotent write is allowed when every retried kind is a refusal and the write is the last
  call; literal patterns; reserved form names; one report per duplicate diagnostic; `sleep`;
  literal tuple indexes; an extra closer inside an open bracket is called extra.

New Muse Spark answers after these changes, one sample: development 16 of 17 and the retired set
22 of 24 at first attempt, all correct at final. The retired set informed the changes, so this is
not a gate result. The fresh set's gate run used commit 8027f85.

**Phase 2 fresh held-out result.** Muse Spark, standard guide, the 28 expressible tasks of the
fresh set, two samples, written by an agent whose tasks the language designer never read:

| Arm | First attempt | Final | Attempts | First prompt | Reasoning per trial | Tokens per solved task |
| --- | --- | --- | --- | --- | --- | --- |
| TypeScript | 96.4% | 96.4% | 1.00 | 456 | 2,228 | 3,176 |
| Sx at 8027f85 | 94.6% | 98.2% | 1.04 | 2,382 | 2,561 | 5,420 |

Sx minus TypeScript is -1.8 points at first attempt (95% interval -7.1 to +3.6) and +1.8 at final
(0.0 to +5.4). The gate passes: first attempt is within 5 points, and final correctness is
better. The final margin is one trial, so the pass rests on the first-attempt bound more than on
a real lead. Sx still costs 1.7 times the tokens per solved task; its first prompt is 1,926 tokens longer
than TypeScript's, mostly the guide. The expressiveness gap is 2 of 30. The fresh set's answers stay
unread, so it can serve once more as a gate after a future revision.

Of that cost, output tokens per solved task are 2,897 for Sx and 2,703 for TypeScript, 1.07
times. The difference is almost all input, and the guide is a fixed prefix a host can cache.
A shorter guide does not help: after the gate, the `minimal` level on the development and
retired sets scored 37 of 41 at first attempt against 38 of 41 for `standard`, but cost more
tokens per solved task (4,773 against 4,602 on development, 7,996 against 6,429 on the retired
set), because Muse Spark reasons more when the guide says less. `standard` stays.

**Data model before the Rust port.** `Map`, `Float`, `Bytes`, and open record declarations
landed ahead of the roadmap's campaign 2, so the port implements one language instead of
chasing it. `Float` came before `Decimal` because the only types both held-out sets lacked were
`Float` and, in the first set, `Map` and `Bytes`.

Out of the revision: `Map` and dynamic keys (`assoc`, `keys`), which also make up the
expressiveness gap, belong to the type-set work in the roadmap.

One scoring change comes with it and applies to both arms: a zero-length wait no longer counts
as a wait, so an immediate retry scores the same whether it is written as a `0ms` Sx delay or as a
TypeScript loop with no `sleep`. The table above is rescored under it.

Held-out exposure before the revision: the aggregate report, diagnostic codes with repair counts,
and the judge's reason line for each final failure. No held-out task or model answer was read.
The bracket-diagnostic change (item 8) matches the held-out `unclosed` repair rate as well as the
development evidence.

**Phase 3.** The `text`, `semver`, `git`, and `github` cores are workspace packages that register
through the Python API with no kernel change, and each reports its evidence by level through `sx help
CORE` and `sx cores`. `sx-repomod` runs `discover.sx` and `status-offline.sx` over read-only
capabilities and prints through repomod's own text code. Gate evidence:

- The parity tests (26 in `hosts/repomod/tests`) match the Python commands' parsed `--json`, text,
  and exit status on a 16-repository fleet that covers every checkout variant and CI cache shape, on
  11 option sets for `status` and 5 for `discover`, and on every error path. A read-only shadow run
  against the live catalog matched too: `status --offline` on 34 repositories, `--failed` on 27, and
  `discover`.
- `uv run python -c "import importlib.util, sys; sys.exit(importlib.util.find_spec('repomod') is not
  None)" && uv run pytest tests cores` passes without repomod installed.
- repomod's suite, run from a copy, gives 595 passed and 1 skipped, and `../repomod` is unchanged.

Three deviations from the phase text. `Invalid` carries its error text, because the report prints
it. The host finds repomod on `PYTHONPATH` instead of declaring a path dependency, since a
dependency makes uv build into `../repomod` and the worktree layout moves it. `status` has no
`--checkout` or `--details`. The programs keep repomod's flat JSON row, so a missing checkout still
reports zero changes there, but the cores and the program decide on the enum.

Decided at the Phase 0 checkpoint:

- Keywords used as values, such as `{:kind :unknown}`, appeared in both rounds despite the guide.
  They stay errors with a habit diagnostic. Phase 2 records whether that diagnostic repairs them
  in one attempt, and a poor repair rate reopens the question.

Open:

- Watch items from one answer each: omitting an `Option` field from a record literal, a bare
  symbol as a catch-all `match` pattern, and branches returning records with different fields.
- The six held-out tasks that declare `Map`, `Float`, or `Bytes` cannot load into the registry.
  The Phase 2 harness must still show their tools to the model, as raw declaration text.

## Phases

Each phase ends at a gate. Do not start a phase until the previous gate passes.

### Phase 0: Skeleton, task corpus, and syntax pilot

This phase tests whether a model can learn the syntax before the checker and machine exist.

1. Create the uv workspace, ruff and ty configuration, and the kernel package with a test runner.
2. Write the probe's task corpus in `bench/codemode`. The corpus is described under
   [Task corpus](#task-corpus). Commit it before any model writes Sx for it. A held-out task
   does not change once any model has run on it.
3. Write the reader: source text to a syntax tree with source positions. Errors recover, so one
   bad form does not hide the errors after it.
4. Write the `sx help` guide by hand from `docs/language.md`. Phase 1 replaces it with the
   generated one.
5. Run the pilot. Give `opencode-go/iceman-contributor-free` the guide and each development task,
   several times per task. Run each answer through the reader, and read every answer by hand.
6. Change the syntax where the pilot shows a construct models keep getting wrong. Update
   `docs/language.md` to match.

**Gate:**
- `uv run pytest`, `uv run ruff check`, and `uv run ty check` pass.
- Every syntax change from the pilot is in `docs/language.md`.
- Every mistake that appeared in more than one pilot answer is a reader or checker fixture with
  its expected diagnostic. These fixtures fail until Phase 1 implements them.

#### Task corpus

The corpus has about 45 tasks. Each task has:

- A natural-language description.
- Capability declarations, with input and output types in Sx type notation.
- Fixture responses for every capability call, in the
  [JSON encoding](docs/language.md#values-and-json).
- The expected final value, as JSON.
- A JavaScript reference solution that produces the expected value.

The tasks cover filtering and grouping, parallel reads, conditional calls, partial failure
handled with `attempt`, retries of transient failures, polling with a deadline, schema mistakes,
composition of two cores, and tasks unrelated to repomod. Use repomod-shaped fixture capabilities
such as `catalog.list`, `git.status`, and `github.ci` for realism, but never call repomod.

Include tasks that JavaScript should win: string manipulation, arithmetic, and ad hoc logic.
Include paginated capabilities as real APIs have them. Do not shape any task around what Sx can
express. A task that needs a feature Sx v0 lacks, such as an object with arbitrary keys or a
fractional number, is marked `sx-expressible: false`. The gate leaves it out, and the report
states the share of such tasks as the expressiveness gap.

Split the corpus into a development set of about 15 tasks and a held-out set of about 30. The pilot, diagnostic tuning, and any language revision use only the development set. Only
the held-out set decides the Phase 2 gate. No model output on a held-out task informs a change to
the language, the guide, or the diagnostics.

### Phase 1: Python interpreter

Implement the language in `docs/language.md` in this order, on top of the Phase 0 reader:

1. Checker: name resolution, bidirectional type inference, effect inference, and authorization
   against a capability allow-list. It reports every error it finds in the
   [diagnostic format](docs/language.md#diagnostics).
2. Machine: pure evaluation with budgets.
3. Suspension: `call`, `par`, `par-map`, and `attempt`, with resumption by request ID.
4. JSON boundary: decode capability responses and encode results as specified in
   [Values and JSON](docs/language.md#values-and-json).
5. Registry: core and capability registration through a public API, with signatures readable
   without invoking anything. Every entry has a one-line description and at least one example.
6. Habit detectors: recognize forms from Clojure, Scheme, and JavaScript that are not Sx, such as
   `defn`, `nil`, `#(...)`, `->`, `(:key r)`, and reversed `filter` arguments. Each one says what
   to write instead. Start from the mistakes the pilot recorded.
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
- All fixtures pass, including the pilot fixtures from Phase 0.
- A test shows that a program rejected by the checker, or a program that names an unauthorized
  capability, produces no capability request.
- Every diagnostic code has a fixture that pins its full message.
- Every example in the guide and in `sx help NAME` runs as a test and produces its documented
  result.

### Phase 2: Model probe against JavaScript

Build the harness in `bench/codemode` that runs the Phase 0 corpus in two arms.

- A run is correct when its final value equals the task's expected JSON value.
- The Sx arm gets the `sx help` guide and the task's capability entries. Repairs feed back
  `sx check` or runtime diagnostics.
- The JS arm gets TypeScript declarations for the same tools and runs them as async functions in
  Node, with the same fixture responses. Repairs feed back `tsc` type errors before running and
  runtime errors after. JS gets its best realistic tooling, so a win comes from the language and
  not from weaker feedback.
- Run the model through the `pi` CLI with `--mode json`, `--no-tools`, `--no-skills`,
  `--no-context-files`, `--no-extensions`, `--no-mcp`, `--no-prompt-templates`, `--no-session`,
  and a fixed `--system-prompt`, so the model sees only the task. A one-line prompt then costs 69
  input tokens. An earlier opencode harness could not remove its built-in tools: the model tried
  to call them in up to 28 of 60 runs, and one looping run used 145,997 input tokens. Treat any
  tool attempt as a harness failure to fix, not a data point.
- Run `opencode-go/muse-spark-1.3-contributor` with `--thinking medium`. The probe uses a weak
  model on purpose: a language a weak model writes correctly is easy enough for a strong one.
- Muse Spark is the only probe model. The first held-out run also used
  `opencode-go/iceman-contributor-free`, a weaker free model. The owner dropped it after that
  run, because Muse Spark is the model in daily use.
- Read token counts, reasoning tokens, and stop reasons from the assistant `message_end` events
  in pi's JSON output.
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

Tune diagnostics and habit detectors on the development set before the held-out run.

The guide is the largest fixed cost in an Sx prompt, so the probe tracks token efficiency as
closely as correctness. `sx help` has three levels: `minimal`, a cheat sheet of signatures;
`standard`, the hand-written guide the pilot tuned; and `full`, the standard guide plus every help
example. `probe.py --measure-guides` reports each level's cost on the model's own tokenizer. The
report breaks every condition's tokens into the first prompt, total input, output, and reasoning,
with tokens per solved task.

Choose the guide level on the development set only. Run every level there, then take the level
with the fewest tokens per solved task among those within 5 percentage points of the best
first-attempt correctness. The held-out run uses that level for the gate. Other levels may run on
the held-out set as description, but never choose the gate's level.

Score each task as the mean over its samples, and compare the arms task by task, so repeated
samples of one task do not count as independent evidence. Report a bootstrap confidence interval
over tasks with every result. With about 30 held-out tasks, one task is about 3 percentage points.

**Gate:** On the held-out tasks Sx can express, for Muse Spark over the pairs it completed, Sx
first-attempt correctness is no more than 5 percentage points below JavaScript. Sx must also be
better on either final correctness or total tokens per solved task. The gate measures whether
models can write the language, so tasks that need a type Sx v0 lacks stay out of it. The report
gives the expressiveness gap, the share of held-out tasks Sx cannot express, beside the gate
result, because it measures the type set rather than the language. If the gate fails, revise the language once,
using only development-set evidence, and rerun the held-out set. If it fails again, stop the
project. Passing the gate means the prototype continues. It is not evidence that Sx beats
JavaScript in general. That claim needs the workflow comparison in the roadmap.

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
6. Programs: `discover.sx` and `status-offline.sx`. Command-line options such as
   `discover --owner` reach the program as [inputs](docs/language.md#inputs).

Test parity by running the Python command with `--json` and the Sx program on the same inputs.
Compare the parsed JSON and the exit status. Text output stays in Python: the host renders the
program's result with repomod's existing formatting code, and Sx never formats terminal text.
Build the inputs from repomod's own test helpers: copied catalogs, temporary Git repositories,
fake `gh` responses, and a fixed clock.

**Gate:** Both programs match the Python commands' `--json` output and exit status on every
parity fixture. The kernel test suite
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
