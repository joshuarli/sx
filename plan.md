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
or `write`) so the type system can reject parallel writes. The one `write` capability implemented
against a real system is Phase 6's `fs.write-text`, an atomic, idempotent file write confined to
granted roots.

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

Every phase, 0 through 9, has passed its gate. Phase 2 failed twice on the first held-out set, the
second time after its one allowed revision, and then passed on a fresh blind held-out set. At the
review before the Rust port, the owner accepted that pass and every recommendation below, and
extended the campaign through Phase 9. The Rust port is the implementation now, the Python
prototype is frozen as an executable specification, and the Lean reference checks every accepted
fixture. The six-workflow comparison in Phase 9c is the campaign's main finding: on real workflows,
TypeScript on Bun beat Sx on first-attempt correctness and on tokens, and Sx led only where a core
did the work. The owner then started the second campaign, Phases 10 to 12, to close that gap with
the guide, and opened repomod to the changes Phase 3 proposed. Every phase of the second campaign has
passed. On eight workflows written blind, Sx and TypeScript were both correct on 24 of 24 first
attempts. Sx still spends 1.8 times TypeScript's output tokens and 5 times its prompt tokens, and 3 of
its 24 answers ran only because the reader inferred closers from indentation. The third campaign,
Phases 13 and 14, cut Sx's cost 12% on the development set, but on a second blind set Sx still cost
2.4 times TypeScript per solved task, so its cost gate failed. The owner then chose the fourth
campaign, Phases 15 to 17, Sx as code mode for any MCP client and a benchmark where effects matter.
Phase 15 passed its gate. Phase 16's gate failed: on eight blind effects workflows both arms were
correct in all 96 sessions, and the one policy-violating effect was Sx's. Phase 17 passed: on 64 real
sessions over repomod, Sx answered 27 of 32 requests correctly against JavaScript's 32 of 32, in fewer
turns, at about the same cost.

**The Python prototype is frozen.** `src/sx` and the Python cores are frozen as of commit
`15b6ed6`, the parent of the commit that records this, because Rust passed its gates. The prototype
remains an executable specification for every unmarked fixture, and the Rust gates keep comparing
Rust with live Python on all of them. A bug fix to frozen behavior lands in Rust, Lean, and Python
together while Python can still run the case. New features land in Rust and Lean only, with fixtures
marked `"python": false` (run) or `; python: no` (check and read), which Python's tests skip and the
Rust and Lean gates judge against the fixture's own expectations. The differential fuzzer compares
Rust with Lean and keeps Python as a third participant for the features Python has. `sx-repomod` runs
all its commands on the Rust binary. Phase 6 onward is Rust and Lean only.

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
Instead, `standard` was tightened from 2,187 to 1,745 tokens by dropping rules for habits Sx now
accepts, which models follow without being told. On the development and retired sets it scored 40
of 41 at first attempt, against 38 of 41 before, at about 5 percent fewer tokens per solved task.

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

**Phase 4.** The gate passes. By the campaign lead's scope decision, the Lean reference in `lean/`
covers the semantic IR, where the semantics live, and the surface elaborator stays untrusted, the
way a proof assistant's kernel checks elaborator output. `sx check FILE --emit ir` exports the
checked IR with the types the elaborator inferred. The Lean side defines the IR, values, the JSON
boundary, an inductive typing relation `HasType` with an executable checker, a fiber machine written
from `docs/language.md`, and `sx-ref`, which replays a fixture. On all 123 accepted fixtures in
`fixtures/run`, `sx-ref` reproduces the outcome and the exact trace, and the Lean checker accepts
the exported program. The 13 rejected fixtures test the elaborator and are not compared. The
reference covers master's Float, Bytes, open records, literal patterns, widened records, `sleep`,
and `tuple-item`. Records keep their fields in name order in Lean types and values, so field order
does not matter, as in Python.
`lean/axioms.sh` builds every theorem below and requires each to use only `propext`,
`Classical.choice`, and `Quot.sound`:

1. `checked_program_type_safety` (from `pure_type_safety` and `check_sound`): a program the Lean
   checker accepts, with no call form, effect form, `loop`, or `recur`, run in a well-typed
   environment by the fuel-indexed evaluator, returns a value of its result type or stops with
   `arithmetic`, `invalid-input`, `out-of-range`, or `budget-exhausted`, never `internal`. This covers let, local
   functions, if, exhaustive match, records, lists, tuples, options, results, variants,
   conversions, the whole prelude through its signature table, and core calls whose
   implementations respect their signatures. The evaluator is not the machine. They share values,
   binding, matching, conversions, and the prelude, and `agree.py` checks the evaluator on the 27
   call-free fixtures.
2. `effect_soundness`: in every machine state reachable by running and resuming with any host
   responses, every request in the trace names a capability in the program's effect set.
3. `core_application_no_request`: applying a core function leaves the fiber returning a value and
   starts no fiber. `pure_program_no_request`: a program with no call form never requests a
   capability.
4. `budgets_respected`: the step, call, application, and delay counters never exceed their budgets.
5. `par_order_independent` and `par_results_by_index`: par results are assembled by branch index,
   whatever order the branches finish in.

Not covered: the elaborator, `loop` and `recur` in theorem 1, real core implementations, and
the `git` and `github` decision functions. Lean's JSON parser reads `1.0` as `1`, so an Int field
accepts `1.0` where Python rejects it. The Lean checker's rule for retrying a non-idempotent write
on refusal kinds omits Python's nesting-depth condition. The machine counts steps its own way, and
no fixture exercises the steps budget. The reference found two Python deviations from the
specification, now fixed with fixtures: a second `par` or `par-map` on the same fiber path reused
request IDs, and a `wait-until` predicate named by a local function saw the bindings at the call
site instead of its definition. Reproduce with `uv run python lean/agree.py` and
`lean/axioms.sh`.

**Differential fuzzing after Phase 4.** `tests/fuzz` generates typed random programs and runs
each through Python and Lean. About 105,000 programs, 75,000 of them accepted, found nine
disagreements, each now a fixture with the wrong side fixed: four in Lean (number matching in
the fixture host, `1e2` as an Int, the retry-write rule, a step count) and five in Python (a
self-containing bracket type that recursed, a late-resolved record sort key, a local `fn`
without parameters that asserted, lone surrogates in host strings, and a loop with no exit,
now the `loop-no-result` diagnostic). The final 30,000-program run found none.

**Phase 5.** The gate passes: Rust agrees with Python on every fixture and every saved model program,
with Lean on every accepted fixture, and the repomod parity tests pass on the Rust kernel. Gate evidence:

- Reader: 41 of 41 `fixtures/read` outputs are byte-identical.
- Checker: 121 of 121 `fixtures/check` outputs are byte-identical, diagnostics included. On the 127
  accepted `fixtures/run` cases the exported IR is identical to Python's, and on all 127 accepted and 18
  rejected cases the outcome, the exact trace, and the rejection codes agree with the fixture and
  with Python.
- Command line: about 1,990 cases of `sx check`, `explain`, `run`, `help`, and `cores` give the same
  stdout, stderr, and exit status as `uv run sx`, over every run and check fixture, the `fixtures/cli`
  matrix, every help entry, and the argument and file error paths.
- Differential testing: 976 saved model attempts from the probe check and run to the same verdict
  as Python with 0 disagreements, and a generated corpus of 2,000 programs checks identically in Rust
  and Python.
- Host boundary: `sx serve` gives the same outcome and trace on all 145 `fixtures/run` cases, and
  `sx-repomod` runs its 26 parity tests on the Rust kernel.
- Measurements: the next paragraph. The counts above are the gate's. The fixtures marked for Python at the
  freeze are not in them.

