# Sx language, draft v0

This is the working specification for the Python interpreter. It changes as the syntax pilot, the
interpreter, and the model probe expose problems. Every change updates this file, the fixtures, and the
interpreter in the same commit.

## Programs

A program is one expression. There are no top-level definitions, imports, macros, quoting, or
recursion. Evaluation is strict, and arguments evaluate left to right, except inside `par` and
`par-map`.

Every program terminates. Iteration runs over lists that already exist, and the only repetition,
`retry` and `wait-until`, has an attempt count the checker computes from literal options. A
program cannot loop over an unbounded sequence, so it cannot follow a pagination cursor. A
capability that reads a paginated source returns every page as one result.

## Lexical syntax

| Token | Form | Notes |
| --- | --- | --- |
| Comment | `; text` | Runs to end of line. |
| Whitespace | space, tab, newline, `,` | Commas are whitespace, as in Clojure. |
| Boolean | `true`, `false` | |
| Integer | `-?[0-9]+` | Signed 64-bit. A literal outside that range is rejected. |
| Duration | `250ms`, `5s`, `2m`, `1h` | Only in `retry` and `wait-until` options. |
| String | `"..."` | UTF-8. Escapes: `\"`, `\\`, `\n`, `\t`, `\u{HEX}`. |
| Keyword | `:name` | Record field names only. |
| Symbol | `name`, `name?` | Lowercase words of letters and digits joined by single `-`, starting with a letter. |
| Operator | `+ - * = not= < <= > >=` | Prelude functions |
| Wildcard | `_` | Match patterns only |
| Variant | `OnBranch` | A capital letter, then letters and digits. Enum variants in patterns. |
| Qualified symbol | `core.name`, `core.Type` | A core function, core type, or capability. Core IDs are one segment. |
| List | `[a b c]` | |
| Record | `{:k v ...}` | Field names are unique. |
| Form | `(head args...)` | |

## Types

| Type | Values |
| --- | --- |
| `Bool` | `true`, `false` |
| `Int` | Signed 64-bit integers |
| `String` | UTF-8 strings |
| `Unit` | One value, returned by capabilities with no result |
| `List<T>` | Ordered, finite |
| `Tuple<T1, ..., Tn>` | Produced by `par` and `group-by` |
| `Option<T>` | `(some x)`, `none` |
| `Result<T, E>` | `(ok x)`, `(err e)` |
| `Record {k: T, ...}` | Structural, with exactly the declared fields |
| Core enum | Nominal, declared by a core, for example `git.Checkout` |
| Core nominal | Nominal wrapper over a base type, declared by a core, for example `git.RepoId` |

There is no `Any`, no subtyping, no implicit conversion, and no user-declared type. Only core
functions construct core enum and core nominal values. Each enum variant carries zero or one
value. A variant that needs several values carries a record.

Programs have no type annotations. The checker infers types bidirectionally from literals,
bindings, prelude and core signatures, capability signatures, and the expected type of each
argument position. A lambda parameter gets its type from the position the lambda fills. `none`,
`[]`, `(ok x)`, and `(err e)` take their missing type parameters from the expected type. The
checker rejects any expression whose type it cannot resolve.

Equality is structural on every type. Ordering with `<`, `<=`, `>`, and `>=` is defined on `Int`
and on `String`, where strings compare lexicographically by Unicode code point.

## Special forms

