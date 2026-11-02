# Sx roadmap

This document describes where Sx is going after the current campaign, and which decisions the
current campaign makes early so that direction stays open. The current campaign itself is in
[plan.md](../plan.md). Nothing here is in scope for it.

## Position against Bun

Bun is a JavaScript runtime, a Node-compatible ecosystem, a package manager, a bundler, and a
large collection of systems APIs. Sx does not compete as a JavaScript runtime. It competes for
one job that Bun, Deno, Node, and V8 isolates do today: running code a model wrote, against tools
a host provides. TypeScript in one of those runtimes is the default for Code Mode now.

Sx can win that job on four axes, each measurable:

1. **Authority before execution.** The host sees every capability a program can call before it
   runs, and every request's input before that request runs. A JavaScript host learns what model
   code does only as it does it.
2. **Reliability and cost for model-written programs.** First-attempt correctness, repairs, and
   tokens per solved task. The current campaign's probe is the first measurement.
3. **Determinism and replay.** The machine reads no clock and no environment. The same responses
   produce the same trace, so a failed run replays exactly.
4. **Embedding cost.** Startup time and memory per execution, for hosts that run many short
   programs. A Rust interpreter with no JavaScript engine should beat a runtime or isolate start.

Bun's systems coverage (files, processes, HTTP, SQLite, object storage, SQL, Redis) sets the bar
for Sx's capability packs. Its JavaScript compatibility, npm support, bundler, and HTTP server
are not goals.

## Rules for every campaign

- **Cores transform values, and capabilities touch the world.** A `path` core normalizes paths.
  The `fs` capability pack reads files. A core that needs an observation receives it as an
  argument.
- **The kernel never learns a domain.** A new domain is a new core or capability pack, never a
  new form.
- **A new form needs evidence.** The bar is a task class that cores cannot serve, shown by a
  measured failure.
- **Native cores are trusted code.** Registering a Python or Rust function as pure is a promise
  the kernel cannot check. Purity becomes enforced only when cores run sandboxed. A Lean theorem
  is about the Lean model, not about any native core.
- **Shared data shapes, not pairwise bridges.** Bridges between every pair of cores grow as the
  square of the core count. Cores exchange a few shared representations, and a bridge exists only
  where generic records lose domain meaning.

## Where the architecture is weaker than it looks

Separating pure cores from effectful capabilities is the functional-core, imperative-shell
pattern, enforced at the language boundary. Static authority, explicit suspension, and a kernel
with no domain knowledge follow from it. That part should not change. These weaknesses are known
and bounded, and each has a campaign that addresses it:

1. **Native cores are pure by promise.** A Rust or Python function registered as pure is trusted,
   not checked. The guarantee is only as strong as each core's author until cores run sandboxed
   in campaign 4.
2. **Static authority is coarse for the capabilities that matter most.** The checker knows a
   program calls `fs.read`, not which file, when the path is computed at run time. For files,
   HTTP, and processes, the per-request check does most of the work, so "explain before running"
   promises less than it sounds. Campaign 2's scoped authorization carries this weight.
3. **Many small cores cost discovery and composition.** A model must find which core holds which
   function, and every core boundary adds conversions, especially between nominal types such as
   `RepoId`. That cost grows with the core count. Lazy discovery in campaign 3 and shared data
   shapes address it, and neither is designed yet.
4. **Logic that no core covers has nowhere good to live.** Local functions exist, bound in
   `let` and typed at each call site, but there is no recursion, so a recursive algorithm must be
   written as a bounded `loop`. No probe task has needed more yet.

## Risks to the project

- **Whether models write Sx reliably was measured once, narrowly.** On a fresh blind set, Muse
  Spark wrote Sx correctly at first attempt 94.6% of the time against 96.4% for TypeScript, at 1.7
  times the tokens per solved task. That is one model, 28 tasks, and two samples. A second model
  and the campaign 2 workflows are the next evidence.
- **Adoption is harder than the technology.** Agent hosts already run model code in JavaScript
  isolates that work well enough. The MCP importer is the most likely entry point: a host points
  Sx at the tools it already exposes and gains authority checks and replay without rewriting
  them. Measure the host code an integration needs. A few lines is a credible pitch, and a
  bespoke adapter per tool is not.
- **Growth erodes the advantage.** Every form added for coverage costs guide tokens and model
  reliability, which is the edge Sx has over JavaScript. The rule that a new form needs evidence
  protects the project's main claim, not a style preference.

