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