| Form | Type rule |
| --- | --- |
| `(let [p1 e1 p2 e2 ...] body)` | Each `e` is visible to later bindings and to `body`. |
| `(if c then else)` | `c` is `Bool`. Both branches have the same type. |
| `(match e clause...)` | See [Match](#match). |
| `(fn [p ...] body)` | Allowed only as a function argument to a prelude function or `par-map`. |
| `(get r :field)` | `r` is a record with `field`. A missing field is rejected by the checker. |
| `(call cap.id arg)` | `arg` has the capability's input type. The result has its output type. |
| `(attempt e)` | `Result<T, CallError>`, where `e` has type `T`. See [Retries and polling](#retries-and-polling). |
| `(retry {options} e)` | `T`. Re-evaluates `e` after a failure. See [Retries and polling](#retries-and-polling). |
| `(wait-until {options} f e)` | `T`. Re-evaluates `e` until `f` returns `true` for its value. |
| `(par e1 ... en)` | `Tuple<T1, ..., Tn>`. Every branch has only `read` effects. |
| `(par-map {:limit n} f xs)` | `List<U>`. `n` is an integer literal. `f` has only `read` effects. |

A pattern `p` is a symbol or a tuple pattern `[p1 ... pn]`. Tuple patterns appear in `let` and in
`fn` parameters.

A function argument `f` is either a `fn` form or the name of a prelude or core function with the
right arity. Function values cannot be bound, returned, or stored.

### Match

```
(match checkout
  (OnBranch info) (get info :branch)
  (Detached head) "detached"
  _ "unavailable")
```

The scrutinee's type determines which variant names are in scope, so patterns are unqualified.
`Option` patterns are `(some x)` and `none`. `Result` patterns are `(ok x)` and `(err e)`. `_`
matches anything. A `match` without `_` must name every variant. A clause that can never match
is rejected.

## Retries and polling

These forms follow xsh's `try`, `retry`, and `wait until`. Each does one thing. `retry` and
`wait-until` repeat, and `attempt` captures. All options are literals, so the checker knows the
largest number of attempts before the program runs, and the `calls` budget accounts for it.

### attempt

`(attempt e)` evaluates `e`. If a capability call inside `e` fails and nothing inside `e` handled
it, the result is `(err e)` with that `CallError`. Otherwise the result is `(ok value)`. Errors
other than `call-failed` pass through, as listed in [Failures](#failures).

### retry

```
(retry {:delays [1s 2s 4s] :on [Timeout Unavailable]}
  (call github.ci {:repo repo :sha sha}))
```

`retry` evaluates `e`, and evaluates it again after each delay while it fails with a retried
kind. An empty delay list means exactly one attempt, so `[1s 2s 4s]` allows up to four. A failure
whose kind is not in `:on` ends the form at once, without using a delay. When attempts run out,
the last failure propagates as if `retry` were not there. Wrap the form in `attempt` to capture
it.

`:on` lists [`CallError` kinds](#call-errors). Without `:on`, `retry` retries the transient kinds
`Timeout`, `Unavailable`, and `RateLimited`. It never retries `NotFound`, `PermissionDenied`, or
`Invalid` unless `:on` names them.

`{:backoff [100ms 5s] :within 30s}` replaces `:delays` with computed delays. The first delay is
`100ms`, and each later delay doubles, never longer than `5s`. A delay is taken only if the total
of all delays so far stays within `30s`. Nothing is added at random. `:within` bounds the total
delay, not wall-clock time. The host enforces wall-clock time separately.

`e` may contain several calls, and each attempt evaluates all of `e` again. A `write` capability
inside `e` must be declared `idempotent`, because a retry may repeat it. The checker rejects
`retry` around a non-idempotent write with `retry-write`.

### wait-until

```
(wait-until {:within 10m :every 15s}
  (fn [job] (= (get job :state) "done"))
  (call jobs.status {:id id}))
```

`wait-until` evaluates `e`, then applies `f` to its value. If `f` returns `true`, the form returns
that value. Otherwise it waits and evaluates `e` again. `:every` sets a fixed interval, and
`{:backoff [first cap]}` sets doubling intervals as in `retry`. `:within` is required and bounds
the total waiting time. When the next wait would exceed it, the form fails with a `CallError` of
kind `Timeout`, which `attempt` can capture and `retry` can retry. A failed call inside `e` fails
the form at once. Wrap `e` in `retry` to tolerate it. `e` must have only `read` effects. Intervals
must be at least `1ms`.

### Delays

A delay is a request like a capability call. The machine suspends with a delay request carrying
its ID and duration, and the host resumes it after waiting. The machine never reads a clock.
Fixture hosts resume at once, so tests run in no time and stay deterministic. The trace records
each delay.

## Prelude

Prelude functions are unqualified, and the kernel defines them. Function arguments come first,
as in Clojure.

| Function | Type |
| --- | --- |
| `+ - *` | `Int Int -> Int` |
| `quot rem` | `Int Int -> Int`, truncating toward zero |
| `= not=` | `T T -> Bool` |
| `< <= > >=` | `T T -> Bool` for `Int` or `String` |
| `not` | `Bool -> Bool` |
| `and or` | `Bool Bool -> Bool`. Both arguments always evaluate. Use `if` to make a call conditional. |
| `str` | `String String -> String` |
| `count` | `List<T> -> Int` |
| `take` | `Int List<T> -> List<T>`. A negative count is an arithmetic error. |
| `concat` | `List<T> List<T> -> List<T>` |
| `flatten` | `List<List<T>> -> List<T>` |
| `contains?` | `List<T> T -> Bool` |
| `map` | `(T -> U) List<T> -> List<U>` |
| `filter` | `(T -> Bool) List<T> -> List<T>` |
| `any? all?` | `(T -> Bool) List<T> -> Bool` |
| `reduce` | `(A T -> A) A List<T> -> A` |
| `sort-by` | `(T -> K) List<T> -> List<T>`, stable, `K` is `Int` or `String` |
| `distinct-by` | `(T -> K) List<T> -> List<T>`, keeps the first element for each key |
| `group-by` | `(T -> K) List<T> -> List<Tuple<K, List<T>>>`, groups in first-occurrence order |

Integer overflow, `quot` or `rem` by zero, and a negative `take` count end evaluation with an
`arithmetic` error. Nothing wraps or saturates.

Text operations other than `str`, `=`, and ordering live in the `text` core, for example
`text.casefold` and `text.from-int`.

## Effects and authorization

Each capability declares an ID, an input type, an output type, and an effect class of `read` or
`write`. A `write` capability also declares whether it is `idempotent`, meaning that repeating a
call with the same input has the same effect as making it once. Effects are inferred from the program text. Without dynamic dispatch, the set of
capability IDs a program can call is exactly the set of IDs named in its `call` forms.

The host supplies an allow-list of capability IDs for each execution. The checker rejects a
program that names any ID outside the allow-list. A rejected program produces no capability
request. `sx explain` prints a program's capability set and effect classes without running it.

`par` branches and `par-map` functions must have only `read` effects. Read effects in parallel
branches do not observe one consistent snapshot of the outside world. Determinism holds only for
fixed responses.

Pure core functions have no effects. The registry rejects a core whose signature declares one.

## Execution

The machine is a step function over a state held as plain data. Each run ends in one of three
outcomes.

- `Completed(value)`: the program finished.
- `Suspended(requests, continuation)`: the program needs capability results.
- `Failed(error)`: the program stopped. See [Failures](#failures).

Each request has an ID derived from its position in the program and, inside `par-map`, its
element index. IDs never depend on completion order. One suspension holds every request that is
ready: one per active `par` branch, and up to `limit` for `par-map`.

The host resumes with a response for every request in the suspension. Each response is either an
output value or a `CallError`. The host may run the requests concurrently and in any order. The
machine assembles results in source order.

The machine fails with `invalid-host-response` if a response has an unknown ID, a duplicate ID,
or a value that does not match the output type, or if a request has no response.

The trace records each request (ID, capability, input) and each response (ID, outcome). Fixtures
compare traces sorted by request ID.

## Failures

| Error | Cause | Caught by `attempt` |
| --- | --- | --- |
| `rejected` | Parse, name, type, effect, or authorization error. Nothing ran. | No |
| `call-failed` | A capability returned a `CallError`, or `wait-until` timed out, and nothing captured it | Yes |
| `invalid-host-response` | See [Execution](#execution) | No |
| `budget-exhausted` | A budget ran out. The error names the budget. | No |
| `arithmetic` | Overflow, division by zero, negative `take` count | No |
| `internal` | Interpreter bug | No |

### Call errors

`CallError` is `Record {kind: CallErrorKind, message: String}`. Programs branch on the kind,
never on the message. `CallErrorKind` is a closed kernel enum:

| Kind | Meaning | Retried by default |
| --- | --- | --- |
| `Timeout` | The operation exceeded its time limit. | Yes |
| `Unavailable` | The service could not be reached or reported a transient failure. | Yes |
| `RateLimited` | The service refused the request because of request volume. | Yes |
| `NotFound` | The target does not exist. | No |
| `PermissionDenied` | The host or service refused access. | No |
| `Invalid` | The service rejected the input. | No |
| `Conflict` | The target changed, or is in a state that prevents the operation. | No |
| `Failed` | Any other failure. | No |

The host maps each failure to exactly one kind. A failure that fits no other kind is `Failed`.

When a `par` branch fails, the machine waits for the current suspension's responses, then
reports the failure with the lowest request ID. Effects that already ran are not rolled back.

## Values and JSON

Capability inputs and outputs, program results, and fixtures use one JSON encoding. Decoding
always knows the expected type, so every encoding below is unambiguous.

| Type | JSON |
| --- | --- |
| `Bool` | `true` or `false` |
| `Int` | An integer in the signed 64-bit range. A fraction, an exponent, or `true` is rejected. |
| `String` | A string |
| `Unit` | `null` |
| `List<T>` | An array |
| `Tuple<T1, ..., Tn>` | An array of exactly `n` elements |
| `Option<T>` | `null` for `none`, the encoding of `x` for `(some x)` |
| `Result<T, E>` | `{"ok": x}` or `{"err": e}` |
| `Record {k: T, ...}` | An object with exactly the declared fields, named without the colon |
| Core enum, variant without a value | The variant name as a string, for example `"Missing"` |
| Core enum, variant with a value | `{"tag": "OnBranch", "value": x}` |
| Core nominal | The encoding of its base type |

`Option<Option<T>>` has no JSON encoding, because `null` would be ambiguous. The checker rejects
a capability or program result whose type contains it.

Decoding a capability response fails with `invalid-host-response` when a value does not match
the expected type. A missing field, an extra field, and `null` for a non-`Option` type are all
mismatches. Nothing is dropped, defaulted, or converted. The error gives the path to the bad
value, the expected type, and what was found, for example
`$.repos[3].name: expected String, found number`.

Encoding writes record fields in declaration order. Comparisons of JSON values, in fixtures and
in parity tests, ignore object key order.

## Diagnostics

The checker reports every error it finds, up to 20, followed by a count of the rest. The reader
recovers at the next balanced form, so one unclosed bracket does not hide later errors. Nothing
runs while any error remains.

Each diagnostic has these parts:

| Part | Content |
| --- | --- |
| Code | Stable ID, for example `unknown-field`. Fixtures and the probe key on it. |
| Span | Line and column of the start and end of the offending text |
| Excerpt | The source line, with the span underlined |
| Message | What is wrong, in one sentence, using Sx type names |
| Detail | Expected and found types, or the names that do exist |
| Fix | The exact text to write, when one fix is likely |
| Help | The `sx help NAME` entry to read next |

In text form, where a habit's code is written `habit:CONSTRUCT`:

```
error[unknown-field] 4:12-4:17
  (get repo :nmae)
            ^^^^^
  This record has no field :nmae.
  Fields: :id, :name, :tags
  Fix: (get repo :name)
  Help: sx help get
```

`--format json` emits the same parts as one object per diagnostic.

A diagnostic's message follows these rules:

- Say what is wrong before saying why.
- Name types as Sx writes them, for example `List<String>`, never interpreter class names.
- Never use type-theory terms such as "unify" or "judgment".
- Suggest a name only when it is within edit distance 2 of a name in scope.

The checker's codes are:

| Group | Codes |
| --- | --- |
| Reading | `unclosed`, `unexpected-close`, `bad-literal`, `bad-symbol`, `int-range`, `bad-escape`, `record-key`, `record-missing-value`, `duplicate-field`, `empty-program`, `multiple-expressions` |
| Names | `unknown-name`, `unknown-function`, `unknown-capability`, `unknown-field`, `unknown-variant` |
| Calls | `arity`, `type-mismatch`, `unresolved-type`, `not-a-record` |
| Forms | `condition-not-bool`, `branch-mismatch`, `fn-position`, `bad-pattern`, `bad-option` |
| Match | `non-exhaustive`, `unreachable-clause` |
| Effects | `unauthorized`, `parallel-write`, `par-map-limit`, `retry-write`, `wait-write` |
| Size | `too-large`, naming the `source-bytes`, `nodes`, or `depth` budget |
| Habits | `habit`, naming the construct. The reader detects `nil`, `capitalized-boolean`, `quote`, `reader-macro`, `anonymous-fn-shorthand`, `set-literal`, `threading`, `js-operator`, `js-object-key`, `method-call`, and `namespace-slash`. The checker detects `defn`, `keyword-call`, and `argument-order`. |

A habit diagnostic names the language the construct comes from and gives the Sx form. For
example, `(:name repo)` gets `Fix: (get repo :name)`.

A runtime failure is reported in the same format. It adds the error class from
[Failures](#failures), and, for capability errors, the request ID and capability. Its span points
at the form that failed.

## Help

`sx help` prints the guide: every form, the prelude in one line per function, and a few complete
examples. `sx help NAME` prints one entry, where `NAME` is a form, a prelude function, a qualified
core function or type, or a capability. An entry has:

- The signature, and for capabilities, the effect class.
- A one-sentence description.
- At least one example with its result.

The registry generates core and capability entries from their declarations. Every example is a
test, so a help entry cannot drift from the interpreter.

## Budgets

The host sets every budget. A program cannot read or raise one.

| Budget | Limits |
| --- | --- |
| `source-bytes` | Program text size |
| `nodes` | Syntax tree size |
| `depth` | Syntax tree nesting |
| `steps` | Machine steps |
| `calls` | Total capability requests |
| `delay` | Total time requested by delays across the run |
| `concurrency` | Requests in one suspension. A larger `par-map` limit is rejected. |
| `value-size` | Size of any intermediate or final value |
| `applications` | Function argument applications |

The host enforces wall-clock time outside the machine.

## Example

This sketch shows the intended shape of `discover.sx`. The capability signatures are not fixed
yet.

```
(let [catalog (call catalog.load {})
      pages (par-map {:limit 8}
              (fn [owner] (call github.owner-repos {:owner owner}))
              (get catalog :owners))
      found (sort-by text.casefold (distinct-by text.casefold (flatten pages)))
      found-keys (map text.casefold found)
      known-keys (map text.casefold (get catalog :repos))]
  {:owners (get catalog :owners)
   :uncatalogued (filter (fn [name] (not (contains? known-keys (text.casefold name)))) found)
   :missing (sort-by text.casefold
              (filter (fn [name] (not (contains? found-keys (text.casefold name))))
                      (get catalog :repos)))})
```