## What the model probe taught

These findings come from the Phase 2 probe and its two held-out sets, recorded in plan.md's
Status. They should shape every later language decision.

**Strictness cost more than syntax.** Almost every rejected first attempt was a program a
TypeScript or Clojure programmer would call correct: testing an optional in an `if`, joining
records whose branches set different fields, writing `(str "n=" 3)`, passing a string where an
enum was expected. Bracket errors were rare for the stronger model. The revisions that worked
accepted such a habit wherever it has exactly one safe meaning, and kept rejecting the rest with a
diagnostic that names the fix. First attempts on the fresh set rose to within two points of
TypeScript without changing the S-expression surface.

**A development set written beside the language overstates transfer.** The stronger model
scored 93% on the development set and 71 to 77% on the first held-out set. Tasks written by the
language designer exercise what the designer already handled. Held-out tasks must come from an
author who never shows them to the designer, and a held-out set that has spent its revision
becomes development evidence, replaced by a fresh blind set.

**Accepting a habit shrinks the guide.** A rule a model already follows needs no teaching. After
the habit revisions, the guide lost 20% of its tokens by dropping such rules, and first-attempt
correctness held.

**A shorter guide is not a cheaper one.** With the one-page cheat sheet instead of the guide, the
model reasoned more, and total tokens per solved task rose. Output, the expensive side, is within
7% of TypeScript. The remaining premium is the guide, a fixed input prefix that a host can cache.

**Effect-aware rules can say more than a general-purpose language.** `retry` around a write that
is not idempotent is rejected, except when every retried kind is a refusal such as `RateLimited`
and the write is the last call, because a refused request did not take effect. TypeScript cannot
express that distinction, and one held-out task needed it.

**A second implementation finds bugs the first one's tests miss.** Writing the Lean reference
semantics from the specification exposed two Python bugs that every Python fixture passed: reused
request IDs across two `par` forms on one fiber, and a local predicate that saw call-site
bindings. The Rust port should be checked against both.

**Near-parity on tool tasks did not carry over to real workflows.** On the six-workflow
milestone, TypeScript on Bun was right at first attempt 18 of 18 times against 10 of 18 for Sx, at
2.5 times fewer tokens, with the same tools behind both. The probe's tasks needed only the kernel,
which the guide teaches; the workflows needed packs the guide does not cover, and TypeScript
brings years of familiarity with files, processes, and HTTP. Sx led where a core did the work: on
YAML edits it used 37% fewer tokens than TypeScript's hand-written line scanning. The authority
`explain` reports before a run is real and TypeScript has nothing like it, but no answer in the
comparison was harmful, so it earned nothing measurable. The next campaign has to make Sx's
advantages count where a model pays for them: in the guide, in cores, and in tasks where a wrong
effect costs something.

## Alternatives to rule out

Full JavaScript is the baseline, not the strongest alternative. The strongest is a restricted,
typed TypeScript subset, analyzed before it runs. It could deliver much of Sx's static authority
while keeping the familiarity models already have. Sx does not build one. After the first
held-out probe failed, the owner chose to keep designing a new language rather than restrict an
existing one. The risk stays: if Sx cannot beat TypeScript in the campaign 2 workflow comparison,
the custom syntax has not earned its cost.

## Campaign 1: language correctness

This is the current campaign: Python interpreter, model probe, Git and GitHub cores, repomod
parity, Lean semantics, and the Rust port. Its scope stays frozen until its gates pass.

These decisions are made in campaign 1 so later campaigns extend programs instead of breaking
them:

| Decision | What it keeps possible |
| --- | --- |
| JSON decoding is directed by the expected type | `Map`, `Bytes`, and `Decimal` add encoding rows without changing existing ones |
| Request IDs include every enclosing repetition index | New repetition forms cannot make IDs collide |
| Authorization is an allow-list plus per-request refusal | Scoped authority for files, hosts, and commands needs no kernel change |
| `CallErrorKind` is a closed kernel enum | New kinds are additions, and programs never parse messages |
| Inputs are typed names the host declares | Script headers can declare the same inputs later |
| Delays are host requests | Replay and virtual time work for every capability pack |
| `loop` requires a literal `:max` | Pagination works without general recursion |

## Campaign 2: practical substrate

The goal is real automation through capability packs, with no change to the machine's shape.

