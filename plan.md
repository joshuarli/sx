# Sx — Project Charter & Implementation Campaign

Objective: Build a small, formally specified, typed Lisp-shaped orchestration language, with an imperative execution kernel, independently specified functional cores, explicit typed bridges, and an extensible capability system.

The first production-relevant experiment will be [repomod](https://github.com/joshuarli/repomod?utm_source=chatgpt.com). The long-term goal is a general-purpose Code Mode execution substrate that can support arbitrary applications and third-party cores without expanding the language itself.

I reviewed repomod's current architecture, documentation, and tests. Three decisions should shape the campaign from the outset.

1. Keep Sx independent of repomod. Incubate it inside the repository, but enforce a clean dependency direction so it can later be extracted without rewriting its internals.
2. Make Lean authoritative for language semantics, not for real-world Git behavior. Small functional cores get their own specifications and proofs. Rust handles execution, and host adapters enforce real-world preconditions.
3. Treat extensibility as a v0 requirement. A third-party developer should eventually be able to add a new core, expose its types and functions, supply an optional Lean specification, and compose it with existing cores without changing the kernel.

Repomod is also much larger than its command-line interface suggests. Its current implementation contains substantial Git and GitHub logic, a large collection of policy rules, and carefully designed mutation safeguards. This strongly favors porting orchestration first, then selectively migrating domain logic, rather than undertaking a simultaneous Python-to-Rust rewrite.

Below is the self-contained agent charter and initial technical specification.

# Sx: Typed Orchestration IR

## Project charter, initial language specification, and unattended implementation campaign

**Status:** Founding specification, v0.1\
**Initial incubator:** [https://github.com/joshuarli/repomod](https://github.com/joshuarli/repomod)\
**Reference implementation:** Rust\
**Formal specification:** Lean 4\
**Initial application:** repomod\
**Long-term application:** General-purpose Code Mode and typed agent orchestration\
**Working name:** Sx (provisional; do not make architectural decisions depend on the name)

---

# Part I — Mission and philosophy

## 1. Mission

Build a radically small, typed, formally specified orchestration language that can coordinate sophisticated software through reusable functional cores and explicit effectful capabilities.

The architecture is:

**Imperative kernel + small functional cores + typed bridges + capability-controlled host effects.**

The kernel handles execution and composition. Functional cores implement deterministic domain logic. Bridges compose distinct domains. Host adapters interact with the outside world.

The primary hypothesis is that large language models do not need a general-purpose programming language to coordinate tools effectively.

They need a compact, easily generated, statically checkable representation of computations and tool invocations.

We will test whether such a language can deliver superior reliability, safety, execution efficiency, or token economics compared with JavaScript-based Code Mode.

This is an empirical hypothesis, not a predetermined outcome.

### 1.1 Foundational principles

1. **Small kernel, expansive capability surface.** Support new domains primarily by adding cores, not language constructs.
2. **Imperative glue, functional substance.** Orchestration expresses sequencing, branching, concurrency, and effects. Domain algorithms reside in pure cores.
3. **Formal semantics from the beginning.** Lean defines the intended behavior. Rust implements it.
4. **Composition over monoliths.** Core specifications remain independently understandable and testable.
5. **Explicit effects.** Every external operation is registered, typed, and authorized.
6. **No ambient authority.** Programs cannot access a filesystem, network, environment, or subprocess implicitly.
7. **Extensibility without kernel modifications.** Third parties can implement cores and bridges against a published interface.
8. **Separate specification from implementation evidence.** A proof about Lean definitions is not automatically a proof about Rust code or external systems.
9. **Native-first, portable by design.** WASM is an optional implementation boundary, not a defining feature.
10. **Measure before optimizing.** Performance, token efficiency, and model reliability must be established by controlled experiments.

### 1.2 Non-goals

Sx must not become:

- A replacement for XSH, Rust, Python, or JavaScript.
- A general-purpose shell.
- A Lisp with unrestricted macros and metaprogramming.
- A package manager or build system.
- A full Git or GitHub implementation.
- A generic database or durable workflow engine.
- An agent framework that embeds prompting, model selection, or conversation management.
- A system that automatically treats third-party code as formally verified.
- A system that claims to make external operations transactional merely because they are typed.

Sx should remain a focused execution substrate usable *by* agent frameworks and ordinary programs.

## 2. Success criteria

A successful prototype demonstrates that:

1. The kernel is independently specified in Lean.
2. The Rust interpreter conforms to the executable Lean semantics across a substantial generated and adversarial corpus.
3. Functional cores can be developed and tested independently.
4. Bridges connect cores without violating their type or effect boundaries.
5. A new core can be registered without editing kernel source code.
6. A nontrivial repomod command can run through Sx while preserving observable behavior.
7. All existing repomod functionality remains available and its original tests continue passing.
8. The system can explain which capabilities a program may invoke before execution.
9. The architecture permits future sandboxed third-party extensions without granting ambient host authority.
10. A repeatable benchmark can compare Sx against conventional JavaScript Code Mode.

The initial prototype does not need to demonstrate that Sx is faster or more token-efficient than JavaScript.

It must establish a rigorous foundation for determining whether that claim is true.

---

# Part II — Architectural specification

## 3. The four architectural layers

### 3.1 Layer A: Imperative orchestration kernel

The kernel owns:

- Parsing and AST construction.
- Static name resolution and type checking.
- Immutable lexical bindings.
- Control flow.
- Evaluation order.
- Calls to registered pure functions.
- Capability invocation requests.
- Bounded concurrency.
- Failure propagation.
- Explicit suspension and resumption.
- Resource accounting.
- Observable execution traces.

The kernel knows nothing about Git, GitHub, repomod, HTTP, or filesystem paths beyond generic typed values and capability identifiers.

This is a hard boundary.

### 3.2 Layer B: Pure functional cores

A core is a versioned package of deterministic functions, associated data types, and contracts.

Examples:

- `git`: classify repository states, interpret commit relationships, construct guarded Git plans.
- `github`: normalize CI results, classify pull requests, interpret remote metadata.
- `collections`: deterministic collection transformations.
- `policy`: predicates, policy findings, and rule evaluation.
- `repomod`: application-specific decisions built from the other cores.

Core functions are referentially transparent with respect to their declared input values.

A pure core may not:

- Read the current time.
- Access the filesystem.
- Fetch network resources.
- Use hidden mutable global state.
- Invoke host capabilities.
- Depend on unmodeled randomness.
- Mutate another core's state.

Time, randomness, and external observations must be supplied explicitly as values when required.

Core implementations may internally allocate memory or use caches that are semantically invisible, but their observable results must remain deterministic.

### 3.3 Layer C: Typed bridges

A bridge composes domain types and functions from two or more cores.

Examples:

- Git checkout identity ↔ GitHub repository identity.
- Git commit state ↔ GitHub default-branch metadata.
- GitHub CI state ↔ repomod policy decision.
- Filesystem metadata ↔ repository policy findings.

A bridge:

- Declares exact source-core dependencies.
- Declares its input and output types.
- Documents assumptions about those types.
- Implements deterministic transformations.
- May have its own Lean specification and theorems.
- Cannot acquire effects by virtue of being a bridge.

Bridges are essentially a specialized kind of pure core.

Do not create separate runtime machinery unless implementation evidence justifies it.

Where possible, use shared canonical types such as `RepoId` and `CommitId` to avoid an explosion of pairwise conversion packages.

Only create a bridge when there is genuine semantic translation or cross-domain reasoning.

### 3.4 Layer D: Host capabilities

Host capabilities perform external operations.

Examples:

- `git.snapshot`
- `git.fetch`
- `git.apply-plan`
- `github.repository`
- `github.ci`
- `github.merge-pr`
- `fs.read-file`

Each capability has:

- A stable identifier.
- A typed input contract.
- A typed output contract.
- A declared effect set.
- A host-controlled authorization policy.
- Optional retry, timeout, and concurrency policies.
- A concrete implementation.
- Mock and replay implementations.

A declaration is not sufficient to grant authorization.

The host independently decides which capabilities a particular execution can use.

Pure cores never invoke capabilities directly.

---

# Part III — Initial Sx language specification

## 4. Surface syntax

Sx uses a small S-expression grammar with Clojure-like collection literals.

Examples:

```
(let [x 10
      y 20]
  (+ x y))
```

```
{:owner "laputa-systems"
 :repo "xsh"
 :archived false}
```

```
(let [repos (call catalog.list {:tags ["tool"]})]
  (take 20
    (filter repos
      (fn [repo]
        (not (get repo :archived))))))
```

Syntax should be intentionally unsurprising to models familiar with Lisp, Scheme, or Clojure.

However, the project must not inherit an entire existing Lisp runtime or dynamic semantics.

### 4.1 Required syntactic forms

The v0 language supports:

| Construct                 | Syntax                    | Purpose                                      |
| ------------------------- | ------------------------- | -------------------------------------------- |
| Immutable bindings        | `let`                     | Name intermediate values                     |
| Conditional               | `if`                      | Branch on a boolean                          |
| Tagged-union matching     | `match`                   | Handle alternatives                          |
| Pure function application | `(core.function ...)`     | Invoke registered pure operations            |
| Capability invocation     | `(call capability {...})` | Request an external effect                   |
| Parallel composition      | `par`                     | Evaluate independent operations concurrently |
| Parallel mapping          | `par-map`                 | Bounded fan-out over a list                  |
| Error capture             | `attempt`                 | Capture recoverable execution failures       |
| Anonymous function        | `fn`                      | Local collection transformations             |
| Field selection           | `get`                     | Access a statically known record field       |

Literal forms:

- `true`, `false`
- Signed integer literals
- UTF-8 strings
- `nil` only where explicitly typed as an absent optional value
- List literals: `[a b c]`
- Record literals: `{:key value}`
- Qualified symbols
- Tagged-union constructors

Comments use `;`.

There are no arbitrary top-level definitions or module imports in v0 program source.

A program is one expression.

### 4.2 Lexical scope

Variables use immutable lexical bindings.

No reassignment, mutable cells, or hidden environment lookup.

Anonymous functions capture immutable lexical values.

`fn` is permitted only in explicitly supported higher-order operations such as `map`, `filter`, and `par-map`.

No recursive user-defined functions in v0.

No dynamic evaluation, quoting, macros, or arbitrary AST manipulation.

### 4.3 Core expression semantics

Evaluate arguments left to right unless the expression explicitly declares parallel evaluation.

All evaluation is strict.

Pure functions must receive fully evaluated, validated arguments.

A malformed expression cannot trigger an effect during validation.

No implicit conversions between numeric, string, optional, or nominal domain types.

Boolean conditions must have type `Bool`.

Integer arithmetic is checked; overflow is an error, never wrapping silently.

Define exact behavior for division by zero, ordering, Unicode strings, empty collections, and missing option values.

Avoid floats in the first semantic kernel unless an actual application requirement demonstrates their necessity.

### 4.4 Example: repomod status

Illustrative future program:

```
(let [repos
      (call catalog.select
        {:tags ["tool"]})]

  (par-map {:limit 8} repos
    (fn [repo]
      (let [[local remote]
            (par
              (call git.snapshot
                {:repo (get repo :id)})
              (call github.status
                {:repo (get repo :id)}))]

        (repomod.classify-status
          local
          remote)))))
```

This illustrates the intended abstraction boundary.

The kernel manages concurrency and effects.

The `repomod` core classifies the observations.

The host owns Git and GitHub access.

This is conceptual source code: the implementation must define and validate the exact signatures before accepting it.

## 5. Type system

The v0 type system should be intentionally conservative.

### 5.1 Primitive and composite types

Required:

```
Bool
Int
String
Unit

List<T>
Tuple<T1, ..., Tn>
Option<T>
Result<T, E>

Record { field: Type, ... }
Enum { Variant(Type...), ... }
```

Support nominal domain types:

```
RepoId
CommitId
BranchName
PullRequestId
```

These prevent accidental interchange of semantically distinct strings.

Use structural record typing for ordinary records, with exact declared fields initially.

Use nominal identity where domain meaning is significant.

No user-authored generic functions, classes, inheritance, unrestricted unions, subtyping framework, or dynamic `Any`.

The implementation may use parametric types internally to describe standard collection operations.

### 5.2 Type inference

Infer types from:

- Literal values.
- Lexical bindings.
- Core function signatures.
- Capability signatures.
- Record declarations.
- Tagged-union constructors.
- Expected types propagated into lambdas.

For example:

```
(let [repos (call catalog.list {})]
  (map repos (fn [repo] (get repo :name))))
```

If `catalog.list` returns `List<Repository>`, then the lambda parameter is `Repository` and the result is `List<String>`.

The model does not need to supply these annotations.

Reject unresolved or ambiguous types before execution.

### 5.3 Static validation

Validation must reject:

- Undefined variables.
- Unknown functions or capabilities.
- Incorrect argument counts.
- Incorrect field names.
- Incompatible types.
- Invalid tagged-union matches.
- Unauthorized effects.
- Incorrect parallel-effect usage.
- Malformed literals.
- Invalid resource declarations.

Validation must be atomic with respect to effects.

**No capability invocation may occur until the complete program has passed static validation and authorization.**

## 6. Effect system

Use a minimal effect-set model.

Example signatures:

```
git.snapshot:
  RepoId -> GitSnapshot
  ! {git.read}

github.status:
  RepoId -> GitHubStatus
  ! {github.read}

git.apply-plan:
  GitPlan -> ApplyResult
  ! {git.write}

github.merge-pr:
  MergeRequest -> MergeResult
  ! {github.write}
```

Effects are inferred compositionally.

A program requiring `{git.read, github.read}` must not gain `{git.write}` through conditional branching or a bridge.

Authorization operates on exact capability identities and configured scopes, not solely on broad labels such as `git.write`.

A broad read/write classification is useful for policy and display, but must not become the sole authorization mechanism.

### 6.1 Parallel effects

For v0:

- `par` accepts independently evaluable expressions.
- `par-map` supports bounded concurrency.
- Parallel branches may invoke approved read-only capabilities.
- Parallel writes are rejected initially.
- Shared mutable state is unavailable to programs.
- Results are returned in source order, not completion order.

Read-only effects are not guaranteed to observe a single atomic snapshot of the external world.

Therefore, determinism claims apply to fixed injected responses, not necessarily to independently changing external systems.

Future parallel-write support requires explicit resource-conflict policies, ordering rules, and additional semantic analysis.

Do not introduce it merely to simplify repomod implementation.

### 6.2 Failure semantics

A capability call succeeds with a validated typed result or produces a recoverable `CallError`.

By default, a failed call aborts the current expression.

`attempt` converts a recoverable call failure into `Result<T, CallError>`.

For v0, restrict `attempt` to a single call expression unless broader structured recovery is independently specified and justified.

Static validation failures, authorization failures, invalid host responses, exhausted resource budgets, and internal interpreter failures are not silently recoverable through `attempt`.

Define their separate terminal outcomes.

Do not hide partial failures in parallel operations.

### 6.3 Resource limits

The interpreter must enforce explicit budgets covering:

- AST size and nesting.
- Evaluation steps/fuel.
- Total capability invocations.
- Concurrent requests.
- Intermediate and returned data size.
- Lambda applications.
- Continuation state size.
- Wall-clock limits at the host execution boundary.

Choose conservative configurable prototype defaults and test their enforcement.

Never let a user program raise its own limits.

Resource exhaustion terminates evaluation predictably.

Limits do not imply rollback of external effects already executed.

## 7. Execution model

Implement a bounded abstract machine.

Conceptual states:

```
Running(machine_state)

Suspended(
  requests,
  continuation
)

Completed(value)

Failed(error)
```

The interpreter evaluates until it:

1. Produces a final value.
2. Requires host capability results.
3. Reaches a terminal error.

It emits structured requests with stable invocation identifiers.

The host executes authorized requests and resumes the machine with corresponding validated responses.

The continuation must contain all evaluation state necessary for correct resumption.

Do not depend on preserving a native Rust or WASM call stack across tool calls.

### 7.1 Parallel request ordering

Request batches carry deterministic source-derived logical positions.

The host may complete operations in any order, but the evaluator reconstructs their results according to the originating expression.

A trace must distinguish:

- Logical invocation order.
- Host completion order, when relevant.
- Result delivery to the evaluator.

Differential conformance compares semantic outcomes and normalized logical traces, not incidental worker scheduling.

### 7.2 Canonical representation

The parser constructs a typed AST.

Define a stable, deterministic representation of validated programs for:

- Hashing.
- Caching.
- Logging.
- Differential tests.
- Reproducibility.

Reject unknown fields and unsupported versions.

Use a human-readable fixture representation for development.

Do not prematurely freeze a cross-language binary ABI. Establish the abstract value model first; add a versioned wire encoding before supporting independently compiled third-party components.

WASM is not required for the initial interpreter.

---

# Part IV — Functional cores and public extensibility

## 8. Core contracts

Every core provides:

1. A stable namespaced identity.
2. A semantic version.
3. A kernel ABI compatibility declaration.
4. Exported nominal and structural types.
5. Exported pure function signatures.
6. Dependencies on other cores, if any.
7. An implementation.
8. A test corpus.
9. A contract document.
10. An optional Lean formalization and proof inventory.

A core may declare dependencies but must not create dependency cycles.

All dependencies are resolved before program validation.

Core functions should be individually useful, not giant application-level operations.

Prefer:

```
git.classify
git.can-fast-forward
github.aggregate-ci
repomod.plan-sync
```

Avoid:

```
repomod.run-everything
github.do-all-work
```

## 9. Initial public core interface

Use a small Rust-facing interface, approximately:

```
pub trait PureCore {
    fn descriptor(&self) -> &CoreDescriptor;

    fn invoke(
        &self,
        function: FunctionId,
        arguments: &[TypedValue],
        budget: &mut ComputeBudget,
    ) -> Result<TypedValue, CoreError>;
}
```

This is an illustrative interface, not a mandated final Rust API.

Requirements:

- Signatures are available without invoking functions.
- All results are checked against declared output types.
- Core invocations are synchronous and pure from the kernel's perspective.
- No kernel-specific Git or GitHub logic.
- No ambient host handles passed into pure functions.
- Resource accounting applies to core work as well as interpreter steps.

Resource accounting for arbitrary native code is not enforceable solely by a supplied `ComputeBudget`; native cores are therefore trusted code in v0.

A stronger isolation boundary is necessary for untrusted cores.

## 10. Public extension model

Third-party extensibility is a first-class design goal.

A developer should eventually be able to publish a core supplying functions such as:

```
semver.compare
semver.satisfies

ci.aggregate-results

cloudflare.classify-deployment

finance.compute-allocation

kubernetes.plan-rollout
```

These are illustrative domains, not standard-library commitments.

### 10.1 Package manifest

Use a versioned, declarative manifest, provisionally TOML:

```
[core]
id = "example.semver"
version = "0.1.0"
abi = "sx-core-v0"
kind = "pure"

[[types]]
name = "Version"
definition = "Record { major: Int, minor: Int, patch: Int }"

[[functions]]
name = "compare"
input = ["Version", "Version"]
output = "Ordering"
```

The actual schema must be designed and validated; treat this as an example.

The manifest describes interfaces, not implementation authority.

Package identity must include immutable content identity for reproducible resolution.

Published core versions must not silently change their signatures or semantics.

### 10.2 Static and dynamic loading

Support two tiers.

**Tier 1: Trusted native cores**

- Statically linked Rust implementations initially.
- Smallest possible runtime overhead.
- Intended for built-in and explicitly trusted host code.
- Registration through the same public registry API as external cores.

**Tier 2: Sandboxed portable cores**

- Independently compiled modules, potentially WASM components.
- No ambient filesystem, networking, clock, or randomness.
- Bounded CPU and memory.
- Explicit interface imports.
- Versioned value encoding and ABI.
- Runtime enforcement independent of package declarations.

Portable modules may be introduced after the native registry stabilizes.

Do not implement native dynamic loading of untrusted third-party libraries.

### 10.3 Formal assurance levels

Do not use a misleading universal `verified` flag.

Define evidence categories:

- **Unverified:** Interface validation only.
- **Conformance-tested:** Has passed named behavior and differential tests.
- **Specified:** Has a published formal semantic model.
- **Proved-properties:** Has kernel-checked theorems for explicitly named properties.
- **Implementation-linked:** Has separately documented evidence connecting implementation behavior to its specification.

These categories are not mutually exclusive and do not imply one another automatically.

A Lean theorem about `SemVer.compare` does not establish correctness of an arbitrary external implementation of that function.

The loader should expose evidence metadata without treating a package author's self-reported proof claims as trusted authorization.

### 10.4 Extending types

Third-party cores must be able to define nominal types and tagged unions without modifying the kernel.

For example:

```
example.semver.Version
example.semver.Constraint
example.semver.Ordering
```

Namespacing and version resolution must prevent accidental type collisions.

No arbitrary implicit coercions.

Cross-core conversions are explicit pure functions.

### 10.5 Third-party host capabilities

Host capability plugins are separate from pure cores.

A developer may eventually register external operations, but these require host-specific authorization and review.

Such plugins must never be loaded or executed merely because a pure core depends on them.

The kernel's effect and permission model remains authoritative.

### 10.6 Distribution neutrality

Do not tie Sx to GitHub, crates.io, MCP, a specific agent harness, or a proprietary service.

A package registry can be added later.

Package metadata, schemas, and conformance artifacts should be distributable independently of any central registry.

Support local packages first.

---

# Part V — Formal verification strategy

## 11. Lean as the semantic oracle

Lean 4 is the authoritative source of language meaning.

Implement independently:

```
Syntax
Types
Typing judgments
Values
Operational semantics
Machine state
Effects
Resource budgets
Continuations
Observable traces
```

The reference evaluator must consume the same logical program inputs and simulated capability responses as Rust.

It should be executable on bounded examples.

Avoid defining Lean semantics in terms of Rust-generated outputs.

Avoid deriving the entire Lean evaluator automatically from the Rust interpreter.

Independence is necessary for useful differential testing.

## 12. Foundational theorems

Prioritize:

### Kernel safety

- Well-typed pure expressions preserve types during evaluation.
- Well-typed pure expressions do not get stuck except at explicitly modeled terminal errors.
- Unbound variables cannot occur in validated executable programs.
- Unknown or unauthorized capabilities cannot be requested by admitted programs.
- Resource accounting is monotonic and never exceeds the configured limits.
- Continuation resumption respects the specified state transition rules.
- Fixed capability-response traces produce deterministic logical outcomes.

### Effects and composition

- Pure function evaluation cannot emit host capability requests.
- Effects inferred for an expression cover every capability invocation permitted by its execution.
- A bridge cannot introduce additional effects.
- Parallel result assembly respects source-position ordering.
- Parallel read-only evaluation obeys the specified trace semantics.

Do not claim that arbitrary external read operations commute or observe a common snapshot.

### Core contracts

For each core, prove small, useful properties rather than attempting universal algorithm verification.

Example Git theorem:

```
If the Git planning function returns FastForward,
then all modeled fast-forward eligibility
conditions held in the supplied snapshot.
```

Example GitHub theorem:

```
CI aggregation never classifies a result
as passing when a modeled required check
has a failing state.
```

The exact GitHub theorem must reflect real CI semantics, including missing, stale, pending, and unknown observations.

## 13. Proof discipline

Prohibited in accepted core proofs:

- `sorry`
- Unreviewed axioms.
- Unsafe definitions that bypass intended guarantees.
- Hidden trusted assumptions.
- Misleading theorem names that overstate what is proved.

Record:

- The source revision modeled.
- The theorem statement.
- The assumptions.
- Relevant definitions.
- Whether the theorem is kernel-checked.
- The Rust conformance evidence, if any.
- The boundary of the claim.

Follow the useful separation already established in XSH's formal project: specification proofs, artifact evidence, and implementation conformance are distinct.

Do not assume the current XSH Lean project has completed universal soundness proofs.

## 14. Differential conformance

Build a common fixture harness.

Each test contains:

- Program source or validated AST.
- Registered type and capability declarations.
- Available authorization scopes.
- Resource budgets.
- Mocked capability responses.
- Expected terminal result.
- Expected normalized logical trace.
- Expected failure classification when applicable.

Run each case against:

1. The Lean reference evaluator.
2. The Rust implementation.

Compare structural values and normalized traces.

Include:

- Handwritten semantic examples.
- Generated well-typed programs.
- Generated ill-typed programs.
- Parser adversarial inputs.
- Nested parallel expressions.
- Delayed and out-of-order responses.
- Missing responses.
- Duplicate response identifiers.
- Invalid host results.
- Budget exhaustion.
- Capability denial.
- Recoverable and terminal failures.
- Large records and collections.

For external calls, inject deterministic mock responses rather than making live network requests.

Use property-based generation and fuzzing where practical.

A production interpreter is not considered conformant simply because its normal-path tests pass.

---

# Part VI — Initial standard cores and bridges

## 15. Git core

Use abstract models rather than modeling the complete Git implementation.

Initial types:

```
RepoId
CommitId
BranchName

GitSnapshot {
  repo: RepoId
  present: Bool
  valid: Bool
  origin_valid: Bool
  detached: Bool
  dirty: Bool
  branch: Option<BranchName>
  head: Option<CommitId>
  default_branch: Option<BranchName>
  ahead: Option<Int>
  behind: Option<Int>
}
```

This is an initial abstraction. Add only information required by actual decisions.

Potential pure functions:

```
git.classify
git.is-clean
git.is-on-default-branch
git.plan-fast-forward
git.plan-push
git.classify-divergence
```

Do not let the pure Git core execute Git subprocesses.

Ancestry relationships that cannot be derived from a snapshot must be supplied as explicitly observed data or queried through a host capability before planning.

Avoid silently interpreting a missing observation as false.

## 16. GitHub core

Initial types:

```
GitHubRepository
RemoteBranch
CiStatus
WorkflowStatus
PullRequest
RemoteMetadata
```

Potential pure functions:

```
github.aggregate-ci
github.classify-checks
github.classify-pr
github.merge-eligibility
github.normalize-repository
```

Handle unknown and missing data explicitly.

Do not treat an API error as evidence that a repository has no failures.

Do not hide network retries or pagination inside pure functions.

Those belong in a host adapter or a separately modeled orchestration operation.

## 17. Git/GitHub bridge

Initial functions:

```
git-github.same-repository
git-github.compare-default-branch
git-github.sync-observation
```

The bridge should prove or test:

- Canonical repository identity is preserved by conversion.
- Missing metadata never becomes fabricated metadata.
- Commit identifiers retain their nominal meaning.
- Branch comparisons do not silently substitute local tracking state for live remote state.

Repomod currently distinguishes last-fetched Git state from live GitHub state.

The prototype must retain this distinction.

## 18. Repomod core

Application-specific pure functions:

```
repomod.classify-status
repomod.should-surface
repomod.filter-repositories
repomod.plan-sync
repomod.classify-finding
repomod.plan-push
```

Do not immediately port all existing policy rules.

Initially delegate complicated rule evaluation to existing Python functionality through a carefully controlled adapter, or port individual pure rules to Rust with parity tests.

The purpose is to test the IR's domain composition, not to rewrite hundreds of kilobytes of mature policy code.

---

# Part VII — Repomod incubation strategy

## 19. Repository integration

Work in:

[https://github.com/joshuarli/repomod](https://github.com/joshuarli/repomod)

Read first:

```
AGENTS.md
README.md
docs/design.md
docs/policy-development.md
docs/tagging.md
src/repomod/cli.py
src/repomod/types.py
src/repomod/git.py
src/repomod/github.py
src/repomod/sync.py
src/repomod/policy.py
```

Review the existing command modules and test suite.

Repomod currently uses Python 3.15, uv, Ruff, ty, Git subprocesses, GitHub operations, a curated TOML catalog, and an extensive pytest suite.

Its established safety model must remain intact.

Read and follow repository conventions before making modifications.

### 19.1 Proposed incubator layout

```
repomod/
  experiments/
    sx/
      Cargo.toml
      crates/
        sx-kernel/
        sx-sdk/
        sx-repomod/
        sx-cli/
      formal/
        lakefile.toml
        lean-toolchain
        Sx/
          Syntax.lean
          Types.lean
          Semantics.lean
          Effects.lean
          Machine.lean
          Proofs.lean
          Cores/
            Git.lean
            GitHub.lean
            Repomod.lean
          Bridges/
            GitGithub.lean
      programs/
        discover.sx
        status-offline.sx
        status.sx
      fixtures/
      conformance/
      docs/
        charter.md
        language.md
        architecture.md
        extension-api.md
        proof-coverage.md
        benchmark.md
        extraction.md
```

This is a proposed layout, not an inflexible requirement.

Use fewer crates if that reduces needless fragmentation.

The logical components must remain independent even if some share an initial crate.

Keep all prototype dependencies and artifacts isolated from repomod's existing Python packaging.

Do not change the default `repomod` CLI or workflows simply to accommodate the experiment.

### 19.2 Extraction requirement

The kernel must not import repomod-specific modules or types.

The SDK must not depend on repomod.

The formal kernel must not depend on repomod formalizations.

Only the repomod adapter or application crate may depend on both Sx and repomod-related abstractions.

A concrete extraction test should demonstrate that the kernel, SDK, and at least one unrelated sample core can build and run without repomod present.

Do not perform the extraction into a new public repository during the initial campaign unless specifically authorized.

---

# Part VIII — Implementation campaign

## Phase 0 — Baseline and inventory

Before implementation:

1. Inspect the current default branch.
2. Read repository instructions.
3. Inventory commands, modules, and tests.
4. Run the existing non-destructive validation suite.
5. Record existing failures before introducing changes.
6. Map repomod's read-only and write-capability boundaries.
7. Preserve existing catalog and checkout state.

Use the established commands where applicable:

```
uv sync
make test
uv run ruff check src tests
uv run ty check
```

Repomod's instructions also call for synchronization before work. Follow them only in a manner consistent with the current checkout and user-data safety constraints; do not use an unsafe live-catalog mutation as an expedient way to satisfy a procedural step.

Never reset, clean, stash, or overwrite user repositories.

**Deliverable:** Baseline report and architecture inventory.

## Phase 1 — Freeze initial semantics

Implement the minimal language specification.

Deliver:

- Grammar.
- AST.
- Type model.
- Effect model.
- Evaluation rules.
- Resource-budget semantics.
- Failure classification.
- Formal Lean reference evaluator.
- A small, executable semantic fixture corpus.

Start with pure expressions.

Introduce host effects only after pure semantics are functional.

Do not add Git or GitHub logic to the kernel.

**Gate:** All accepted pure fixtures evaluate consistently in Lean, and the initial proof obligations have explicit statuses.

## Phase 2 — Rust interpreter

Implement:

- Parser.
- Type checker.
- Registry.
- Typed values.
- Evaluator.
- Resource accounting.
- Structured errors.
- Continuations.
- Capability request/resume protocol.

Use idiomatic Rust and established high-quality crates where justified.

Avoid custom foundational algorithms when mature dependencies offer smaller, safer implementations.

Avoid adding runtime dependencies merely for convenience.

Maintain straightforward ownership and error behavior.

**Gate:** Rust agrees with Lean across the initial corpus; malformed or unauthorized programs never invoke host capabilities.

## Phase 3 — Core SDK and extension smoke test

Implement:

- Core registration.
- Versioned identity.
- Interface descriptors.
- Type resolution.
- Function dispatch.
- Output validation.
- Dependency resolution.
- Duplicate and incompatible-version rejection.

Create at least one independent sample core, preferably something unrelated to repomod, such as a small SemVer or interval-analysis core.

Demonstrate that the core can be registered without modifying kernel source code.

Implement a meaningful pure composition between the sample core and another core.

**Gate:** Extensibility works without a new kernel construct or repomod-specific dependency.

## Phase 4 — Git and GitHub cores

Implement the minimal models and pure functions required by the initial repomod workload.

Reuse existing repomod behavior as an implementation reference.

Write Lean specifications for high-value decision functions.

Prove carefully scoped properties.

Add a Git/GitHub bridge.

Use deterministic test fixtures rather than live GitHub requests for conformance.

**Gate:** Core behavior is independently testable, bridge contracts are explicit, and no effect leaks into pure functions.

## Phase 5 — Repomod read-only port

Port in this order:

### 5A. Discover

Implement orchestration for discovering catalog discrepancies.

Preserve:

- Repository identity.
- Owner selection.
- Deduplication.
- Missing and uncatalogued classifications.
- Deterministic ordering.
- JSON output contract.

This is the first genuine end-to-end test.

### 5B. Offline status

Implement `status --offline` using existing cached data and local Git observations.

Preserve:

- Local checkout classification.
- Cached CI semantics.
- Stale-data handling.
- Fork/upstream distinctions.
- Findings and visibility rules.
- Existing JSON structure.
- Relevant exit status.

Inject a fixed clock and controlled environment into tests.

### 5C. Online status

Add GitHub host capabilities and concurrent observation collection.

Preserve:

- Correct API batching and pagination.
- Cache semantics.
- Required-check handling.
- Unknown and stale states.
- Rate-limit/error behavior.
- Existing output ordering.

### 5D. Lint orchestration

Port command coordination and filtering.

Keep complex policy algorithms in existing implementations initially.

Use pure cores for reusable classifications and result transformations.

**Gate:** Golden-output parity with the Python implementation on representative existing and new fixtures. All original tests still pass.

## Phase 6 — Guarded mutations (subsequent expansion)

Do not activate production writes during the initial read-only prototype.

Specify, test, and implement write support separately.

Required order:

1. Dry-run sync plans.
2. Guarded sync.
3. Guarded push.
4. Fix planning.
5. Isolated fixer execution.
6. Pull request creation.
7. Guarded merge.

Repomod safety invariants include:

- Validate canonical repository identity and checkout paths.
- Reject unsafe symlink paths.
- Preserve dirty or divergent user work.
- Fast-forward only when currently valid.
- Use repository locks where required.
- Keep fixes in disposable worktrees.
- Preserve recoverable commits after interruption.
- Respect allowed fix paths.
- Revalidate before mutation.
- Pin PR merges to the expected head commit.
- Never silently force-push or overwrite user state.

Plans should carry expected state and required conditions.

Host adapters must independently recheck preconditions at the moment of execution.

Do not assume a pure plan provides transaction isolation.

**Gate:** No observable safety regression, including adversarial and concurrent-change tests.

## Phase 7 — Code Mode evaluation

Once the runtime and read-only repomod application work, compare against JavaScript.

Create a frozen task corpus containing:

- Read-only discovery tasks.
- Multi-source repository status.
- Structured filtering and grouping.
- Pagination.
- Parallel API calls.
- Conditional execution.
- Partial failure handling.
- Incorrect input schemas.
- Core composition.
- Cross-core bridges.
- Representative non-repomod tasks.

Keep tool schemas and available capabilities equivalent.

Measure:

- First-attempt task correctness.
- Final task correctness.
- Input and output token consumption.
- Tokens spent on syntax guidance.
- Tokens spent on repair attempts.
- Static validation failures.
- Runtime failures.
- Capability invocations.
- Unnecessary or duplicate invocations.
- End-to-end task latency.
- Interpreter CPU time and memory.
- Cold and warm startup overhead.

Report by model and task class.

Compare identical task descriptions and tool semantics.

Use paired runs and multiple samples where practical.

Do not cherry-pick favorable examples.

**Gate:** A reproducible comparison with conclusions supported by measured evidence.

---

# Part IX — Repomod safety and compatibility

## 20. Preserve existing contracts

The experiment must not weaken or silently change:

- Catalog semantics.
- Checkout locations and identity.
- Git origin validation.
- Symlink safeguards.
- Local-change preservation.
- CI cache provenance.
- Offline behavior.
- Policy-rule applicability.
- Fixer isolation.
- Per-fixer commits.
- Remote-write authorization.
- Error visibility.
- Deterministic result ordering.
- Documented command flags.

Read current implementation and tests rather than relying only on this charter.

### 20.1 Compatibility fixtures

Use:

- Temporary Git repositories.
- Copied catalogs.
- Fake GitHub responses.
- Fixed clocks.
- Deterministic local state.
- Explicit dirty/divergent/detached checkout scenarios.

Capture comparable outputs from the Python implementation.

Treat the existing application as the behavioral baseline, not automatically as a formally correct implementation.

Where the new architecture exposes a genuine issue, record and test it rather than silently changing behavior.

### 20.2 Shadow execution

Before replacing any real command, support experimental shadow execution.

The original Python command remains authoritative.

The experimental Sx implementation runs against equivalent fixture inputs or safe captured observations and produces a comparison report.

No second application of mutations.

Do not run shadow implementations that perform additional live writes.

---

# Part X — Performance and complexity discipline

## 21. Runtime budget philosophy

Sx should be exceptionally lightweight.

Prefer:

- Small Rust modules.
- Minimal dependencies.
- In-process native execution.
- Compact typed values.
- Bounded allocation.
- Efficient immutable bindings.
- Explicit continuations.
- No GC requirement if avoidable.
- No embedded JavaScript or Python runtime.
- No unnecessary serialization between native cores.
- No hidden thread pools inside the evaluator.

Avoid optimizing tiny parser costs before measuring host-call and scheduling overhead.

Report separate measurements for:

1. Parsing/type checking.
2. Pure evaluation.
3. Continuation suspension/resumption.
4. Host dispatch overhead.
5. Whole-application behavior.

### 21.1 Complexity guardrails

New language features require evidence.

For every proposed construct, document:

- The concrete use case.
- Why existing constructs are insufficient.
- Changes required in Lean semantics.
- Changes required in Rust.
- Impact on model generation.
- New safety or complexity costs.
- Whether the need belongs in a pure core instead.

Prefer extending the core library over the language itself.

Reject macros, general recursion, unrestricted dynamic dispatch, and arbitrary embedded host-language execution unless future empirical work establishes a compelling need.

### 21.2 Native and WASM strategy

The v0 reference runtime is native Rust.

Do not implement WASM merely as an architectural demonstration.

Once the native runtime has stable semantics and benchmarks, consider:

- WASM execution of the Sx interpreter.
- WASM components implementing sandboxed pure cores.
- Portability across agent hosts.

Each must justify its runtime, packaging, and compatibility costs.

---

# Part XI — Testing and CI

## 22. Required test layers

### Unit tests

Parser, types, values, evaluation, budgets, errors, registry, and core functions.

### Property tests

Type preservation checks, determinism under controlled inputs, round trips, namespace collisions, and schema compatibility.

### Differential tests

Lean versus Rust behavior.

### Fuzz tests

Source admission, AST validation, continuation handling, malformed host results, and bounded resource behavior.

### Core tests

Domain-specific invariants, boundary conditions, and version compatibility.

### Bridge tests

Identity consistency, conversion correctness, invalid input rejection, and composition properties.

### Application tests

Repomod parity, deterministic fixtures, existing CLI behavior, and original regression tests.

### Performance tests

Microbenchmarks and representative application benchmarks.

Maintain a machine-readable proof and conformance ledger.

Do not equate the presence of a test with proof of a universal semantic property.

## 23. CI requirements

Pin toolchain versions.

Keep the formal verification environment reproducible.

Run Rust formatting, linting, tests, and differential fixtures.

Run Lean proof checks separately, reporting their status explicitly.

Do not silently skip formal checks and then report a fully green qualification.

Do not generate committed benchmark claims without recording their environment.

Keep intermediate build output under ignored locations.

Avoid making all ordinary repomod development depend on a Lean toolchain during incubation.

---

# Part XII — Public API and documentation

## 24. Initial extension documentation

Write enough documentation that someone unfamiliar with repomod can create a core.

Include:

- What a core is.
- Core lifecycle.
- Type definitions.
- Function declarations.
- Registration.
- Version resolution.
- Dependency handling.
- Error contracts.
- Testing requirements.
- Optional Lean specification.
- Bridge construction.
- Trust and sandboxing boundaries.
- Compatibility expectations.
- How host capabilities differ from pure cores.

Provide a complete sample extension.

Document how it can be built and run without repomod.

## 25. Long-term distribution model

The architecture should eventually support:

- Built-in standard cores.
- Independently published pure cores.
- Independently published bridges.
- Host-specific capability providers.
- Package-specific proof/conformance bundles.
- Generated compact tool documentation for LLMs.
- Stable interface discovery.
- Reproducible package resolution.

A future registry may distribute these artifacts, but must not control the language specification.

Avoid making any registry a prerequisite for local execution.

## 26. Model-facing interface

A core registry should ultimately be able to generate concise model-facing descriptions containing:

- Available functions.
- Their argument types.
- Return types.
- Short semantic descriptions.
- Effect requirements.
- Representative examples.

The user or agent should not need to load a large hand-written language tutorial to invoke ordinary functions.

Evaluate whether generated signatures and a few examples are sufficient for reliable model behavior.

This is important to Code Mode token economics.

---

# Part XIII — Extraction and long-term roadmap

## 27. Graduation from repomod

Repomod is an incubator, not the permanent architectural center.

Graduation criteria:

1. Kernel builds without repomod.
2. Formal semantics build without repomod.
3. SDK supports unrelated cores.
4. Public registry API exists.
5. At least one non-repomod sample application works.
6. Lean/Rust differential conformance passes.
7. Kernel-specific dependencies do not reference repomod.
8. Extraction can preserve public APIs and test fixtures.

Prepare an extraction plan documenting the independent repository/workspace boundaries.

Do not extract prematurely or create needless repository management overhead.

## 28. Future research directions

Preserve these as research questions rather than v0 commitments.

### 28.1 More sophisticated effect analysis

Resource-aware capabilities, conflict detection, scoped authorization, and selectively permitted parallel writes.

### 28.2 Typed execution plans

Static dependency graphs and automatic scheduling of independent operations.

### 28.3 Direct AST generation

Constrained decoding into a typed representation instead of unconstrained source text.

### 28.4 Core specialization

Static compilation or specialization of frequently executed pure compositions.

### 28.5 Portable extension runtime

WASM components for sandboxed third-party cores.

### 28.6 Richer proof composition

Reusable proof interfaces, refinement relations, and stronger correspondence evidence between specifications and implementations.

### 28.7 Agent harness integration

Optional Code Mode backends for existing agent harnesses, without forcing Sx into their control flow.

### 28.8 Alternative syntaxes

Evaluate whether Lisp surface syntax is genuinely preferable to a compact typed expression language or direct structured AST generation.

The semantic IR must remain distinct from any chosen surface syntax.

---

# Part XIV — Agent execution instructions

## 29. Your assignment

You are an autonomous coding agent implementing the initial Sx research prototype inside repomod.

Implement the foundational architecture and the first genuinely usable read-only repomod integration.

The primary deliverable is working software with executable semantics, tests, and documentation—not merely a design proposal.

Proceed without requesting routine confirmations.

Use the current repository contents as ground truth when this charter differs from implementation details.

Do not rewrite unrelated repomod functionality.

Do not replace mature working code for the sake of language purity.

Do not add gratuitous dependencies, frameworks, abstractions, or crates.

### 29.1 Execution priorities

Priority order:

1. Correctness and safety.
2. Clear semantic boundaries.
3. Formal tractability.
4. Independent extensibility.
5. Working repomod integration.
6. Performance.
7. Code size and implementation simplicity.
8. Ergonomic polish.

Do not optimize the final priorities by violating the earlier ones.

### 29.2 Minimum completed prototype

Before reporting the founding campaign complete, deliver:

- A written language specification.
- An executable Lean reference evaluator.
- Initial kernel safety proofs with a precise proof-status ledger.
- A working Rust parser, checker, and interpreter.
- An explicit continuation-based tool protocol.
- A core registry with at least one independently registered non-repomod core.
- Initial Git and GitHub domain abstractions.
- A typed Git/GitHub bridge.
- A working Sx-backed `repomod discover` experiment.
- A meaningful read-only status experiment, preferably `status --offline`.
- Differential Lean/Rust test fixtures.
- Existing repomod test compatibility.
- Reproducible commands to run all major checks.
- A documented next-stage plan.

If a significant requirement cannot be completed without compromising correctness, isolate it, document the obstacle, and keep completed work working.

Do not replace an unproved theorem with an assumption merely to satisfy a milestone.

### 29.3 Source control

Work according to repository conventions.

Commit coherent, verified milestones.

Prefer direct commits to the authorized working branch when that is the repository's established workflow.

Do not open unnecessary pull requests.

Do not mutate unrelated repositories or user checkouts.

Never run live repomod fix, push, merge, garbage collection, or destructive synchronization during this experiment.

Use isolated fixtures for mutation testing.

### 29.4 Documentation obligations

Keep the charter and implementation notes updated when evidence forces a design change.

Maintain an explicit decision log for:

- Language additions.
- Type-system changes.
- New effect categories.
- Core-interface revisions.
- Bridge design changes.
- Proof-scope changes.
- Performance tradeoffs.

State what was considered and why the chosen approach is smaller or safer.

Avoid sprawling speculative documentation that has no implementation consequence.

### 29.5 Final report

Report:

1. Implemented components and associated commits.
2. Working programs and invocation examples.
3. Lean theorems actually checked.
4. Lean/Rust conformance results.
5. Existing repomod test results.
6. Extension API demonstration.
7. Performance measurements, if available.
8. Remaining limitations and unsafe assumptions.
9. Features intentionally excluded.
10. Recommended next implementation milestones.

Distinguish accomplished facts from future plans.

Do not claim that a formally specified model establishes correctness of real Git or GitHub effects without separate evidence.

---

# Final design constraints

Preserve these as the project's governing architectural rules:

**The kernel must remain ignorant of application domains.**

**Pure cores must remain independently specifiable.**

**Bridges must compose contracts, not smuggle effects.**

**Host capabilities must enforce actual authority and real-world preconditions.**

**Lean proves properties of its models; Rust conformance and external adapter correctness require separate evidence.**

**Adding a new domain should normally require no language changes.**

**Repomod must remain safe and useful throughout incubation.**

**The project is successful only if its complexity remains proportional to the orchestration problem—not to the total complexity of software it can coordinate.**

Begin with the repository inventory, then implement vertical slices through Lean, Rust, core registration, and repomod. Continue until the minimum completed prototype passes its qualification gates or a concrete, documented blocker prevents safe completion.

## Two design decisions I'd emphasize beyond the charter

The first is a stable extension interface before a stable binary ABI. Independent developers should be able to write cores against the same Rust SDK immediately. Compiled-component interchange can come later. Freezing the binary ABI too early would introduce substantial complexity without making the initial language experiment more informative.

The second is that I'd resist turning Lean into an elaborate verification framework for third-party packages. The kernel needs strong proofs, and core authors should be able to supply independently checkable proofs. But a useful, conformance-tested core without Lean should still be admissible, with its assurance level accurately represented.

The natural progression is:

First proving ground

repomod

Git + GitHub + policy cores, real orchestration, existing regression tests

First independent extension

A separately registered SemVer or interval-analysis core proving the kernel isn't repomod-specific

First external application

A small tool-orchestration application without repomod dependencies

First Code Mode qualification

Paired JavaScript/Sx generation benchmarks, including model retries and actual tool outcomes

The most ambitious idea here isn't the Lisp syntax. It's making domain knowledge modular, typed, composable, and independently verifiable, while keeping the model-facing language almost trivially small.

If that holds up under repomod, we have the beginnings of something much more general than a scripting experiment.