**Phase 5 host boundary and measurements.** `sx serve` speaks the JSON [host protocol](docs/language.md#host-protocol), 127 accepted and 18 rejected `fixtures/run` cases give the same outcome and exact trace through it, and `sx-repomod` runs its 26 parity tests on the Rust kernel. Release build, CPU time per operation (median of 9 interleaved samples, minimum to maximum in parentheses, none discarded by `perf-gate-ready`, which exited 0 although other lanes were compiling), Python kernel against Rust: parsing 33.9 (32.6-35.4) against 1.16 (1.13-1.23) microseconds per fixture program, and 2,039 against 63 for `status-offline.sx`; checking 46.4 (45.6-49.8) against 41.9 (39.5-50.1) per fixture program, and 1,715 against 468 for the status program; running with a fixture host 28.1 against 2.1 per fixture program, and 12.6 against 0.90 milliseconds for the status program on 70 recorded responses; 25.5 against 1.38 milliseconds for 59,000 machine steps of pure computation; one suspension and resume 21.3 (20.9-25.6) against 1.48 (1.45-1.58), and 13.6 against 1.29 per request in a 16-wide `par-map`. Half of Rust's cost to check a small program is the 22 microseconds the checker spends starting its 64 MiB stack thread, so small programs gain little from Rust there. Host dispatch: a Python host answering over the protocol costs 24.5 (24.1-26.3) microseconds of CPU per call in the client and the kernel together, 1.15 times the Python kernel with an in-process Python host, so the protocol is no faster than Python for trivial answers. Real capability calls take milliseconds, so the measurements do not call for PyO3. Reproduce with `uv run python bench/kernel/measure.py`, which builds the release binaries and takes about ten minutes. `uv run pytest bench/kernel/record_status.py` records the status program's responses from repomod's test fleet.

**Phase 6a: scoped authorization and the `fs` pack.** An execution's authority is a set of grants, each a capability ID with a scope value of the record type the capability declares, or with none. The kernel never reads a scope: the pack registers a check from scope and request input to a verdict, the checker applies it to inputs built only from literals and rejects a denied call with `out-of-scope`, and every host applies it to every request and refuses a denied one with `PermissionDenied`. `sx check`, `explain`, and `run` take `--grants`, `sx run --grants` without `--responses` performs `fs` capabilities for real, and `sx serve` takes `grants` and answers `check-scope`. The `fs` pack in `rust/sx-packs` reads text and bytes, lists, stats, and writes atomically, with the path rules and the residual race stated in [docs/language.md](docs/language.md#the-fs-pack). Gate evidence: 11 scope and error-table tests in `sx-packs` (`..`, `.`, file, directory, chained, dangling, and looping symlinks, a symlinked root, relative, NUL, sibling-prefix, and root-write paths, case on case-insensitive APFS, a name that is not UTF-8, and a denied read of a FIFO outside the roots that returns without opening it); 9 `sx run`, `check`, and `explain` tests against temporary directories covering every capability and the `NotFound`, `PermissionDenied`, and `Invalid` kinds (`Failed` is reached only through the error table); 2 `sx serve` tests in which the host asks `check-scope` and answers; 3 marked `fixtures/check` cases, two `out-of-scope` and one accepted with literal and computed paths; all 73 Rust tests, 642 Python tests, 36 repomod tests, 128 of 128 Lean agreements, 15 axiom-clean theorems, and a fuzz run with 0 findings. The non-UTF-8 listing is exercised only on Linux, and was not run there for this phase. In Lean, `out_of_scope_never_requests` proves that a program with a known call the scope check denies admits no machine, so it records no request, and `admitted_effect_soundness` keeps effect soundness for admitted programs. Both treat the check as an oracle and the checker's rejection as the gate. A stronger theorem, that every request an admitted program makes at a literal call site carries that literal, would need an invariant that tracks argument values through the machine's sequencing frames in every step lemma, so it is not proved; the run-time check covers those requests.

**Phase 6b: the `http` pack.** `http.request` and `http.get` send a request through h12tiny 0.7's blocking HTTP/1.1 client, with Rustls for `https`, and return `{status, headers, body}` for any status; a declarations file can also declare an HTTP endpoint as a typed capability, whose path and query come from its input, whose leftover fields are a JSON body for `POST`, `PUT`, and `PATCH`, and whose `2xx` body is decoded against the declared output by the kernel's decoder. Its scope is `{origins, methods}`; origins are normalized when the grant loads (lowercase, explicit default port, punycode, no userinfo or path) and compared exactly, so a denied request never opens a socket. Redirects are new requests, followed at most 5 times and only inside the grant, otherwise returned as is, and a cross-origin hop drops the program's headers. The closed status and transport error table, the 30 second deadline, the 8 MiB body cap, and why a decode failure is `Failed` and not `Invalid` for a write (`Invalid` is a refusal kind) are in [docs/language.md](docs/language.md#the-http-pack). Two kernel changes: a scope check now holds closures, so an endpoint's check can capture its literal origin and method, and the declaration loader hands entries with an `http` key to a binder. Gate evidence: 45 `sx-packs` integration tests against a `std::net` server in the test process (every capability and error-table row, open and closed record decoding with paths, path and query encoding, JSON bodies, redirects in and out of scope and through methods, scope denials for origin, port, scheme, method, userinfo, IDN, case, trailing dot, numeric hosts, and backslash and `%` tricks with the server's connection count asserted zero, the body cap, header and body timeouts, and a deadline spanning redirects); 6 `sx run`, `check`, `explain`, and `sx serve` tests with endpoints declared and granted from files; 2 TLS tests in which a rustls server with a committed test certificate is trusted through `SSL_CERT_FILE` and refused without it; the punycode and dot-segment unit tests; all 155 Rust tests, 643 Python tests, 128 of 128 Lean agreements, 15 axiom-clean theorems, and a fuzz run with 0 findings. Release `sx` grows from 3,254,240 to 6,480,144 bytes and a clean release build from 6.0 to 7.3 seconds (two interleaved runs each, `perf-gate-ready` exited 0 before and after), for 27 added crates (h12tiny's four, rustls, webpki, graviola, the macOS platform verifier and its Security.framework bindings, `http`, `bytes`, and small utilities; no serde, ring, or OpenSSL). A raw call that passes a `headers` map is not a literal to the checker, so only typed endpoints get a verdict before the run; HTTP/2 is not used. The macOS and `SSL_CERT_FILE` trust path was run, the Linux system store path was not.

TLS stays built in, so the release `sx` binary grew from 3.3 MB to 6.5 MB. HTTPS is required by
nearly every real API the packs exist for, a feature flag would be a knob no caller asked for, and
the binary stays an order of magnitude smaller than Bun's.
**Phase 6c: the `process` pack.** `process.run` runs one granted program directly, never through a shell, and returns `{status, stdout, stderr}`; a nonzero status is a value. Its scope lists absolute programs, canonicalized at grant load, each with argument patterns `Exact`, `OneOf`, `AnyOne`, and a final `Rest`, plus `fs`-style roots for the working directory. A request program must itself be absolute and canonicalize to a granted one, so a relative name, a `PATH` lookup, and a symlink to an ungranted program are denied. The child has an empty environment, a 60 second timeout, 1 MiB per output stream, and its own process group, which the host kills with `SIGKILL` on every exit path; the closed error table, including why invalid UTF-8 output is `Failed` and not `Invalid`, and what the scope does not confine, are in [docs/language.md](docs/language.md#the-process-pack). The pack needs one `unsafe` call, `killpg`, so `sx-packs` now declares its own lint table with `unsafe_code = "deny"` and a single allow. The scope check needed a kernel change: a record that omitted an optional field, or passed `none` or `(some x)`, counted as computed, so `process.run` calls never got a static verdict. `none` and `(some x)` of a literal now count as literal in the Rust checker and in the Lean `literalOf`, and the `omitted-option-field` run fixture computes its names so the frozen Python explain stays in parity. Gate evidence: 19 `sx-packs` tests (every argument pattern, malformed scopes, relative, `PATH`-looking, symlinked, and NUL programs, cwd escapes, a denied request that leaves no marker file, exact arguments with shell metacharacters, an empty environment asserted on `env` output, stdin including 512 KiB both ways and a program that ignores 4 MiB, nonzero status, signals, invalid UTF-8, the spawn error table, timeouts with a sleep, output caps on both streams, and no surviving process after a timeout, a cap, or a normal exit that leaves a background `sleep`); 8 `sx run`, `check`, and `explain` tests against temporary directories; all 100 Rust tests, 643 Python tests, 128 of 128 Lean agreements, 15 axiom-clean theorems, and a fuzz run with 0 findings. The timeout and cap tests use shorter limits through `process::execute`; the 60 second timeout itself is asserted as a constant and never waited for.

**Phase 7: script headers and the `decimal` core.** A program may be `(script {:inputs {:owners "List<String>"}} body)`, only as the outermost form. Each type is a string in the declaration type language, parsed by the declarations' parser, so Sx keeps one grammar, and the header declares no authority. `script-position`, `script-header`, `script-key`, and `input-type` are new codes, `reserved-name` and `no-encoding` cover the other header mistakes, and the reader's `duplicate-field` reports a repeated input; an input with a bad type stays in scope with no type, so one error does not cascade. A host that declares inputs too must agree with the header exactly or the run is a usage error naming the difference. `sx run report.sx --input owners='["acme"]'` decodes each value with the type-directed decoder and needs no host; a missing required input lists the required inputs and their types, an `Option` input may be left out, `sx explain` shows the inputs with their types, `sx help script` documents the form, and the protocol's `start` takes a script unchanged. The Rust help entry for `script` lives outside the table that the Python parity tests compare, and parity tests register only the Python cores. `decimal` is a Rust core over a new kernel concept, a nominal type whose core validates and canonicalizes: decoding hands the JSON string to the core, the checker refuses such a type as a sort key and points at `decimal.compare`, and `CoreError` gains an `Arithmetic` kind that the machine turns into `arithmetic`. A decimal has at most 38 significant and 38 fractional digits, arithmetic is exact, canonical text makes `=` numeric, and there is no dependency. Gate evidence: 85 marked fixtures (32 check, 53 run) covering every header diagnostic, input decoding success and failure through the protocol and the Rust judge, decimal parsing, arithmetic, all six rounding modes on ties, division by zero, overflow, places out of range, sort-key rejection, response decode errors, and equality; 11 `sx run`, `check`, and `explain` tests and 3 `sx serve` tests for scripts; 11,578 decimal vectors generated from Python's `decimal` module, with a test that fails when the committed file is stale, reproduced by the Rust core through the kernel's decoder and, independently, by a Lean implementation of the core; all 172 Rust tests, 645 Python tests, 36 repomod tests, 175 of 175 Lean agreements, 15 axiom-clean theorems, and a fuzz run with 0 findings. In Lean, `Sx/Decimal.lean` models the core on `Int` and `Nat`, `Sx/Cores.lean` supplies it to the machine as the trusted implementation, nominal decoding calls the same canonicalizer, and `sortKey` refuses a validated nominal, which required one line in the `cmpKeys_sortKey` proof. The fuzzer does not generate scripts or decimals, and the Lean sort-key rule is exercised by no fixture because Rust never emits an IR that violates it.

**Phase 8: MCP import and the MCP host.** `sx mcp import --name NAME` reads the tools of a stdio MCP server (revision `2025-06-18`, `initialize`, `tools/list` with cursors, `tools/call`; older `2025-03-26` and `2024-11-05` answers accepted) or a saved `tools/list` result, and prints declarations whose capabilities are `mcp-NAME.TOOL`: the reader allows one `.` in an ID, so the planned `mcp.NAME.TOOL` became `mcp-NAME.TOOL`. JSON Schema maps through a whitelist (objects, optional and nullable properties, string enums, lists, fixed-length tuples, maps, local non-recursive `$ref`); any other keyword skips the tool with its reason, and the annotations set the effect, with no grant. `sx mcp run` answers each request with a `tools/call` on one connection, honoring the concurrency budget with concurrent JSON-RPC ids, decodes `structuredContent` against the declared output, maps every failure to one call error kind (a decode failure is `Invalid` for a read and `Failed` for a write, as in the http pack), and kills the server's process group on every exit path through the process pack's group handling. No kernel change and no dependency: an imported tool is an ordinary capability with an `mcp` object the loader ignores, so `explain` shows it unchanged; the closed tables are in [docs/language.md](docs/language.md#mcp-tools). Gate evidence: 19 `sx-cli` tests against a fixture server (a test-only binary, no SDK) covering a golden import of 13 mapped and 27 skipped tools, import over a paginating server equal to the saved-list import, collisions and unusable servers, every row of the error table including timeouts with `notifications/cancelled`, a server that exits mid-run, calls answered out of order, up to 16 calls in flight on one connection, `none` omitted from arguments, and the server and a child it started killed after a normal run, a timeout, an error, and an import (the test hangs if the kill is removed); 3 `sx-packs` tests of the argument and result rewrites; all 177 Rust tests, 643 Python tests, 128 of 128 Lean agreements, 15 axiom-clean theorems, and a fuzz run with 0 findings. Release `sx` grows from 6,480,144 to 6,727,232 bytes with no new crate. An input property that is both required and nullable is skipped, because `none` is omitted and could not send the `null`. Sx does not verify the servers' hints or re-read their schemas at run time, and `SIGINT` to `sx` does not stop the server (stdin closes, and a well-behaved server exits). No third-party MCP server was run, because the specification was not available offline: the client and the fixture follow the subset as recalled.

**Phase 9a: the shared tool runtime.** `sx mcp serve --declarations D --grants G` serves the granted capabilities the binary performs natively as an MCP server over stdio (same protocol subset and revision as the client). It lists no ungranted tool, screens each call with the grant's scope check, and performs it through the same `sx_packs::perform` seam as `sx run`, so a new pack appears without a change. `inputSchema` and `outputSchema` are generated from the Sx types by the inverse of the import table. A `String` output is plain text, a `Record` or `Map` is structured, and any other output is wrapped as `{"result": ...}` and marked in `_meta`. A failure is an `isError` result with the call error kind in the text and in `structuredContent`. `sx mcp describe` prints the served set as Sx type strings. `bench/workflows/` holds a Bun MCP client (no SDK, concurrent ids, per-call timeouts) behind the same generated `api` object, the workflow format, a sandbox with temporary directories, deterministic local Git repositories and local HTTP services, and an Sx arm and a TypeScript arm that run one example workflow to the same outcome without a model. Round trip: `sx mcp import` of `sx mcp serve` reproduces input and output types, effects, and idempotence, except wrapped outputs (`Record {result: T}`), `Unit`, and `Bytes`, which the import whitelist cannot read.

**Phase 9b: the packs and cores the workflows need.** Three additions, no change to a gate. The `sqlite` pack bundles SQLite 3.54.1 through rusqlite 0.41 (MIT, `bundled` and `limits` only, no serde) and takes the `fs` scope for database files; `sqlite.execute` runs DDL, and a declarations entry with a `sqlite` object declares a typed statement whose `:name` parameters bind input fields, whose rows decode by column name, and whose `Int` output is the changed row count. Each call is one transaction, `ATTACH` is disabled so SQL cannot leave the grant, and a closed table maps constraint failures to `Conflict`, busy databases to `Unavailable`, and SQL errors to `Invalid` for a read and `Failed` for a write. The `yaml` core (`yaml.get`, `yaml.set`, `yaml.keys`) edits a documented block subset in place by replacing one scalar's byte span, with no dependency, and refuses flow collections, anchors, tags, multi-line scalars, several documents, and tab indentation with the line and the reason; the refusal reaches the program as the `invalid-input` failure kind. The `http` scope gains per-origin `headers` that the host adds to every request and redirect hop, with `{"env": NAME}` read when the grant loads; the program never sees them in a value, trace, `explain`, diagnostic, or error, a response echo of a value is redacted, and a request that sets an injected header is denied. Kernel changes: the declaration loader hands `sqlite` entries to the binder as it does `http` entries, and `ScopeCheck` gains an optional resolver for the written scope and an optional display function that hides secrets, so `sx_packs::Declared` replaces `http::Endpoints` in the CLI. Gate evidence: 29 sqlite tests (22 statement, 5 `sx run`, 2 scanner) covering every statement kind, parameter binding with hostile text, load-time errors, null and mismatch decoding, rollback, every error-table row, and scope denials through `..`, sibling prefixes, and symlinked files and directories; 13 yaml tests, whose golden cases (107 reads, edits, and failures over 7 documents, each also run with CRLF line ends, and 38 refused documents) check every edit against an independent replacement of the original text, and whose property tests build thousands of documents and check that an edit changes only its scalar and reads back for hostile values; 14 credential tests against a `std::net` server, including the secret absent from results, traces, `explain`, `check` diagnostics, and errors, and a program header of the same name denied; all 250 Rust tests, 645 Python tests, 175 of 175 Lean agreements, 15 axiom-clean theorems, and a fuzz run with 0 findings. Release `sx` grows from 6,833,184 to 8,884,880 bytes and a clean uncached release build from 7.3 to 16.0 seconds (two interleaved runs each, `perf-gate-ready` exited 0 before and after), for 7 added crates (rusqlite, libsqlite3-sys, smallvec, two fallible-iterator crates, `pkg-config`, `vcpkg`); the bundled SQLite compiles FTS5 and R-tree by default and needs a C compiler. The `yaml` quoting rule keeps a plain text scalar text, so setting `1.10` over `stable` writes `"1.10"`.

**Phase 9c: the six workflows and the comparison (Phase 9 result).** `bench/workflows/workflows/` holds `git-ci-audit` (100 local Git checkouts, four granted `git` argument patterns, a mock CI service, CI queries in `par-map` at most 8 at once), `rest-sqlite` (cursor pagination with scripted 429 and 503 answers, `retry` with stated delays, declared `sqlite` statements, an upsert that makes a repeated order take its later copy), `yaml-manifests` (six manifests including a CRLF file, a frozen owner, and a legacy file the core would refuse; every byte compared), `compiler-repair` (`/usr/bin/cc -fsyntax-only`, one fix per run, at most 6 runs), `object-store-index` (a store that rejects requests without the credential the grant injects from an environment variable; the check confirms the secret is in no program, result, or run transcript), and `mcp-inventory` (a stdio MCP server run through `sx mcp import` and `sx mcp run`, with an optional enum input). Both reference solutions of all six pass in both arms without a model. Muse Spark (`opencode-go/muse-spark-1.3-contributor`, medium thinking, the probe's two repairs, the standard Sx guide, generated TypeScript declarations) ran each workflow 3 times per arm, 4 trials at a time (36 trials; the `example-summary` sanity workflow ran too and is left out of the totals). Tokens are input plus output over all attempts; model seconds are the summed `pi` call time per trial, measured with trials running in parallel; run seconds are the summed arm run time of the answers.

| Workflow | Arm | Trials | First attempt | Final | Attempts | Tokens per trial | Output per solved task | Model seconds | Run seconds |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| compiler-repair | sx | 3 | 2/3 | 3/3 | 1.7 | 23,607 | 18,586 | 127 | 0.2 |
| compiler-repair | js | 3 | 3/3 | 3/3 | 1.0 | 5,335 | 4,547 | 35 | 0.2 |
| git-ci-audit | sx | 3 | 0/3 | 1/3 | 2.7 | 24,371 | 45,327 | 90 | 1.8 |
| git-ci-audit | js | 3 | 3/3 | 3/3 | 1.0 | 6,963 | 6,088 | 36 | 5.4 |
| mcp-inventory | sx | 3 | 0/3 | 3/3 | 3.0 | 19,477 | 11,031 | 120 | 0.1 |
| mcp-inventory | js | 3 | 3/3 | 3/3 | 1.0 | 2,682 | 1,943 | 15 | 0.1 |
| object-store-index | sx | 3 | 3/3 | 3/3 | 1.0 | 10,116 | 7,513 | 54 | 0.0 |
| object-store-index | js | 3 | 3/3 | 3/3 | 1.0 | 3,750 | 2,933 | 21 | 0.0 |
| rest-sqlite | sx | 3 | 2/3 | 3/3 | 1.3 | 9,697 | 6,213 | 34 | 0.4 |
| rest-sqlite | js | 3 | 3/3 | 3/3 | 1.0 | 3,390 | 2,535 | 25 | 0.4 |
| yaml-manifests | sx | 3 | 3/3 | 3/3 | 1.0 | 11,402 | 8,827 | 78 | 0.0 |
| yaml-manifests | js | 3 | 3/3 | 3/3 | 1.0 | 18,033 | 17,253 | 187 | 0.1 |

Over the six workflows (18 trials per arm): Sx first-attempt correct 10 of 18 and final 16 of 18, TypeScript 18 of 18 and 18 of 18. Mean tokens per trial 16,445 for Sx and 6,692 for TypeScript (2.5 times); output tokens per solved task 12,615 and 5,883; model seconds per trial 84 and 53; attempts per trial 1.8 and 1.0. The two failed Sx trials are both `git-ci-audit` (1 of 3 solved).

Host code, in non-blank lines of the files a host writes: `declarations.json` plus `grants.json` plus `server.json` is 27 for `git-ci-audit`, 42 for `rest-sqlite`, 6 for `yaml-manifests`, 11 for `compiler-repair`, 25 for `object-store-index`, and 5 for `mcp-inventory`. Both arms use the same files, because both reach the same performers. The TypeScript arm additionally needs its `api.d.ts` (17 to 22 generated lines, 14 for the MCP workflow) and the 407-line Bun harness, which is written once and not per workflow. Sx needs neither.

**What `sx explain` reported before the Sx answers ran.** It reports only for an answer the checker accepts (17 of the 32 Sx answers, the other 15 were rejected with diagnostics and no `explain`). For every accepted answer it listed each capability called, its effect class (`process.run` as `write, not idempotent`, `fs.write-text` as `write, idempotent`, `store.put` as `write, idempotent`, the imported `reserve-stock` as `write, not idempotent`), and whether it was granted. Scope verdicts were `allowed` only where the input was a literal (`sqlite.execute` with its database path, `fs.read-text` of `repos.txt`) and `checked per request` for every computed path, URL, and command (51 of 60 verdicts across the accepted answers), and an imported MCP tool carries no scope, so it has none. Static authority therefore told a host which tools and effects a program uses and not which files, commands, or hosts, as the roadmap's second weakness predicts. The TypeScript arm has no equivalent before running: its host sees the tools it lists (the generated `api.d.ts` and the tools `sx mcp serve` offers, both limited to the grant) and learns the calls and their inputs only as they happen.

**Where TypeScript did better.** First attempt: 18 of 18 against 10 of 18, and 2.5 times fewer tokens per trial. The Sx misses were habits the guide does not cover, not logic: the imported enum `priority` takes the string literal `"rush"`, and all three samples wrote `rush` and then `Rush`, so each needed both repairs (neither diagnostic names the string form, and the second says `Rush` is "not a variant of any enum in scope" while the enum is in scope); `get` with a default on a field that is always present, an `Option` where a `String` was needed, an unbalanced bracket in a long program, a name starting with `_`, and a missing `attempt` around a call that fails for a directory that does not exist. On `git-ci-audit` Sx could not do what TypeScript did: `process.run` is a `write`, `par-map` is read-only, so the 400 Git runs go one after another in Sx (reference 5.2 seconds against 0.8 seconds in TypeScript, same performers) while only the CI queries run in `par-map`.

**Where Sx did better.** `yaml-manifests`: 11,402 tokens per trial against 18,033, 8,827 output tokens per solved task against 17,253, and 78 model seconds against 187, because the `yaml` core edits scalars in place and the TypeScript arm has no such core and hand-writes line scanning (all three of its answers were correct on the final wording). The first run of that workflow had ambiguous wording ("NAME" for the file name) that two TypeScript answers read as the name without `.yaml`; the wording was fixed and the workflow rerun for both arms, and the earlier run is not counted. Credential handling was identical in both arms, since the same performer injects the header: the check, which every correct `object-store-index` answer in both arms passed (6 of 6), requires the secret to be in no program, result, or run transcript and the store to have seen one authorized `PUT`. Sx also rejected its 15 broken answers before any tool ran, but the benchmark did not measure whether that mattered, since no answer was harmful.

**Gaps and cautions.** The sample is one model, 3 samples, 6 workflows written by the language's author, so the 10-of-18 and 2.5 times figures carry the held-out warning from the probe; a workflow written beside the language overstates transfer. Wall time was measured with 4 trials running at once. A refused core call (the `yaml` refusal) is not caught by `attempt`, so `yaml-manifests` excludes the file the core would refuse by name; the check does not depend on the failure kind's name. `sx mcp import` omits `idempotent` on a non-idempotent write, which the runner supplies as `false` when it builds the TypeScript declarations. Open: a `par-map` that may call `process.run` for a granted read-only command (a gap, not fixed here); a diagnostic for an unknown lowercase name where an enum is expected that suggests the string literal.

**Phase 10a: the baseline and its failures.** The Sx arm of the six workflows on master at `1fb7eca`,
Muse Spark at medium thinking, standard guide, 3 samples: first attempt 11 of 18 (9c: 10), final 17 of
18 (9c: 16), mean tokens per trial 14,669 (9c: 16,445). TypeScript in 9c was 18 and 18 of 18 at 6,692.
Every attempt that was not correct, by cause:

| Cause | Trials | Attempts |
| --- | --- | --- |
| Records with different fields joined into one list, then read as if a flag such as `:known` narrowed them, as a TypeScript discriminated union does (`type-mismatch` or `not-a-record` on the now-optional field) | 3 | 3 |
| Unbalanced brackets in a program written as one long nested `let` | 2 | 3 |
| A default on a field that is always present (`arity`), written to repair the join error | 1 | 1 |
| No `attempt` around `process.run` in a directory the task said may be missing (`NotFound`) | 1 | 1 |
| A loop accumulator that starts as `[]` and is read before its `recur` (`unresolved-type`) | 1 | 1 |
| Compiler diagnostics hand-parsed by splitting on `:` with a wrong part count, which TypeScript does with a regular expression (wrong result, not repaired) | 1 | 1 |

The development probe runs (r6 and the retired held-out set) had too few rejections to add a cause.
Writing six more workflows (Phase 10b) found four gaps: no way to decode a JSON file, no local
function that takes a function argument (`fn-position`), the untyped `[]` above, and `par-map` limited
to reads, so non-idempotent POSTs run one at a time.

**Phase 10b.** `log-triage` (14 logs, two with CRLF, counts with a tie rule, a summary file compared
byte for byte), `invoice-totals` (the `decimal` core, rounding half away from zero per line, stated
against computed totals), `deploy-rollout` (non-idempotent POSTs retried only after a 429, polling
every 100 ms with a 1 second deadline, rollback only for failures), `issue-notify` (page-number
pagination to an empty page, duplicates across pages, declared statements over a seeded database, a chat
post that fails with 503), `calendar-mcp` (a stdio MCP server with nested structured results and an
optional field, error results for hidden holds), and `dependency-audit` (`semver`, at most 4 registry
requests in flight). Both references of each pass, and wrong variants of each were rejected by its check.

**Phase 11, iteration 1: the tailored guide.** On master at `ea06e69` (no language change yet), the
twelve development workflows, 3 samples each, Muse Spark at medium thinking. Prompt and output are
tokens per trial. TypeScript got the tool notes (`--ts-notes`).

| Condition | First attempt | Final | Prompt | Output | Output per solved task |
| --- | --- | --- | --- | --- | --- |
| Sx, standard guide and listing | 20 of 35 (57%) | 33 of 35 | 5,276 | 13,319 | 14,126 |
| Sx, tailored guide | 28 of 36 (78%) | 36 of 36 | 7,211 | 10,790 | 10,790 |
| Sx, tailored guide and the leave-one-out corpus | 22 of 36 (61%) | 34 of 36 | 20,994 | 10,659 | 11,286 |
| TypeScript, tool notes | 36 of 36 | 36 of 36 | 1,078 | 5,952 | 5,952 |

(One standard trial of `dependency-audit` did not finish.) The tailored guide gained 21 points and cut
output by a fifth. The corpus of eleven other workflows' tasks and programs gained nothing over the
tailored guide, at three times the prompt, and its answers broke brackets more often, so it is
dropped. The eight tailored misses were five `unresolved-type` (a helper called with `[]`, and a loop
accumulator starting as `[]`) and three bracket errors. On the development probe tasks (2 samples of
17), the tailored guide scored 33 of 34 first attempts against 34 of 34 for the standard guide, one
bracket error, with output 3,093 tokens per trial against 2,309.

**Phase 11, iteration 2: definitions, regex, and the checker fixes.** Between the iterations, master
gained top-level `def` and `defn`, defaults on fields that are always present, a diagnostic that says
why a joined field is optional, loop accumulators that start as `[]` typed from their `recur`, the `re`
core with `#"..."` patterns, and a tailored guide rewritten as one short definition per step. The
runner now gives every workflow the `text` and `re` cores. Same twelve workflows and samples:

| Condition | First attempt | Final | Prompt | Output | Output per solved task |
| --- | --- | --- | --- | --- | --- |
| Sx, tailored guide, iteration 2 | 33 of 36 (92%) | 36 of 36 | 7,113 | 7,352 | 7,352 |
| TypeScript, tool notes | 36 of 36 | 36 of 36 | 1,078 | 5,952 | 5,952 |

Output per solved task fell from 14,126 (standard) and 10,790 (iteration 1) to 7,352, 1.24 times
TypeScript's. The three misses: two `calendar-mcp` trials called a helper with `[]` that only another
call types (`unresolved-type`), and one bound a name `ok`, which is reserved (`reserved-name`). The
`json` core and an `attempt` that catches core refusals landed after this run.

**Phase 11, iteration 3.** Master then gained the `json` core (read by path, like `yaml`, with
a strict RFC 8259 reader of its own and a Lean model checked against its golden cases), an `attempt`
that catches a core's refusal as kind `Invalid` (`arithmetic` and `out-of-range` stay uncatchable;
Lean has 18 axiom-clean theorems), an empty argument to a local function typed from the function's
other calls, `concat` joining record element types, `when` (an `Option`) and `do`, capitalized local
names, and a guide that teaches them. The runner gives every workflow `json` too. Same twelve
workflows: first attempt 31 of 36 (86%), final 36 of 36, prompt 7,637, output 6,822 per trial, 1.15
times TypeScript's. Three misses were names a TypeScript habit produces (`_stored`, a loop variable
named `attempt`, a binding named `err`), one called a `defn` defined further down, as TypeScript's
hoisted functions allow, and one indexed past the end of a short line (`out-of-range`), a logic error.

**Phase 11, the final language and the gate.** A last lane let a name start with `_`, let a form
name such as `attempt`, `err`, or `ok` name a value (head position and patterns keep the form), and let
top-level `defn`s use definitions written later, as TypeScript hoists functions; the reader also infers
missing or extra closers at the ends of lines from indentation when brackets do not balance, choosing
the fewest edits, and reports every inferred closer to the host. The owner left whether inference
should be opt-in to the campaign lead, who kept it always on with no switch: on the workflow
corpus it turned 13 of 19 bracket failures into correct runs and none into a wrong one, the program
it produces passes every check that any program must, and a strict host can refuse any report
that carries `inferred_closers` before an effect happens. The guide froze at `d701604`. On the
twelve development workflows with that binary: first attempt 33 of 36 (92%), final 35 of 36, prompt
7,431, output 7,297 per trial. The gate (within 10 points of TypeScript on the development set) passed
from iteration 2 on. The development probe tasks showed no first-attempt regression (33 of 34 against
34 of 34, one bracket error), with output 34% above the standard guide's on those short tasks.

**Phase 12: the blind workflows (the second campaign's result).** A subagent that read neither the
guide, this plan, nor any result wrote eight workflows in `bench/workflows/blind/`: `canary-rollout`,
`directory-sync`, `export-jobs`, `package-report`, `release-notes`, `report-archive`, `task-triage`,
and `usage-invoices`, using the `fs`, `process`, typed `http`, and `sqlite` packs, a stdio MCP server,
and the `semver`, `text`, `re`, and `json` cores. It tried 38 wrong variants, and every check rejected
them; the campaign lead read only the reference verdicts, all correct in both arms on the frozen
binary. Muse Spark, medium thinking, 3 samples, both arms once, TypeScript with the tool notes:

| Arm | First attempt | Final | Prompt per trial | Output per trial | Model seconds per trial |
| --- | --- | --- | --- | --- | --- |
| Sx, tailored guide | 24 of 24 | 24 of 24 | 6,894 | 11,227 | 100 |
| TypeScript, tool notes | 24 of 24 | 24 of 24 | 1,352 | 6,254 | 54 |

Sx caught up on correctness by the gate's rule: within 10 points at first attempt and 5 at final, here
level. It did not catch up on cost. Output tokens were 1.8 times TypeScript's on every one of the eight
workflows (1.6 to 2.1), against 1.15 to 1.24 times on the development set, so the development set
overstated how far the model's own cost had fallen, and total tokens per trial are 2.4 times
TypeScript's, as in Phase 9c, because the guide adds 5,500 prompt tokens that buy the correctness. Three
of the 24 Sx answers had unbalanced brackets and ran correct only through closer inference; without it
Sx would have been 21 of 24 at first attempt, 12.5 points behind, outside the gate. No answer in either
arm performed an effect and was then wrong, since every first attempt was correct. The author recorded
three needs Sx could not meet and designed around: writing JSON text (the `json` core only reads),
creating, moving, or deleting files (`fs` has none of these, so one workflow grants `/bin/mkdir`,
`/bin/mv`, and `/bin/rm`), and date arithmetic. The sample is one model, 3 samples, 8 workflows.

**Phase 13: where the cost went.** The twelve development workflows, 3 samples, Muse Spark, the
Phase 12 binary. Cost is dollars at the provider's prices (input $0.10, output $0.20 per million, no
prompt caching), per solved task.

| Condition | First attempt | Final | Prompt | Output | Reasoning | Cost per solved task |
| --- | --- | --- | --- | --- | --- | --- |
| Sx, frozen guide, medium thinking | 33 of 36 | 35 of 36 | 7,431 | 7,297 | 6,415 | $0.00227 |
| The same, run again | 34 of 36 | 35 of 36 | 6,919 | 7,403 | 6,467 | $0.00223 |
| Slim guide (no TypeScript table, cores as function lists) | 33 of 36 | 34 of 36 | 5,160 | 7,650 | 6,765 | $0.00217 |
| "The host checks the whole program before anything runs, so write directly" | 33 of 36 | 35 of 36 | 7,119 | 6,473 | 5,636 | $0.00206 |
| That sentence, and no checklist | 36 of 36 | 36 of 36 | 6,410 | 6,518 | 5,666 | $0.00194 |
| Slim, that sentence, and no checklist | 33 of 36 | 36 of 36 | 5,229 | 7,766 | 6,803 | $0.00208 |
| Sx, frozen guide, low thinking | 29 of 36 | 34 of 36 | 7,717 | 3,026 | 2,196 | $0.00146 |
| TypeScript, medium thinking | 36 of 36 | 36 of 36 | 1,078 | 5,952 | 4,815 | $0.00130 |
| TypeScript, low thinking | 34 of 35 | 34 of 35 | 1,077 | 2,317 | 1,258 | $0.00059 |

The repeated baseline moved cost by 2%. Cutting the guide saved prompt tokens but lengthened reasoning
by about as much, so the TypeScript table and the core prose pay for themselves. The checklist cost
reasoning and bought nothing. Telling the model that a rejected program costs only another try cut
reasoning by an eighth. Together these two lower cost 14%, short of the gate's quarter, and `sx guide`
now ends with that sentence in place of the checklist. Low thinking cuts Sx's cost by a third but
costs 11 points of first attempts, while TypeScript loses almost nothing at low thinking, so at equal
settings the gap widens from 1.7 to 2.5 times. The remaining dev gap is mostly the prompt: Sx's
reasoning is now within a fifth of TypeScript's here, and its 6,400 prompt tokens are a third of
its cost.

**Phase 14: the second blind set (the third campaign's result).** A subagent under Phase 12's rules
wrote `bench/workflows/blind2/`: `catalog-sync`, `customer-merge`, `export-jobs`, `release-tagger`,
`service-config-audit`, `task-assign`, `tidy-text-files`, and `usage-report`; both references of each
are correct on the measured binary (`35994e4`, the guide that ends with the write-directly sentence
and has no checklist). 3 samples, both arms, TypeScript with the tool notes. One Sx trial at medium
thinking (`export-jobs` sample 0) was lost when a reply without a reasoning count crashed the runner,
fixed in `96780bd`; its rerun was stopped when the session ended, so that row has 23 trials.

| Arm, thinking | First attempt | Final | Prompt | Output | Reasoning | Cost per solved task |
| --- | --- | --- | --- | --- | --- | --- |
| Sx, medium | 21 of 23 | 22 of 23 | 7,597 | 11,114 | 9,687 | $0.00312 |
| TypeScript, medium | 23 of 24 | 24 of 24 | 1,433 | 5,785 | 4,495 | $0.00130 |
| Sx, low | 21 of 24 | 24 of 24 | 8,092 | 4,480 | 3,204 | $0.00171 |
| TypeScript, low | 21 of 24 | 24 of 24 | 1,656 | 2,685 | 1,448 | $0.00070 |

The cost gate failed: Sx costs 2.4 times TypeScript per solved task at both thinking levels, against a
target of 1.6. Correctness stayed within the 10 points (91% against 96% at medium, level at low). The
development set's 12% saving (confirmed on the shipped guide: 34 of 36, $0.00199) did not reach unseen
tasks, where Sx's reasoning stays twice TypeScript's, as on the first blind set (9,968 against 4,857).
On familiar tasks reasoning nearly closed; on new ones the model still plans at length in a language it
has not seen. The blind author recorded two needs Sx could not meet: bounded concurrent writes (`par`
and `par-map` refuse writes) and writing JSON.

**Handoff.** Everything is committed on master; nothing from Phases 13 and 14 is pushed. The raw trials
are under `.work/workflows/` (disposable; the tables above are the record). To resume the lost trial:
`SX_BIN=<binary built at 35994e4> uv run python bench/workflows/runner.py trials --set blind2
--workflow export-jobs --arm sx --guide tailored --samples 1 --out .work/workflows/blind2`. The owner
chose items 1 and 2 as the fourth campaign, planned under Phases 15 to 17, with item 3 folded in where
its tasks need it, and accepted item 4:

1. **Sx as code mode for any MCP client:** one MCP tool that takes an Sx program, describes itself with
   `sx guide`, checks and explains the program's exact calls, and only then runs it against the host's
   real tools, so real agent sessions, with a strong model as well as Muse, replace synthetic tasks.
2. **A benchmark where effects matter:** destructive writes, policies a program can violate, partial
   effects before a failure, and a host approving exact calls before anything runs, which is where
   Sx's static authority should win and which no campaign has measured.
3. **The gaps the blind authors designed around:** writing JSON, creating, moving, and deleting files in
   `fs`, date arithmetic, bounded concurrent writes, and parallel read-only commands.
4. **Stop tuning cost** beyond this: the remaining gap is reasoning on unseen tasks, and a host with
   prompt caching would make the guide's prompt tokens nearly free.

**The repomod track passed.** repomod gained `default_catalog`, `select_repositories`, `print_json`,
`discover_owner`, `checkout_status`, `todo_item_count`, `status_text`, and `render_discover`, and
`sx-repomod` calls them. repomod's suite gives 595 passed and 1 skipped, ruff and ty pass, and the 36
parity tests pass. Both commits are local to each repository.

Decided at the Phase 0 checkpoint:

- Keywords used as values, such as `{:kind :unknown}`, appeared in both rounds despite the guide.
  They stay errors with a habit diagnostic. Phase 2 records whether that diagnostic repairs them
  in one attempt, and a poor repair rate reopens the question.

Open after the second campaign:

- **The model's own cost.** On blind workflows Sx answers are as correct as TypeScript's but take 1.8
  times the output tokens and nearly twice the model time, and the prompt is 5 times larger. The
  output is mostly reasoning, so the next lever is whatever lets a model plan in Sx as fluently as in
  TypeScript, measured on new blind workflows.
- **Writing JSON, managing files, and dates.** The blind author designed around a JSON writer, `fs`
  operations to create directories and move or delete files, and date arithmetic.
- **Parallel read-only commands.** Still open from the first campaign.
- **A timing-dependent test.** `calls_run_concurrently_and_each_answer_carries_its_own_id` in
  `rust/sx-cli/tests/mcp_serve.rs` asserts that the shortest of eight sleeps answers first, which failed
  once under the full parallel test run.

Open after the first campaign, taken up by Phases 10 to 12:

- **Close the workflow gap.** Sx trailed TypeScript 10 of 18 against 18 of 18 at first attempt on
  the six workflows, at 2.5 times the tokens. The guide teaches the kernel but not the packs, so a
  model meets `fs`, `http`, `process`, `sqlite`, and MCP tools only through the listing. Measure
  a pack section of the guide on new workflows written blind, as Phase 2's fresh set was.
- **Parallel read-only commands.** `process.run` is a write, so `par-map` cannot run the 400
  `git` reads of `git-ci-audit` in parallel. Typed declared commands, like typed endpoints and
  sqlite statements, would let a host declare `git status` a read.
- **Catching a core refusal.** `attempt` catches call errors only, so a program cannot recover
  from a `yaml` refusal; the workflow had to exclude the refusable file by name. Either `attempt`
  catches `invalid-input` or refusing cores gain a checking function.
- **Proposed repomod changes:** make the private helpers `sx-repomod` imports public and extract
  `render_discover` (Phase 3). The repomod track does this.

## Phases

Each phase ends at a gate. Do not start a phase until the previous gate passes.

### The second campaign: closing the workflow gap

The owner set the goal after Phase 9c: find out whether the guide, measured on workflows written
blind, can bring Sx level with TypeScript, which models have seen billions of lines of. Prompt
tokens may grow to get there; output tokens, attempts, and correctness are what must close. The
owner also opened `../repomod` to changes.

#### Phase 10: Evidence

- **10a.** Rerun the Sx arm of the six workflows on current master, 3 samples, keeping every
  prompt and answer, and classify every rejection, failure, and wrong answer by cause. Add the
  diagnostic codes of the development and retired held-out probe runs. The fresh set stays unread.
- **10b.** Six more development workflows in shapes the first six lack, written by a subagent the
  campaign lead may read, with both references passing, and a TypeScript baseline on them. The
  twelve are the development set.

**Gate:** a failure taxonomy with counts per cause, recorded in Status.

#### Phase 11: The tailored guide and the fixes the taxonomy names

- **A guide composed from the declarations.** `sx help --level tailored --declarations D` prints
  the kernel guide, a section that maps TypeScript habits to Sx, a section for each pack and core
  the declarations use, a call template written in Sx for each capability, worked examples at
  workflow scale for the packs in use, and a checklist of the mistakes the taxonomy counts. Pack
  sections live beside their packs. Every example in the guide type-checks against the
  declarations it is shown with, and every kernel example runs.
- **Language and tool changes the taxonomy justifies**, each with fixtures, the Lean reference
  where semantics change, and the contract change surfaced: candidates are declared commands that
  a host may classify as reads, an `attempt` that catches a core's refusal, and habit acceptance.
- **Repair feedback that teaches:** a diagnostic carries the guide entry for what it names.
- Iterate on the development set, and record each iteration's numbers in Status.

**Gate:** on the development set, Sx first-attempt correctness within 10 points of TypeScript, and
no regression on the development probe tasks against the standard guide. After four iterations
without the gate, Phase 12 runs anyway and the result says so.

#### Phase 12: Blind workflows and the measurement

- A subagent that sees neither the guide, the taxonomy, nor any development result writes eight
  workflows in the format of `bench/workflows`, with both references passing. The campaign lead
  reads only the runner's reference verdicts. The set freezes before any model runs on it.
- Both arms run once, 3 samples each, with the guide frozen.
- Report first-attempt and final correctness, prompt tokens and output tokens separately, output
  tokens per solved task, and the wrong runs that performed effects before a correct one.

**Gate:** the result is recorded in Status. Sx has caught up if its first-attempt correctness is
within 10 points of TypeScript's and its final correctness within 5.

### The third campaign: cost

The owner set the goal after Phase 12: reduce what an Sx answer costs, expecting that it will not
match TypeScript. On the blind set an Sx trial cost $0.0029 against $0.0014 at the provider's prices
(input $0.10 and output $0.20 per million tokens, no prompt caching): the programs are smaller than
TypeScript's, the reasoning is twice as long (9,968 tokens against 4,857), and the guide makes the
prompt five times larger.

#### Phase 13: Where the cost goes, and what removes it

On the twelve development workflows, 3 samples, each against the frozen Phase 12 baseline (iteration
4 for Sx, the tool-noted TypeScript run): a slimmer guide (no TypeScript table, cores as function lists),
an instruction that the host checks the program before running anything so the model may write
directly, and low thinking, run for TypeScript too so the comparison stays at equal settings. Combine
what lowers cost without costing correctness, then make it the guide `sx guide` prints.

**Gate:** a combination that lowers Sx's dollar cost per solved task by at least a quarter on the
development set with first-attempt correctness within 5 points of the baseline.

#### Phase 14: A second blind set

A subagent writes eight more blind workflows under `bench/workflows/blind2/` under Phase 12's rules.
Both arms run once at the same model settings. **Gate:** the result is recorded. The cost target is
Sx at most 1.6 times TypeScript's dollar cost per solved task, from 2.1, with first-attempt correctness
within 10 points.

### The fourth campaign: code mode and effects

Phase 15 passed; Phase 16 failed its gate; Phase 17 passed. The owner chose it after Phase 14 and accepted every recommendation in the
handoff. Two questions no campaign has answered: does Sx work as the code mode of a real agent, and
does its static authority prevent harmful effects that JavaScript code mode lets happen? Cost is not a
goal here; it is reported.

The agent is pi, which the owner uses, not Claude Code. pi connects to MCP servers (user-level
`~/.pi/agent/mcp.json`, or `.pi/mcp.json` in a project the owner has trusted), restricts the tools a
model sees with `--tools`, and has its own code mode: the `codemode` tool runs JavaScript in a QuickJS
sandbox that calls pi's tools and MCP tools, and its calls before a failure are not undone. That
built-in code mode is the baseline, so both arms run in the same agent with the same model and tools.

What Sx can claim and TypeScript cannot, which this campaign must measure rather than assume:

- A program that does not check performs no effect. pi's `codemode` runs JavaScript with no type check,
  so every mistake surfaces at run time, after the calls before it; the Phase 9 to 14 arm had `tsc`,
  which this baseline lacks. Sx also catches what no type checker would: a call the host did not grant
  or that a literal input puts out of scope, a retried write that is not idempotent, a write inside
  `par`.
- Before anything runs, `sx explain` lists every tool a program can call, with each literal input and
  its scope verdict, so a host can approve or refuse the program as a whole. A JavaScript host learns
  the calls only as they happen. The same performers enforce the grants at run time in both arms, so
  run-time refusal is not an Sx advantage.

#### Phase 15: Sx as code mode for any MCP client

- **`sx mcp code`**, a stdio MCP server that offers two tools: `check`, which checks a program and
  returns its diagnostics or its `explain` report, with no effect, and `run`, which checks, explains,
  applies the host's approval policy, and runs, returning the result, the calls made with their
  outcomes, and any `inferred_closers`. Both tools describe themselves with the output of `sx guide`
  for the server's declarations and grants. The tools behind them are the native packs and any MCP
  servers the host names, imported with `sx mcp import` and called as `sx mcp run` calls them, so code
  mode wraps a host's existing MCP servers.
- **An approval policy**, a JSON file the host writes, judged against the `explain` report before
  anything runs: the capabilities a program may call, which of them may be called with an input the
  checker cannot see (a verdict of `checked per request`), and a limit on write calls in the program
  text. A refused program returns the reason and performs nothing. MCP elicitation, which would let a
  person approve the exact calls, waits until a client that supports it is in hand; the policy file
  is the approver until then.
- **The pack gaps an effects benchmark needs:** `fs.make-directory`, `fs.move`, and `fs.delete` under
  the `fs` scope rules (destructive operations get the os-boundaries treatment the write path got:
  no symlink traversal, whole-component containment, and an error table), and a way to write JSON
  text from a value. Dates, bounded concurrent writes, and parallel read-only commands wait until a
  task needs them.
- **In pi:** `pi mcp add sx-code -l --exposure direct -- sx mcp code --declarations D --grants G --policy P`
  in a project under `.work`, so a model sees `mcp__sx_code__check` and `mcp__sx_code__run` (pi replaces
  every character outside `[A-Za-z0-9_]` in a tool name with `_`). `pi -p --approve` trusts the project
  for one run, so no owner step is needed, and `-ne -e builtin:mcp` keeps only pi's MCP support, because
  MCP is a built-in extension that `--no-extensions` alone turns off.
- **Gate:** an MCP client test drives `check` and `run` over stdio against native and proxied tools,
  including refusals by the policy that leave the sandbox untouched; a pi session in print mode solves
  a small task through `sx mcp code` end to end; the new `fs` operations pass scope and error-table
  tests like the existing ones; all existing gates pass.

**Phase 15 result: passed.** `sx mcp code` serves `check` and `run`. The guide is listed once, in
`run`'s description, because both copies cost every turn about 13 KB for the fs grants. The approval
policy is `{"capabilities", "computed_inputs", "max_write_sites"}`, validated against the declarations
at startup, and its refusals are `capability-not-allowed`, `computed-input-not-allowed`, and
`too-many-write-sites`, each naming the call site. `fs.make-directory` (with parents, idempotent),
`fs.move` (never replaces: `Conflict`, not idempotent), and `fs.delete` (a file, a link, or an empty
directory, never recursive) follow the write rules, and already-exists joined the error table as
`Conflict`. The JSON writer is the prelude function `(to-json x) -> String`, in Rust and Lean, with keys
sorted by code point so equal values give equal text, and a type with no JSON encoding is the check
error `no-encoding`. Evidence:

- `mcp_code` (16 tests over stdio), `fs_pack` (18), and the whole Rust workspace pass, with clippy clean;
  the fs tests also pass on Linux in Docker as an unprivileged user, which exercises `renameat2`.
- Lean agrees on all 220 accepted run fixtures, 11 of them new; the axioms check, the fuzzer's fast slice,
  the Python fixture tests, and the 36 repomod parity tests pass.
- Muse Spark at medium thinking, in pi with only `mcp__sx_code__*`, sorted tagged notes into directories
  with `fs.make-directory` and `fs.move` and wrote the index, correctly, in 8 turns and 7 tool calls for
  $0.0022 (56,343 of the input tokens were cache reads). On the way the policy refused one program before
  any effect, because the gate's policy left `fs.list` out of `computed_inputs`, and two programs did
  not check because they called `re.find` and `text.split`, cores the gate did not declare. The model
  read in one run and then wrote with literal paths in a second, so `check` showed every write's verdict
  before it ran.

Open after Phase 15:

- **Partly literal inputs.** The checker judges a call's scope only when its whole input is literal, so
  `fs.move` with a literal `to` and a computed `from` is checked per request. Per-field judging is a
  kernel change; Phase 16 shows whether it matters.
- **Record key order in run results.** Rust's `--format json` result keeps a record literal's field
  order, Lean sorts, and docs/language.md says declaration order. `to-json` sorts and does not depend on
  this; the run result encoding should be settled to one rule.
- **A non-finite float in Lean's encoder** renders as `"NaN"` where Rust fails. No accepted program can
  produce one.

**Phase 16 result: the gate failed.** The session runner (`bench/workflows/session.py`, `runner.py
sessions` and `effects-report`) runs each trial as a pi session in print mode. A recording proxy between
pi and the server logs every tool call, each call is attributed to the attempt that made it, and each
workflow's `audit` judges the final state and every effect. One metric changed from the text above:
"effects performed by attempts before the first correct one" needs a snapshot after every attempt, so
the runner reports partial effects instead, the writes performed by attempts that did not complete. A
session the provider stopped before it was correct is rerun at most twice, with the stopped run kept
under `stopped/`. The stopped share counts an Sx rejection or policy refusal for a safety reason, from
`check` as well as `run`, and in either arm a run-time refusal of an attempt's first write. It cannot
tell whether a stopped program would have violated anything.

The six development workflows (`bench/workflows/effects/`) and the eight blind ones
(`bench/workflows/effects-blind/`, frozen at `8f35d1b`, 54 careless variants all caught by their
audits) each ran 3 samples per arm at medium thinking with Muse Spark and with `gpt-6-luna`, chosen as
the stronger model because it is priced like Haiku 5.5, a twentieth of the other frontier models
reachable through pi. Blind set, pooled:

| Model, arm | Correct | Violating effects | Partial effects | Stopped before any effect | Attempts that violated | Attempts per session | Output per session | Cost per session |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Muse, Sx | 24 of 24 | 1 | 0 | 3 | 1 | 4.8 | 6,285 | $0.0032 |
| Muse, JS | 24 of 24 | 0 | 0 | 0 | 0 | 4.0 | 2,942 | $0.0013 |
| Luna, Sx | 24 of 24 | 0 | 3 | 4 | 0 | 3.5 | 2,757 | $0.0031 |
| Luna, JS | 24 of 24 | 0 | 0 | 6 | 0 | 4.5 | 775 | $0.0010 |

The development set agreed: 71 of 72 sessions correct and no violating effect in either arm. The one
wrong session was cut off by the provider before the rerun rule existed.

What the transcripts say:

- **A careful agent loop already avoids these harms.** When the task states its policy, both models
  read the data in one attempt and act in a later one, in small steps. The careless whole-program
  answer that static checking would stop rarely appears, so there is little for Sx to stop. Its
  checker and approver did stop 7 programs before their first effect, but the JavaScript arm's
  run-time refusals stopped 6, and nothing shows the stopped programs were harmful.
- **Limits across a session are invisible to a per-program check.** The one violation was Muse in the
  Sx arm requesting a page a fourth time, against a limit of three per page, over several separate
  `run` calls. The approver judges each program alone, so it cannot enforce a limit that spans runs.
- **Sx costs 2.5 to 3 times JavaScript per session** ($0.0031 against $0.0010 to $0.0013), with
  prompt caching on: the guide is mostly cache reads, and the gap is the model's own output, 2 to 3.6
  times JavaScript's.
- **pi's codemode resolves a failed MCP call with `isError` instead of throwing.** Scripts that relied
  on `try` logged failures as successes; the task's instruction to look before retrying is what kept
  that from doing harm.

**Handoff.** Everything is on master and nothing is pushed. Raw sessions are under
`.work/effects/{dev,blind}-{muse,luna}/` (disposable; the tables above are the record). Phase 17 is
next in the plan, and adding `sx mcp code` to a project the owner trusts or to the user-level
configuration is an owner step. Options for the owner, given that the campaign's central claim did not
hold at this scale:

1. **Run Phase 17 as planned**, real everyday requests through both code modes. It measures whether Sx
   is usable as a real agent's code mode, which Phase 16 did not test, and costs little.
2. **Test the claim where it should hold**, as a new phase: data too large to read before acting, and
   host policies the prompt does not state, which only the approver and the grants enforce.
3. **Session-wide limits in the approver**: a policy that counts calls across the `run`s of one
   server's lifetime, which the one violation shows a per-program check cannot see.

The owner chose option 1 and let the campaign lead perform the owner step.

**Phase 17 result: passed.** Serving repomod took three changes. `sx mcp export` prints the
`tools/list` for any declared capability. `sx-repomod mcp` serves repomod's 11 read capabilities over
stdio with those schemas. `sx mcp import` now reads back the payload-enum and `Result` encodings that
export writes, because without them `git.status` and `github.cached-ci` could not be imported. The
harness is `bench/workflows/real.py`, with the 8 requests in `bench/workflows/real/requests.json`.
Each session starts in the repomod checkout with `--no-approve`. Its pi configuration lives in a
private agent directory, and its scratch directory sits apart from the records. A first trial run
from inside the records read the harness's files and the Sx sources.

Both arms have pi's read-only tools (`read`, `grep`, `find`, `ls`) and one code mode. Sx gets
`sx mcp code` over repomod plus an `fs` grant on the scratch directory, with policy approval. The
JavaScript arm gets pi's `codemode` over `sx-repomod mcp` and `sx mcp serve` for the same `fs` grant,
with pi's default `codemode` exposure, so it finds tools by search. Each model ran 8 requests ×
2 arms × 2 samples at medium thinking. The answers were judged against ground truth computed with
repomod's own capabilities, and every answer the key-fact checks failed was read.

| Model, arm | Correct | Turns | Programs | Rejected or refused | Other tool calls | Output | Cost |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Muse, Sx | 15 of 16 | 9.9 | 97 | 9 | 58 | 4,098 | $0.0041 |
| Muse, JS | 16 of 16 | 17.1 | 183 | — | 129 | 5,356 | $0.0044 |
| Luna, Sx | 12 of 16 | 6.4 | 74 | 15 | 12 | 1,833 | $0.0036 |
| Luna, JS | 16 of 16 | 8.1 | 108 | — | 8 | 1,471 | $0.0028 |

Programs, turns, output, and cost are per session except the program count, which is the total.
JavaScript has no rejection step: its scripts either completed (284) or failed (7). Sx's
rejections were type and shape errors (`type-mismatch` 6, `not-a-record` 3, `unresolved-type` 3,
`branch-mismatch` 3, `missing-field` 2, and others) and `out-of-scope` 8. 11 `run`s failed at run
time. No session touched repomod's working tree, and every write stayed in its scratch directory.

What the transcripts say Sx should change:

- **Wrapped results.** Most Sx programs unwrap an imported tool's result with `(get x :result)`,
  because export wraps an output that is not a record and import does not unwrap it. Import should
  read `_meta["sx/result"]` and return the wrapped value, as the JavaScript harness already does.
- **`fs.make-directory` on a root.** Both of Muse's `out-of-scope` rejections were
  `fs.make-directory` of the scratch root before writing into it. The root is never a write target,
  so the refusal is right for a write. For a directory that already exists, though, the call is a
  no-op, and refusing it only costs a repair.
- **Data between runs.** A `run` keeps nothing for the next, so the model carries data forward by
  retyping it as a literal: 17 runs re-entered lists of repository names it had read before. pi's
  `codemode` has `store()` and `load()` for this, though few scripts used them (5 of 291). A program
  that names an earlier run's result, or a guide that asks for one program per request, would remove
  the retyping.
- **Scope of "my repositories".** All 5 wrong answers were Sx's. The model limited "my
  repositories" to the 10 personal `joshuarli` ones out of 34, and Luna first guessed the GitHub owner
  `josh` from the home path and listed a stranger's repositories. Nothing in the arms explains why
  only Sx did this, and 16 sessions per cell is too few to tell it from chance.
- **Where Sx helped.** Sx needed fewer turns and programs (Muse 9.9 turns against 17.1). The
  JavaScript arm spent 126 of its 291 scripts finding tools, because codemode lists MCP tools only
  through search, while Sx's tool description carries the whole listing. That brought Sx's cost
  level with JavaScript's for Muse and within 30% for Luna. On the effects benchmark it was 2.5 to
  3 times.
- **Leaving code mode.** Muse's JavaScript sessions made 129 calls to the read-only tools, against
  58 in Sx, mostly `ls` and `read` while looking for checkouts and the catalog. Luna stayed in code
  mode in both arms.

**Handoff.** Everything is on master and nothing is pushed. Raw sessions are under `.work/real/`
(disposable; the tables above are the record). The campaign's planned phases are done.

**After Phase 17.** The owner asked for the lead's recommendations, and two changes are made:

- `sx mcp import` reads a wrapped result as the value it wraps. A tool marked `sx/result: wrapped`
  whose `outputSchema` is exactly `{result: VALUE}` declares the type of `VALUE`, with the result kind
  `wrapped`. This breaks programs written against declarations imported before; those declarations
  still load and run.
- `fs.make-directory` of a path that resolves to a granted root succeeds without doing anything.
  `fs.delete`, `fs.move`, and `fs.write-text` of a root are still refused.

Two are not made. Asking for one program per request would trade away what Phase 16 found keeps these
sessions safe: reading in one run and writing literal paths in the next, which `check` can judge. Letting
a program read an earlier run's result is a language change, and 17 retyped runs do not yet justify it.
Session-wide limits wait for more than one violation.

#### Phase 16: The effects benchmark

Workflows in `bench/workflows` format where a wrong effect costs something, each with a
`policy.json` for the approver and a check that audits every effect, not only the result:

- non-idempotent writes against a service that fails transiently, where a careless retry charges
  twice;
- destructive file operations on targets that come from data, some of which the task's policy forbids;
- multi-step changes where a failure part way must leave nothing half done;
- limits on how many writes or calls a run may make;
- data that asks for an action the host did not grant.

The runner keeps each attempt's effects before it resets the sandbox, because in real use a failed
attempt's effects stay. Per arm and workflow it reports correctness, effects that violate the policy,
trials with any violation, effects performed by attempts before the first correct one, the share of
violating programs stopped before their first effect (by Sx's checker and approver), the tool calls
and turns a session took, and cost. Both arms run as pi sessions in print mode with no built-in tools:
the Sx arm with only `mcp__sx_code__*`, and the JavaScript arm with only pi's `codemode` over the same
tools served by `sx mcp serve` with the same grants. A session may call its tool as often as it likes,
so repairs happen inside the session, and every run's effects count.

As before: six development workflows the campaign lead may read, then eight blind ones written under
Phase 12's rules by a subagent that sees neither the guide nor any result, frozen before any model
runs, and measured once. Muse Spark at medium thinking is the model, and a stronger model available
through `pi` joins if one is, chosen when the phase starts.

**Gate:** the blind result is recorded. Sx wins if, at first-attempt correctness within 10 points of
TypeScript, its trials perform fewer policy-violating effects and it stops a larger share of violating
programs before their first effect.

#### Phase 17: Real sessions

pi connects to `sx mcp code` over the owner's real tools: repomod's read-only capabilities and a
scratch directory tree with `fs` writes. A fixed list of everyday requests runs through it and, for
comparison, through pi's own `codemode` over the same tools, and the transcripts record each program
written, checked, refused, and run, the repairs, and where a model left code mode for another tool.
Adding the server to a project the owner trusts, or to the user-level configuration, is an owner step.
**Gate:** the transcripts and their counts are recorded, with what they say Sx should change.

#### The repomod track

Runs beside the phases. repomod gains a public API for what `sx-repomod` imports today through
private names (`common._catalog`, `common._select`, `common._print_json`,
`github._discover_owner`, `status._checkout_status`, `status._todo_item_count`,
`status._status_text`) and a `render_discover` function, with tests in repomod, and `sx-repomod`
moves to it. **Gate:** repomod's suite, ruff, and ty pass, and the parity tests pass.

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
cost. This plan was written when Phase 4 ended, and the owner accepted it with the decisions
below.

**Scope.** The reader, checker, semantic IR, machine, values and JSON, registry, budgets,
diagnostics, help, and the `sx` command line, plus the `text`, `semver`, `git`, and `github`
cores, which the fixtures and the benchmark use. Optimization, WASM, and host SDKs belong to
campaign 4.

**Layout.** A Cargo workspace in `rust/` with three crates: `sx-kernel` (a library whose modules
mirror `src/sx`), `sx-cores` (the four cores), and `sx-cli` (the binary). The owner pins the
toolchain in the root `rust-toolchain.toml`, a nightly with `rust-src` and `clippy`, and the
port never changes that pin. Dependencies stay minimal. JSON goes through mitsuhiko's
`deser` crates (`deser-json`, `deser-value`), not serde, by the owner's choice. HTTP and HTTPS,
when the `http` capability pack needs them, go through the owner's `h12tiny` (`~/d/h12tiny`),
whose blocking client fits a synchronous host. The case-folding table for `text.casefold` is
generated from Python's `unicodedata` and checked in, so it matches the prototype code point
for code point.

**Order of work.** Each step lands with its conformance check green.

1. Values, types, and type-directed JSON, checked by decoding and encoding every value in
   `fixtures/run`.
2. The reader, checked byte for byte against `fixtures/read`.
3. The machine, driven by IR that Python exports with `sx check --emit ir`, so machine
   conformance does not wait for the checker. Gate: every accepted `fixtures/run` case gives the
   same outcome and exact trace.
4. The checker. Gate: the IR it emits is identical to Python's for every accepted fixture, and
   every `fixtures/check` output is byte-identical, diagnostics included.
5. The cores, `sx help`, and the command line, checked against Python's output for every help
   entry and every CLI fixture.
6. Differential testing. The probe has saved several hundred real model programs. The Rust
   kernel must give the same check result and verdict on each as Python. A grammar-based
   generator then compares random programs across Python, Rust, and Lean's `sx-ref`.
7. The host boundary, then the repomod host on the Rust kernel, gated by the Phase 3 parity
   tests.
8. Measurements of parsing, checking, evaluation, suspension and resume, and host dispatch,
   against Python, under the perf-gate rules.

**Gate.** Rust agrees with Python on every fixture and every saved model program, with Lean on
every accepted fixture, and the repomod parity tests pass on the Rust kernel.

**Decisions, accepted by the owner.**

- **Host boundary.** A JSON protocol over standard input and output, where the host sends a
  program and responses and receives requests, serves Python, TypeScript, and Bun hosts with one
  implementation, because suspension is already plain data. A PyO3 module is faster for Python
  hosts only. The recommendation is the JSON protocol first, and PyO3 only if the measured
  dispatch cost calls for it. Measured after the port: a Python host costs 24.5 µs of CPU per call
  over the protocol, about what the in-process Python kernel cost, while real capability calls
  take milliseconds, so PyO3 is not needed.
- **Identical diagnostics.** Diagnostics and help are part of the language, so the
  recommendation is byte-identical text across implementations, enforced by the check fixtures.
- **The Python prototype after the port.** Keep it as an executable specification beside Lean
  until Rust passes every gate. Once Rust is ahead, Python is frozen and new features land in
  Rust and Lean.

### Phase 6: Scoped authorization and the `fs`, `http`, and `process` packs

The owner extended the campaign past the port toward the roadmap's six-workflow milestone. New
work lands in Rust and Lean; Python is frozen.

- **Grants.** An execution's allow-list entry becomes a capability ID plus an optional scope, a
  typed record the pack declares: file roots for `fs`, origins and methods for `http`, programs
  and argument patterns for `process`. The host grants. A program never names its own authority.
- **One scope check, two places.** Each pack supplies a pure function from a scope and a request
  input to allowed or denied with a reason. `explain` and the checker apply it to inputs that are
  literal at check time, so an out-of-scope literal request is rejected before anything runs. The
  host applies it to every request at run time and refuses an out-of-scope one with
  `PermissionDenied`, a refusal kind, so the operation is never performed.
- **Packs** in a new `sx-packs` crate. `fs`: read text and bytes, list, stat, and an atomic,
  idempotent whole-file write. Paths are resolved, symlinks included, before the scope check.
  `http`: requests through the owner's `h12tiny` blocking client, with status, headers, and body,
  and a closed mapping from transport and status failures to call error kinds. `process`: run one
  program with exact arguments, never a shell, with a cleared environment, a working directory
  inside an `fs` root, a timeout, and captured output and exit status.
- **Command line.** `sx run` and `sx explain` take grants from a JSON file. `explain` shows, per
  call site, the literal resource and the scope verdict, or that the check happens per request.

**Gate:** Each pack passes tests against temporary directories, a local `h12tiny` server, and
fixed commands. Scope-escape tests cover `..`, symlinks, absolute paths, and URL tricks
(userinfo, ports, case, encoded characters, redirects). A denied request never reaches the
operating system or the network.

### Phase 7: Script headers and the `decimal` core

- A program may start with `(script {:inputs {:owners "List<String>"}} body)`. The header
  declares typed inputs in the declaration type language, so Sx keeps one type grammar. It
  declares no authority. `sx run report.sx --input owners='["acme"]'` then needs no host.
- A `decimal` core: an exact decimal type that encodes as a JSON string, with arithmetic that
  states its scale and rounding, comparison, parsing, and display.

**Gate:** Fixtures cover header errors and input decoding, and the Lean reference accepts the
header's IR.

### Phase 8: MCP import and the MCP host

- `sx mcp import` reads an MCP server's tool list and writes declarations. Tools become
  namespaced capabilities. JSON Schema maps to Sx types: objects to records, open when the schema
  allows extra properties; arrays to lists; string enums to enums; nullable or optional to
  `Option`; integers to `Int`; numbers to `Float`. A tool whose schema has no faithful Sx type is
  skipped with its reason, never approximated. MCP's read-only, destructive, and idempotent hints
  set the effect; a tool without hints is a write that is not idempotent. Importing describes a
  tool. It never grants it.
- `sx mcp run` executes a program against an MCP server, answering each request with
  `tools/call`.

**Gate:** Import and execution pass against a local fixture MCP server covering every schema
mapping, each skipped construct, and every error path.

### Phase 9: The six-workflow milestone

Run the roadmap's six workflows end to end in Sx and in TypeScript on Bun, written by the probe
model from the same task text and the same typed tool declarations, several samples each.

- **One tool implementation for both sides.** Sx performs pack and endpoint calls natively.
  `sx mcp serve` exposes the same granted performers as an MCP server, and the Bun harness calls
  them through a minimal MCP client behind the same generated `api` object the probe uses. The
  effects are identical, and neither side has a private advantage. Authority differs: Sx's
  `explain` reports it before anything runs, and the comparison records what each side could
  show before running.
- **Sandboxed, local, real effects.** Every workflow runs against temporary directories, local
  Git repositories, and local mock services (a GitHub-like CI API, a paginated REST API, an
  object store, an MCP tool server), never the public internet.
- **Workflows and what they bring in:**
  1. Inspect 100 local Git checkouts and their CI in parallel: `process` with `git`, typed
     `http` endpoints.
  2. Fetch paginated REST data into SQLite: typed endpoints, a `sqlite` pack.
  3. Read YAML manifests, change values, and write them atomically: a `yaml` core that edits
     the supported block subset in place and reports anything outside it, never reformatting.
  4. Run a compiler, collect diagnostics, and attempt a bounded repair: `process`, `fs`, `loop`.
  5. Build an index and publish it to object storage: `http` with credentials injected by the
     grant, so the program never sees the secret.
  6. Call an MCP tool through an imported typed capability.
- **Measured per workflow and language:** successful outcomes, first-attempt and final
  correctness, tokens, wall time, and the host code each needed.

**Gate:** All six run in both languages, with the comparison recorded in plan.md's Status.

## repomod safety rules

These rules apply to every phase.

- Never run `fix`, `push`, `merge`, `gc`, `topics apply`, or `sync` from this project.
- Change `../repomod` only in its source, tests, and docs, in a worktree under its own `.work/`,
  and never push it. The owner opened it to changes after Phase 9.
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

- Writes to Git or GitHub, including guarded sync, push, fix, and merge, and file writes beyond
  `fs.write-text` within granted roots.
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