**Data model.** Real APIs return objects with arbitrary keys, binary payloads, exact decimals,
and many fields a program does not need.

- `Map<String, T>` for objects with arbitrary keys.
- `Bytes`, encoded as base64 only in JSON. Native hosts pass bytes without conversion.
- `Decimal` for exact arithmetic. Floating point only if a task class needs it. No implicit
  conversion between number types.
- Declared projection. A capability's record type may be declared open, so the host drops fields
  the type does not name. A closed record still rejects extra fields.

**Script headers.** A standalone `.sx` file declares its own typed inputs, so
`sx run report.sx --input owners='["acme"]'` works without a host program. This adds no
top-level definitions, imports, or module system.

**Scoped authorization.** The host declares scopes per execution: file roots for `fs`, hosts and
methods for `http`, executables and argument patterns for `process`. `process` carries the most
risk. A capability that runs any command is ambient authority, and Sx never offers one.

**Pure cores.** First `data`, `path`, `url`, `text`, `glob`, and `time`. Then `codec`, `hash`,
`regex` with a linear-time engine, `format` (YAML, TOML, CSV, JSONL) built on existing parsers,
and `decimal`. Each core keeps a small contract. `glob` matches paths, and directory traversal
belongs to `fs`.

**Capability packs**, in order: `fs`, `process`, `http`, `git`, `sqlite`, then `github`.

**Milestone.** Six workflows run end to end in Sx and in TypeScript on Bun with the same tools:

| Workflow | What it exercises |
| --- | --- |
| Inspect 100 local Git checkouts and query their GitHub CI in parallel | Concurrency, aggregation, two packs |
| Fetch paginated REST data and store it in SQLite | `loop`, typed decoding, SQL writes |
| Read YAML manifests, change values, and write them atomically | Formats, file writes, scoped authority |
| Run a compiler, collect diagnostics, and attempt a bounded repair | Processes, errors, repetition |
| Build an index and publish it to object storage | Pure transformation, object writes, credentials |
| Call an MCP tool through a generated typed capability | Extension without kernel changes |

Each comparison records successful outcomes, model attempts, tokens, wall time, and the code a
host had to write.

The first comparison is recorded in plan.md under Phase 9c.

## Campaign 3: capability ecosystem

**MCP import comes first.** Code Mode hosts already expose their tools over MCP. The importer
translates tool schemas into Sx types, assigns namespaced capability IDs, and produces entries
the host adds to an allow-list. Importing a tool describes it. It never authorizes it.

**OpenAPI import** produces a frozen, typed capability pack from a specification. The host still
owns authentication, transport, authorization, and budgets.

**Schema translation fails loudly.** A schema feature with no Sx type, such as `oneOf` without a
discriminator, is reported, and the tool is left out. It is never typed loosely.

**Lazy discovery.** An agent gets a small searchable catalog, `sx search TERM`, and loads exact
signatures only for the tools it chooses. Measure guide tokens at 10, 100, and 1,000 tools.

**Packaging.** Versioned core metadata, local packages first, with content identity for
reproducible resolution. Then packs for object storage (S3 and R2), Cloudflare, SQL, and Redis.

**Open question.** Imported services drift from their schemas. A response that no longer matches
is `invalid-host-response`, which ends the run. Imported capabilities may need to report a
mismatch as a `CallError` of kind `Invalid` instead, so a program can recover.

## Campaign 4: performance and adoption

- Optimize the Rust kernel and add a compact program representation.
- Ship host SDKs for Rust, Python, and TypeScript. With the TypeScript SDK, a Bun or Node host
  embeds Sx instead of running model code in its own runtime. A WASM build of the interpreter
  serves JavaScript hosts and isolates.
- Run third-party cores sandboxed, as WASM components, so purity and resource limits are
  enforced instead of trusted.
- Benchmark against Bun-based Code Mode: startup, memory per execution, per-call overhead, and
  end-to-end time on the campaign 2 workflows.

**Milestone.** A measured advantage on at least one of the four axes, with no loss in
reliability.

## Not building

| Feature | Decision |
| --- | --- |
| JavaScript runtime, npm compatibility, bundler, transpiler, DOM | Never |
| HTTP server, WebSockets, long-lived handlers | A host may serve Sx invocations. Sx gets no event loop. |
| General recursion, `while`, macros | Never. Repetition stays bounded by literals. |
| Native FFI | Only through the trusted-core mechanism, once it exists |
