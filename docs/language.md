# Sx language, draft v0

This is the working specification for the Python interpreter. It changes as the syntax pilot, the
interpreter, and the model probe expose problems. Every change updates this file, the fixtures, and the
interpreter in the same commit.

## Programs

A program is one expression, the result, optionally preceded by top-level definitions. There are no
imports, macros, quoting, or recursion. Evaluation is strict, and arguments evaluate left to right,
except inside `par` and `par-map`. A program may instead be a [script](#scripts): one `script` form
that declares typed inputs around that expression.

A definition is a form before the final expression:

```
(def repos (call catalog.list {}))
(defn name-of [r] (get r :name))
(def _ (call audit.log {:count (count repos)}))
{:names (map name-of repos)}
```

`(def name expr)` binds a value, and `(defn name [params] body)` binds a local function, exactly as
`(fn [params] body)` does in a `let` binding. The program means the same as one `let` that binds the
definitions in order around the final expression, so a later form sees the names of the earlier ones, a
`defn` body sees only the names before it and not its own name, and a later definition of the same name
shadows the earlier one. `name` follows the rules of a `let` binding: a form name is a `reserved-name`
error, `_` runs `expr` for its effect and binds nothing, and a tuple pattern takes a value apart. A `defn` takes
one body expression and no docstring. `sx explain` and `--emit ir` see only the `let`. Definitions before a `script` form move into its body, so
they see its inputs.

A definition is allowed only at the top level, before the final expression. Anywhere else, `def` and
`defn` are the `defn` habit, with a fix that points to `let`. A form before the final expression that is not
a definition is `multiple-expressions`, with the fix `(def _ expr)`. A program whose last form is a definition has no
result and is `missing-result`.

Every program terminates. Iteration runs over lists that already exist, and every form that
repeats, `retry`, `wait-until`, and `loop`, takes a literal bound the checker reads before the
program runs. There is no general recursion and no unbounded loop.

## Lexical syntax

| Token | Form | Notes |
| --- | --- | --- |
| Comment | `; text` | Runs to end of line. |
| Whitespace | space, tab, newline, `,` | Commas are whitespace, as in Clojure. |
| Boolean | `true`, `false` | |
| Integer | `-?[0-9]+` | Signed 64-bit. A literal outside that range is rejected. |
| Float | `1.5`, `-0.25`, `2e3`, `1.5E-7` | Digits with a fraction, an exponent, or both. `1` is an Int. A literal beyond the finite 64-bit range, such as `1e999`, is rejected. |
| Duration | `250ms`, `5s`, `2m`, `1h` | Only in `retry` and `wait-until` options and as the argument of `sleep`. |
| String | `"..."` | UTF-8. Escapes: `\"`, `\\`, `\n`, `\r`, `\t`, `\u{HEX}`. |
| Pattern string | `#"..."` | A String whose text is taken as is: backslashes stay, so `#"\d+"` is the four characters `\d+`. Only `\"` is an escape, and it stands for `"`. For regular expressions; see [The re core](#the-re-core). A Rust-only form. |
| Keyword | `:name` | Record field names only. Same characters as a symbol. |
| Symbol | `name`, `repo-id`, `repoId`, `ahead_count`, `ok?`, `acc'` | A lowercase letter, then letters, digits, `-`, and `_`, then optionally `?` or primes. |
| Operator | `+ - * / = not= < <= > >=` | Prelude functions |
| Wildcard | `_` | Any pattern position: discards the value |
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
| `Float` | IEEE-754 binary64, finite values only. JSON cannot carry an infinity or NaN, so an operation that would produce one stops the program. |
| `String` | UTF-8 strings |
| `Bytes` | An opaque byte string from a tool. It has no literal, and the prelude only measures and compares it. |
| `Unit` | One value, returned by capabilities with no result and by `sleep` |
| `List<T>` | Ordered, finite |
| `Tuple<T1, ..., Tn>` | Produced by `par`, `group-by`, and bracket literals of mixed types or used as tuples |
| `Option<T>` | `(some x)`, `none` |
| `Result<T, E>` | `(ok x)`, `(err e)` |
| `Record {k: T, ...}` | Structural, with exactly the declared fields |
| `Record open {k: T, ...}` | Declared by a tool, never written in a program. It has the named fields, and decoding drops any other field a tool returns. |
| `Map<String, V>` | Any number of string keys, each with a value of type `V`. Keys are always strings, because JSON object keys are strings, so `Map<Int, V>` and every other key type are rejected. |
| Core enum | Nominal, declared by a core, for example `git.Checkout` |
| Core nominal | Nominal wrapper over a base type, declared by a core, for example `git.RepoId` |
| Core-validated nominal | A nominal over `String` whose values the core validates and canonicalizes, for example `decimal.Decimal`. Only the core's functions build one. |

There is no `Any`, no subtyping, and no user-declared type. `Int` and `Float` never convert into
each other: `(+ 1 1.5)`, `(< 1 1.5)`, and `(= 2 2.0)` are each a `type-mismatch` whose fix is
`(float n)` on the `Int` side. `(float n)` turns an `Int` into a `Float`, and `(round x)`,
`(floor x)`, and `(ceil x)` turn a `Float` into an `Int`. Core functions construct core enum
and core nominal values. A program may also write a variant that carries no value by name, such
as `Failing`, where the expected type is the enum, for example `(= state Failing)`. Each enum
variant carries zero or one value. A variant that needs several values carries a record.

There is one implicit conversion, option promotion: where the checker expects `Option<T>`, a
value of type `T` is accepted and becomes `(some value)`. So `(if empty? none name)` has type
`Option<String>`, and `(recur next ...)` may pass a `String` to a binding of type
`Option<String>`. Branch results join field by field, so a branch returning
`{:branch name}` and one returning `{:branch none}` join to `Record {branch: Option<String>}`.
Promotion never applies when `T` is itself an `Option`.

Branches that return records with different fields join to the union of the fields. A field in
both joins as above. A field in only one record becomes `Option<T>`, or stays `Option<T>`, and the
other branch's record has `none` for it. The fields keep the first branch's order, then the new
fields in order of first appearance. So `{:number n :ok true}` and `{:number n :ok false :kind "x"}`
join to `Record {number: Int, ok: Bool, kind: Option<String>}`, and `(get r :kind)` reads the
result. This holds wherever branches join: `if`, `match`, `cond`, and inside lists and tuples.
Records whose shared fields have types that do not join are still a `branch-mismatch`.

A record literal checked against a known record type, such as a capability's input or a
record field of that type, may omit any field whose type is `Option<T>`. The omitted field is `none`, so
`(call audit.page {})` sends `{"cursor": null}`. Omitting a field that is not optional is a
`missing-field` error. A record literal with no known type has exactly the fields it writes.

There is one more conversion, and only where branches join. A core enum whose variants all carry
no value joins with `String` as a `String`, and the enum side becomes its variant's name. This
holds for the branches of `if`, `match`, and `cond`, and field by field, element by element, and
item by item inside the records, lists, and tuples they return. So `(if failed? "missing" state)`
has type `String`, and so do the `state` fields of `{:state state}` and `{:state "missing"}`
joined together. The name is what the JSON encoding already writes for the variant, so the result
encodes the same either way. No other position converts an enum to a `String`.

A string literal where the checker expects a core enum is the variant it names. So
`(= (get ci :state) "Passing")` compares with the variant `Passing`, a record field typed as the
enum takes `"Failing"`, and a core function's enum parameter takes
`(decimal.round x 2 "HalfEven")`. A variant whose name is not a capitalized symbol, such as an imported
`rush`, can only be written as a string, and `unknown-variant` for a bare `Rush` names that
spelling and offers it as the fix. A string literal that names no variant of the enum, or names a variant
that carries a value, is an `unknown-variant` error that lists the variants. A string that is not a
literal never becomes an enum.

A bracket literal whose elements have different types is a tuple. A bracket literal with a known
expected type, a `List<T>` or a `Tuple<...>` of its length, has that type. Any other bracket literal
whose elements share a type takes its shape from its first use as a list or as a tuple of its length:
a list function such as `map` makes it a `List<T>`, and a tuple pattern such as `[repo branch]`
makes it a `Tuple<T, T>`. A literal that no use settles is a `List<T>`. So `[1 2]` alone is a
`List<Int>`, `[(- 0 count) tag]` is a `Tuple<Int, String>`, and in
`(par-map {:limit 8} (fn [[repo branch]] ...) (keep (fn [s] ... [(get s :repo) b] ...) statuses))`
each `[(get s :repo) b]` is a `Tuple<String, String>`. Two undecided literals of different lengths
become lists. A use that fits neither shape, such as a tuple pattern of another length, is a
`type-mismatch`.

A `Map<String, V>` holds string keys that a program computes, where a record has fixed field names.
It is always in ascending key order, by Unicode code point, whatever order its entries were added
in: `keys`, `vals`, `entries`, and the JSON encoding all follow that order. Equality on maps is
structural and ignores insertion order. Nothing writes a map directly. Tools return one, and
`assoc` and `into-map` build one. `{}` with a known `Map<String, V>` type is the empty map, and a
non-empty record literal is never a map. A `{}` with no known type is the empty record until its
first use settles it: `(assoc {} "a" 1)` and `(reduce (fn [acc x] (assoc acc ...)) {} xs)` make it
an empty `Map<String, V>`, and a record use such as `(get {} :a)` or a result that nothing else
constrains keeps it an empty record. A map value that is `Option<T>` cannot be encoded, because a
`null` value and a missing key would collide. See [Values and JSON](#values-and-json).

A declaration marks a record type open by writing `open` between `Record` and its fields:
`Record open {id: String, name: String}`, as the output of a capability or as a named type. An open
record is for a tool that returns many fields a program does not use. Decoding drops the fields the
type does not name, at that record only, so a plain record inside an open one still rejects extra
fields, and an open record inside a plain one drops only its own. The named fields are required and
checked as for any record. The program sees the open record as an ordinary record with the named
fields: `(get r :other)` is an `unknown-field` error, and a value read through an open type encodes
with the named fields only. Only a declaration can be open. A record literal always has exactly the
fields it writes, so `{:id "a" :name "b" :plan "pro"}` is rejected where an open record with `id` and
`name` is expected, and `open` is not program syntax.

Programs have no type annotations. The checker infers types bidirectionally from literals,
bindings, prelude and core signatures, capability signatures, and the expected type of each
argument position. A lambda parameter gets its type from the position the lambda fills. `none`,
`[]`, `(ok x)`, and `(err e)` take their missing type parameters from the expected type. The
checker rejects an expression whose type it cannot resolve where the operation needs it: reading
a field, comparing, or sorting. A type left unresolved in the program's result, such as the
element type of an empty list, is harmless and allowed.

Equality is structural on every type, and on `Float` it is IEEE equality, so `0.0` equals `-0.0` and
`(= (+ 0.1 0.2) 0.3)` is `false`. Ordering with `<`, `<=`, `>`, and `>=` is defined on `Int`, on
`Float`, and on `String`, where strings compare lexicographically by Unicode code point. Both
operands have the same type. Sort keys, used by `sort-by`, may also be `Float`, `Bool` (`false` first) and tuples or lists of sort keys, compared element
by element. A map is not a sort key. Neither is a core-validated nominal such as `decimal.Decimal`,
because its text does not order like its value: the diagnostic names the core's comparison function,
`decimal.compare`.

## Special forms

| Form | Type rule |
| --- | --- |
| `(let [p1 e1 p2 e2 ...] body)` | Each `e` is visible to later bindings and to `body`. The body is one expression. |
| `(if c then else)` | `c` is `Bool`, or an `Option<T>` presence test. See [Conditions](#conditions). Both branches have the same type. |
| `(cond t1 e1 t2 e2 ... :else e)` | Evaluates `t1`, `t2`, and so on in order, and returns the `e` after the first true test, or the `:else` result. `:else` is required and last. Each test follows the rules of an `if` condition, and all results have the same type. See [Conditions](#conditions). |
| `(match e clause...)` | See [Match](#match). |
| `(fn [p ...] body)` | A function argument, or the value of a `let` binding. See [Functions](#functions). |
| `(get r :field)` | `r` is a record with `field`. A missing field is rejected by the checker. |
| `(get m key)` | `m` is a `Map<String, V>` and `key` is a `String`. The result is `Option<V>`, `none` when the key is absent. A keyword key, as in `(get m :name)`, is rejected with the habit `keyword-value` and the fix `"name"`, because keywords only name record fields. |
| `(get m key default)` | The result is the value when the key is present, otherwise `default`, which evaluates only then. A `default` of type `V` makes the result `V`; an optional `default`, such as `none`, makes it `Option<V>`. |
| `(get r :field default)` | `field` has type `Option<T>`. The result is the value when present, otherwise `default`, which evaluates only then. A `default` of type `T` makes the result `T`; an optional `default`, such as `none`, makes it `Option<T>`. A field whose type is not an `Option` is rejected with `arity`, because it is always present. |
| `(call cap.id arg)` | `arg` has the capability's input type. The result has its output type. |
| `(attempt e)` | `Result<T, CallError>`, where `e` has type `T`. See [Recovery and repetition](#recovery-and-repetition). |
| `(retry {options} e)` | `T`. Re-evaluates `e` after a failure. |
| `(wait-until {options} f e)` | `T`. Re-evaluates `e` until `f` returns `true` for its value. |
| `(sleep d)` | `Unit`. `d` is a duration literal. Waits that long. See [sleep](#sleep). |
| `(loop {:max n} [p1 e1 ...] body)` | The type of `body`'s non-`recur` results. `(recur ...)` repeats `body`. |
| `(par e1 ... en)` | `Tuple<T1, ..., Tn>`. Every branch has only `read` effects. |
| `(par-map {:limit n} f xs)` | `List<U>`. `n` is an integer literal. `f` has only `read` effects. |

A pattern `p` is a symbol, `_`, or a tuple pattern `[p1 ... pn]`. Patterns appear in `let`,
`loop`, and `fn` parameters. `_` evaluates the value and discards it, so
`(let [_ (map send items)] (count items))` runs every send.

The names of the special forms are reserved: `let`, `if`, `cond`, `match`, `fn`, `get`, `call`,
`attempt`, `retry`, `wait-until`, `sleep`, `loop`, `recur`, `par`, `par-map`, `some`, `ok`, and
`err`, and `none`. Binding one of them in `let`, `loop`, `fn` parameters, or a `match` pattern, or
naming a local function with one, is a `reserved-name` error at the binding, with a fix that
suggests another name, such as `attempts` for `attempt`. Without the rule a form that follows the
binding, such as `(attempt ...)` after `[attempt 1]`, would call the local instead of the form.
Prelude and core function names are not reserved, so a local may be named `count`. Inside its
scope the local hides the function: using it in head position, as in `(count xs)`, is a
`type-mismatch` that says the local hides the function.

### Conditions

An `if` condition is a `Bool`, or an `Option<T>` where `T` is not `Bool`. An optional condition is a
presence test: the then-branch runs for `(some x)` and the else-branch for `none`. When the condition
is a bare name bound by `let`, `loop`, `fn`, or `match`, the then-branch sees that name as the inner
value, of type `T`:

```
(let [next (get page :next)]
  (if next (recur next ids) ids))
```

Any other optional expression, such as `(get page :next)`, is a presence test without narrowing.
Narrowing covers only a bare name, never a field path such as `(get x :f)`. An `Option<Bool>`
condition is rejected with `condition-not-bool`, because `(some false)` is present but false, and
the diagnostic shows the `match` to write.

`cond` tests its clauses in order and is the same as nested `if` forms:
`(cond t1 e1 t2 e2 :else e3)` means `(if t1 e1 (if t2 e2 e3))`. Each test follows the rules above,
including presence tests and narrowing a bare name, and the narrowed name is visible only in the
result after its test. A `cond` that does not end with `:else` and a result is rejected with
`missing-else`. An `:else` before the last clause is `unreachable-clause`, and an `:else` with no
result after it is `arity`.

### Functions

A function argument `f` is a `fn` form, the name of a prelude or core function with the right
arity, or the name of a local function.

A local function is a `let` binding whose value is a `fn` form:

```
(let [label (fn [svc state] (str svc ": " state))]
  [(label "api" "ok") (label "web" "failed")])
```

The program calls it like any function and may pass it as a function argument. A local function
cannot call itself, and it cannot be returned, stored in a record or list, or passed to a
capability. The checker types its body at each call with the argument types of that call, so its
parameters need no annotations.

### Match

```
(match checkout
  (OnBranch info) (get info :branch)
  (Detached head) "detached"
  _ "unavailable")
```

The scrutinee's type determines which variant names are in scope, so patterns are unqualified.
`Option` patterns are `(some x)` and `none`. `Result` patterns are `(ok x)` and `(err e)`. `_`
matches anything. A lowercase name matches anything too and binds the whole value to that name:
`other (str "not green: " repo)`. A name follows every rule of `_`. A `match` without `_` or a
name must name every variant. A final `_` or name clause is allowed even when the clauses before it
name every variant. Its result is still checked, but it can never run, so its type does not count
toward the type of the `match`. Any other clause that can never match is rejected. The name binds the value
itself, so it has type `Option<T>` in a match on an optional. Only a final `_` after `none`
narrows.

An integer (including a negative one), string, or boolean literal is a pattern that matches when
the scrutinee equals it. A `Float` literal is never a pattern, because computed floats are rarely
equal: it is a `bad-pattern` that points to `=` and `<`. `(match n 1 "one" 2 "two" _ "many")`. The scrutinee's type must be the
literal's type, `Int`, `String`, or `Bool`. Any other scrutinee, such as an `Option<Int>` or an
enum, makes the literal a `bad-pattern`, and for a string that names a variant of the enum the fix
is the variant name. `true` and `false` together cover `Bool`, so a `match` with both needs no `_`.
No other set of literals covers its type, so a `match` on an `Int` or `String` needs a final `_` or
name, or it is `non-exhaustive`. A literal repeated in a later clause is `unreachable-clause`. A
literal is also allowed inside `(some ...)`, `(ok ...)`, `(err ...)`, and `(Variant ...)`, where it
never covers the case, so `(some 1)` needs a `(some x)` or `_` clause after it.

When the scrutinee is a bare name of type `Option<T>` and a `none` clause comes before a final `_`
clause, the `_` clause sees that name as the inner value, of type `T`:

```
(match acc none v _ (semver.compare v acc))
```

## Recovery and repetition

`attempt`, `retry`, and `wait-until` follow xsh's `try`, `retry`, and `wait until`. Each form
does one thing. `retry`, `wait-until`, and `loop` repeat, and `attempt` captures. All options are
literals, so the checker knows the largest number of repetitions before the program runs.

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
`retry` around a non-idempotent write with `retry-write`, with one exception: a non-idempotent
write may be retried when `:on` is given and every kind in it is a refusal kind, `RateLimited`,
`NotFound`, `PermissionDenied`, `Invalid`, or `Conflict`, because a refusal means the service did
not perform the operation. `Timeout`, `Unavailable`, and `Failed` leave the outcome unknown, so a
retry that includes any of them, including the default kinds, stays `retry-write`. The message
names those kinds, and when refusal kinds are among those retried it shows the `:on` list that
would be allowed, such as `:on [RateLimited]`.

A repeat after a refusal is safe only if the write has not already taken effect in an earlier
attempt. So the exception holds only when the write is the last call in `e` in source order and
does not sit inside a `fn` or `loop` body within `e`. Otherwise a later call, or a later run of the
same call, could be refused after the write took effect, and the retry would repeat it. That is
also `retry-write`. Put the `retry` around the single write, inside the `fn` if there is one.

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

### sleep

```
(let [_ (sleep 10s)
      job (call jobs.status {:id id})]
  job)
```

`(sleep d)` waits for `d` and returns `Unit`. `d` must be a duration literal, like every other
duration, so `sx explain` can show it. Bind the result to `_` to run it as a step. `sleep` issues
the same [delay request](#delays) as a `retry` delay or a `wait-until` interval, adds its duration
to the `delay` budget, and works inside `par` branches and `par-map` functions, where each fiber
waits on its own. A `sleep` of `0ms` issues no request. To poll until a condition holds, use
`wait-until`; `sleep` waits once.

### loop

```
(loop {:max 50} [cursor none
                 events []]
  (let [page (call events.page {:cursor cursor})
        all (concat events (get page :items))]
    (match (get page :next)
      (some next) (recur next all)
      none all)))
```

`loop` binds its names like `let`, then evaluates `body`. `(recur e1 ... en)` evaluates its
arguments, rebinds the names to them in order, and evaluates `body` again. Any other result of
`body` is the result of the loop.

- `recur` must be in tail position of the innermost `loop` body, and never inside a `fn`. The
  checker rejects any other `recur` with `recur-position`.
- `recur` takes exactly one argument per binding, with that binding's type, or the checker
  rejects it with `recur-arity` or `type-mismatch`.
- A binding's type comes from its initial value and from the matching `recur` arguments, so
  `none` and `[]` work as initial values.
- Some path through `body` must end without `recur`. A body that always ends in `recur` has no
  result type and can only fail with `loop-limit`, so the checker rejects it with `loop-no-result`.
- `:max` is an integer literal of at least 1 and counts evaluations of `body`. If evaluation
  number `:max` ends in `recur`, the loop fails with `loop-limit`. Choose `:max` as the largest
  count the task can need, not as a timeout.
- The body may call capabilities. Inside a `par` branch or `par-map` function, the usual
  read-only rule applies.

### Delays

A delay is a request like a capability call, issued for a `retry` delay, a `wait-until` interval, or a `sleep`. The machine suspends with a delay request carrying
its ID and duration, and the host resumes it after waiting. The machine never reads a clock.
Fixture hosts resume at once, so tests run in no time and stay deterministic. The trace records
each delay.

## Prelude

Prelude functions are unqualified, and the kernel defines them. Function arguments come first,
as in Clojure.

| Function | Type |
| --- | --- |
| `+ - *` | `Int Int -> Int` or `Float Float -> Float`, never one of each. `(- x)` negates. |
| `/` | `Float Float -> Float`. On two `Int` arguments it is a `type-mismatch` whose fix is `quot`. |
| `quot rem` | `Int Int -> Int`, truncating toward zero |
| `float` | `Int -> Float`, the nearest `Float`. Integers beyond 2^53 may round. |
| `round` | `Float -> Int`, the nearest integer, with halves going away from zero: `(round 2.5)` is `3` and `(round -2.5)` is `-3` |
| `floor ceil` | `Float -> Int`, the largest integer not above, and the smallest not below |
| `= not=` | `T T -> Bool` |
| `< <= > >=` | `T T -> Bool` for `Int`, `Float`, or `String` |
| `not` | `Bool -> Bool` |
| `and or` | `Bool Bool -> Bool`. Both arguments always evaluate. Use `if` to make a call conditional. |
| `str` | `T ... -> String`, one or more arguments joined in order. Each is a `String`, an `Int` written in decimal, a `Float` written as JavaScript's `String(x)` writes a number, a `Bool` written `true` or `false`, or a value of an enum with no payloads written as its variant name. In a `sort-by` key every argument must be a `String`, because joining numbers into a string key sorts them as text. |
| `count` | `List<T> -> Int`, `Map<String, V> -> Int` for the number of entries, or `Bytes -> Int` for the number of bytes |
| `first` | `List<T> -> T`. An empty list is an `out-of-range` error. On a tuple it is the first element, with that element's type. |
| `rest` | `List<T> -> List<T>`, every element after the first. Empty for an empty list. |
| `nth` | `List<T> Int -> T`, counting from 0. A missing index is an `out-of-range` error. On a tuple the index must be an integer literal within the tuple, checked statically, and the result has that element's type. |
| `take` | `Int List<T> -> List<T>`. A negative count is an arithmetic error. |
| `concat` | `List<T> List<T> -> List<T>` |
| `flatten` | `List<List<T>> -> List<T>` |
| `contains?` | `List<T> T -> Bool`, or `Map<String, V> String -> Bool` to test for a key |
| `assoc` | `Map<String, V> String V -> Map<String, V>`, the map with the key set, replacing any earlier value |
| `dissoc` | `Map<String, V> String -> Map<String, V>`, the map without the key. An absent key is not an error. |
| `keys` | `Map<String, V> -> List<String>`, ascending |
| `vals` | `Map<String, V> -> List<V>`, in key order |
| `entries` | `Map<String, V> -> List<Tuple<String, V>>`, in key order |
| `into-map` | `List<Tuple<String, V>> -> Map<String, V>`. A later pair replaces an earlier pair with the same key. It takes the output of `group-by` when the key is a `String`. |
| `identity` | `T -> T`, for use as a function argument such as `(group-by identity xs)` |
| `map` | `(T -> U) List<T> -> List<U>` |
| `filter` | `(T -> Bool) List<T> -> List<T>` |
| `keep` | `(T -> Option<U>) List<T> -> List<U>`, the `some` values in order |
| `any? all?` | `(T -> Bool) List<T> -> Bool`. Each stops at the first element that decides the answer. |
| `reduce` | `(A T -> A) A List<T> -> A` |
| `sort-by` | `(T -> K) List<T> -> List<T>`, stable and ascending. `K` is a [sort key](#types). |
| `distinct-by` | `(T -> K) List<T> -> List<T>`, keeps the first element for each key |
| `group-by` | `(T -> K) List<T> -> List<Tuple<K, List<T>>>`, groups in first-occurrence order |

Integer overflow, `quot` or `rem` by zero, and a negative `take` count end evaluation with an
`arithmetic` error. So does a `Float` operation whose result is infinite or not a number: `/` by
zero (`0.0` divided by `0.0` included) and an overflowing `+`, `-`, `*`, or `/`. `round`, `floor`, and
`ceil` of a value outside the `Int` range are `out-of-range` errors. Nothing wraps or saturates.

`str` writes a `Float` as JavaScript's `String(x)` does: the shortest digits that read back to the
same value, with no trailing `.0`, so `1.0` is `1`. It uses plain notation for magnitudes from
`1e-6` up to below `1e21` and an exponent outside that range, as in `1e+21` and `1e-7`. Negative
zero is `0`.

Text operations other than `str`, `=`, and ordering live in the `text` core, for example
`text.casefold` and `text.from-int`.

## Effects and authorization

Each capability declares an ID, an input type, an output type, and an effect class of `read` or
`write`. A `write` capability also declares whether it is `idempotent`, meaning that repeating a
call with the same input has the same effect as making it once. Effects are inferred from the program text. Without dynamic dispatch, the set of
capability IDs a program can call is exactly the set of IDs named in its `call` forms.

The host grants authority for each execution, and a program never names its own. The checker
rejects a program that names any capability the host has not granted, with `unauthorized`. A
rejected program produces no capability request. `sx explain` prints a program's capability set,
effect classes, and call sites without running it.

`par` branches and `par-map` functions must have only `read` effects. Read effects in parallel
branches do not observe one consistent snapshot of the outside world. Determinism holds only for
fixed responses.

Pure core functions have no effects. The registry rejects a core whose signature declares one.

### Grants and scopes

A capability may declare a scope type, a record in the declaration type language, such as
`Record {roots: List<String>}` for files. An execution's authority is a set of grants. Each grant maps
a capability ID to a scope value of that type, or to no scope for a capability that declares none.
The granted IDs are the allow-list. Granting a scoped capability without a scope is a usage error,
never an implicit "everything", and so is a scope for a capability that declares none, or a scope
value that does not decode to the declared type.

The kernel never interprets a scope. The pack that implements a scoped capability registers two
functions. One prepares a scope when the grant is loaded, for example by canonicalizing directories,
and its error is a usage error. A pack may also register a resolver that rewrites the written scope before it decodes, which the http pack uses to read `{"env": NAME}`, and a function that shows a granted scope in a diagnostic with its secrets hidden. The other is the scope check, a function from the prepared scope and
a request's input to allowed, or denied with a reason. The check runs in two places:

- **Before the run.** When a call's input is known before the run, the checker applies the check and
  rejects a denied call with `out-of-scope`. An input is known when it is built only from literals:
  a `Bool`, `Int`, `Float`, or `String` literal, `none`, `(some x)` of a known `x`, or a record or
  list literal of known values, including a record that omits optional fields, which are `none`. Any
  other input, including a name bound to a literal, is computed, and only the run-time check sees it.
- **During the run.** The host applies the check to every request before performing it, and refuses
  a denied one with a `CallError` of kind `PermissionDenied` whose message is the reason. That is a
  refusal kind, so the operation never took effect and the retry rules treat it as not performed.

A capability declared in a declarations file may name its scope type with `"scope"`, except an HTTP endpoint, whose scope and check come with it (see [the http pack](#the-http-pack)). For any other, no check
comes with the scope: the kernel passes it to the host, which must check each request itself. The
checker cannot judge such a call, and `explain` reports it as checked per request.

`sx explain` lists every call site under its capability. For a capability that declares a scope it
adds the verdict: `allowed` or `denied: REASON` for a known input, `checked per request` for a computed
one, and `not granted` when the host granted nothing. `--format json` gives each tool a `verdicts`
list in the order of its `inputs`, each `{"verdict": "allowed"}`, `{"verdict": "denied", "reason": TEXT}`,
`{"verdict": "checked per request"}`, or `{"verdict": "not granted"}`. A tool without a scope has no
`verdicts` field. `explain` checks the program against every declared capability and never rejects it
for a scope.

`sx check`, `sx explain`, and `sx run` take `--grants FILE`, a JSON object from capability ID to scope
value or `null`, and `--allow ID` for a capability without a scope. Either may name a capability; the
grants are their union.

### The fs pack

A declarations file registers a pack by listing it under `"packs"`, as it does cores under `"cores"`.
The `fs` pack declares these capabilities, each with the scope type `Record {roots: List<String>}`:

| Capability | Input | Output | Effect |
| --- | --- | --- | --- |
| `fs.read-text` | `{path: String}` | `String` | read |
| `fs.read-bytes` | `{path: String}` | `Bytes` | read |
| `fs.list` | `{path: String}` | `List<fs.Entry>` | read |
| `fs.stat` | `{path: String}` | `Option<fs.Stat>` | read |
| `fs.write-text` | `{path: String, text: String}` | `Unit` | write, idempotent |

`fs.Kind` is the enum `File | Directory | Symlink | Other`. `fs.Entry` is `Record {name: String, kind: fs.Kind}`,
and `fs.list` returns entries sorted by name, without following symlinks, so a link is `Symlink`.
`fs.Stat` is `Record {kind: fs.Kind, size: Int}`. `fs.stat` follows symlinks, as path resolution does,
and returns `none` when nothing is at the path. `fs.read-text` fails with `Invalid` when the file is
not UTF-8. A read of anything but a regular file is `Invalid`. `fs.write-text` replaces the file with
a temporary file in the same directory: it writes, syncs, renames, and syncs the directory, keeping
an existing file's mode. A reader sees the old contents or the new, never a mix.

Every root must be an absolute path to an existing directory. Granting loads each root and
canonicalizes it, following symlinks, so a symlinked root grants its target, and a missing root, a
root that is not a directory, and an empty root list are usage errors.

A request path passes the scope check by these rules:

1. It is absolute and contains no NUL character. Empty segments, as in `//`, are ignored.
2. It is resolved by canonicalizing its longest existing prefix, following every symlink, and
   appending the rest. `.` and `..` in the existing part resolve on the file system. In the rest,
   which does not exist yet, they are refused. A prefix that ends in a symlink whose target does not
   exist is refused, and so is one that cannot be resolved, such as a symlink loop.
3. The resolved path must lie inside a canonical root, comparing whole path components, never string
   prefixes: `/data-old` is not inside `/data`.
4. For `fs.write-text`, the parent directory is resolved by the same rules and must lie inside a root,
   so a root itself is never written. The final component must not be `.` or `..`, and must not be an
   existing symlink, even one that points inside a root.

On a case-insensitive file system, such as the default APFS on macOS, canonicalizing returns each
existing component with its stored case, so `/Data/notes` and `/data/notes` resolve to the same path,
and the containment test compares that path. APFS also refuses a name that is not valid UTF-8, so such
a file cannot exist there. On Linux it can, and `fs.list` of a directory holding one fails with
`Invalid`, because a `String` cannot hold the name.

Operating-system failures map to call error kinds by a closed table: not found is `NotFound`;
permission denied is `PermissionDenied`; is a directory, not a directory, directory not empty,
invalid argument, name too long, and a symlink met where none is followed are `Invalid`; anything
else is `Failed`. A `fs.write-text` that renamed its file but could not sync the directory fails with
`Failed`, because the write took effect.

A scope protects against the program, not against another local process. The check resolves a path,
and the operation then uses the resolved path, so a process that rewrites the tree between the two
can redirect it. The pack narrows the window where the standard library allows: it opens the final
component with `O_NOFOLLOW`, so a symlink swapped in there fails instead of being followed, opens
reads with `O_NONBLOCK` and refuses anything but a regular file, so a FIFO cannot block it, and
creates the temporary file with `O_EXCL`. An intermediate directory swapped for a symlink is not
detected. A denied request is never opened, read, written, or created, but resolving its path reads
the metadata of the path's existing prefix.

`sx run FILE --grants FILE` without `--responses` performs granted pack capabilities through a
native host, which drives the machine, performs each suspension's requests concurrently, so never
more than the `concurrency` budget, and waits for real on delays. A program that calls a capability
no pack implements is a usage error before the run starts. With `--responses`, `sx run` answers from
the recorded responses as before, after refusing out-of-scope requests the same way.

### The process pack

A declarations file registers it with `"packs": ["process"]`. It declares one capability:

| Capability | Input | Output | Effect |
| --- | --- | --- | --- |
| `process.run` | `{program: String, args: List<String>, cwd: String, stdin: Option<String>}` | `process.Output` | write, not idempotent |

`process.Output` is `Record {status: Int, stdout: String, stderr: String}`. `status` is the exit code,
and a nonzero code is a value, not an error, so a program branches on it. Sx never offers a capability
that runs an arbitrary command: a host grants specific programs with specific argument patterns, and
a grant of nothing runs nothing. `process.run` executes the program directly with exactly the given
arguments. No shell reads them, so `$HOME`, `*`, `;`, and backquotes reach the program as they are.

**Scope.** The scope type is `Record {commands: List<process.Command>, roots: List<String>}`.
`process.Command` is `Record {program: String, args: List<process.Arg>}`, and `process.Arg` is an
enum with payloads, which the grants JSON writes as the kernel [encodes](#values-and-json) every
enum: a variant with a value is `{"tag": NAME, "value": x}`, and one without is its name as a string.

| Pattern | JSON | Matches |
| --- | --- | --- |
| `(Exact s)` | `{"tag": "Exact", "value": "s"}` | one argument equal to `s` |
| `(OneOf [s ...])` | `{"tag": "OneOf", "value": ["s", ...]}` | one argument equal to any of the strings |
| `(AnyOne)` | `"AnyOne"` | any single argument, including an empty one |
| `(Rest)` | `"Rest"` | all remaining arguments, including none |

```json
{"process.run": {
  "commands": [
    {"program": "/usr/bin/git", "args": [{"tag": "Exact", "value": "status"}, {"tag": "OneOf", "value": ["--short", "--porcelain"]}]},
    {"program": "/bin/echo", "args": ["Rest"]}
  ],
  "roots": ["/srv/work"]
}}
```

A request is allowed when some command lists the request's program and its patterns match the
request's arguments by position: each pattern consumes one argument, `(Rest)` consumes the remainder,
and without `(Rest)` the counts must be equal. A program may appear in any number of commands, and
any one of them may allow the request. A command with no patterns allows no arguments. Loading a grant
refuses a `(Rest)` that is not last, a `(OneOf [])`, which matches nothing, a NUL character in a
pattern, an empty `commands` or `roots` list, and any `roots` problem the `fs` pack refuses.

A program must be an absolute path to an executable regular file. Granting canonicalizes it,
following symlinks. A request's `program` must also be absolute, with no NUL character, and it is
canonicalized the same way and compared with the granted programs as a whole path, so `echo` and
`./echo` are denied, `PATH` is never searched, and a symlink to a program that is not granted is
denied, while one to a granted program is allowed. The program is then started through its canonical
path with the request's path as `argv[0]`, so a multi-call binary such as busybox still sees the name
it was called by. A grant is on the file, not the name: granting `/bin/echo` on a system where it is a
link to busybox grants every name that links to busybox. A request argument with a NUL character is
denied. `cwd` must resolve inside one of the scope's `roots` by the [fs pack's rules](#the-fs-pack):
absolute, symlinks followed, whole-component containment. A `cwd` that does not exist fails with
`NotFound`, and one that is not a directory with `Invalid`, before anything starts.

**Execution.** The child gets an empty environment: no `PATH`, `HOME`, `LANG`, or anything inherited
from the host. A shell started this way chooses its own default `PATH`. Standard input is the
`stdin` text when it is `some`, or an empty input (the null device) when it is `none`. A program that
exits without reading all of its input is not an error. Standard output and standard error are
captured separately, at most 1 MiB (1,048,576 bytes) each, and the whole request has 60 seconds.
The child runs in a new process group, and the host kills the group with `SIGKILL` on every way out:
a normal exit, which also ends any background process the program left behind, an exceeded limit, and
a failure. A request therefore returns only after it has killed the group and reaped the process it
started. A process that
escapes the group, for example with `setsid`, is not killed. If it holds the output open past the
timeout, the request fails with `Timeout`.

**Errors.** The mapping is closed. A nonzero exit status is not an error.

| Situation | Kind | Took effect |
| --- | --- | --- |
| The scope check denies the request, including a program that does not exist | `PermissionDenied` | no |
| `cwd` does not exist | `NotFound` | no |
| `cwd` is not a directory | `Invalid` | no |
| The program or its interpreter cannot be found when it starts | `NotFound` | no |
| The program cannot be executed | `PermissionDenied` | no |
| Any other failure to start | `Failed` | no |
| The 60 seconds pass; the group is killed | `Timeout` | yes |
| Output past 1 MiB on either stream; the group is killed | `Failed` | yes |
| The program was killed by a signal, named in the message | `Failed` | yes |
| The program wrote invalid UTF-8 to either stream | `Failed` | yes |

The last four rows are not refusal kinds, because the program ran. Invalid UTF-8 in particular is
`Failed`, not `Invalid`: the checker allows a retry of this write on refusal kinds only, and an
`Invalid` would let a retry run the program again.

**What the scope does not do.** It decides which program starts, with which arguments, and from
where. It does not confine the program. A granted program runs as the host's user and can read or
write any file that user can, whatever `roots` says. An argument pattern that lets a program run
other programs, such as `(Rest)` after `sh -c` or an interpreter's script argument, grants everything
that program can reach. Grant the narrowest patterns that do the job. As in the `fs` pack, the check
resolves paths and the start then uses the resolved path, so a process that rewrites the tree between
the two can redirect it.

### The http pack

A declarations file registers it with `"packs": ["http"]`. It declares two capabilities, each with the scope
type `Record {origins: List<String>, methods: List<String>, headers: Map<String, Map<String, String>>}`:

| Capability | Input | Output | Effect |
| --- | --- | --- | --- |
| `http.request` | `{method: String, url: String, headers: Map<String, String>, body: Option<String>}` | `http.Response` | write, not idempotent, because the method is data |
| `http.get` | `{url: String, headers: Map<String, String>}` | `http.Response` | read |

`http.Response` is `Record {status: Int, headers: Map<String, String>, body: String}`. Header names are
lowercase, and a header the server sent more than once is joined with `, ` in arrival order (a joined
`set-cookie` is therefore ambiguous, and the pack does not keep cookies). The body must be UTF-8. Any
status is a value: raw requests fail only when no response could be read, and a 404 or 500 comes back
as a `Response` for the program to match on.

**Scope.** `origins` lists `scheme://host[:port]` strings, and `methods` lists HTTP method tokens. Granting
normalizes each origin, and a malformed one, an empty list, or a method that is not an uppercase token
is a usage error:

- The scheme is `http` or `https`, lowercased. Anything else is refused.
- The host is lowercased. A label that is not ASCII is lowercased and encoded as punycode (RFC 3492)
  behind `xn--`, so `https://bücher.example` grants `https://xn--bcher-kva.example`. This is not full
  UTS 46: there is no Unicode normalization and no mapping of look-alike characters, so a spelling that
  needs either names a different host and is denied, which fails safe. A host that ends in a dot, has an empty
  label, holds a `%`, or has a character a host name never has is refused.
- A host whose last label is all digits, or begins `0x`, must be a strict dotted-quad IPv4 address, so
  `127.1`, `0x7f.1`, and `2130706433` are refused rather than guessed at. An IPv6 literal is written in
  brackets and printed in its canonical form.
- The port defaults to 80 for `http` and 443 for `https`, and the normalized origin always writes it, so
  `https://a.example` and `https://a.example:443` are one origin. An empty port, `0`, and a port above
  65535 are refused.
- An origin has no userinfo, path, query, or fragment, not even a trailing `/`.

**Credentials.** `headers` is optional in a grants file and maps an origin to the headers the host adds to
every request sent to that origin, such as `{"https://api.example.com": {"authorization": "Bearer ..."}}`.
The program never holds them:

- A value may be written `{"env": "NAME"}`, which reads the host's environment variable when the grant
  loads. A variable that is not set, not UTF-8, or named with `=` or NUL is a usage error, and the error names
  the variable, never a value. `sx serve` reads its own environment when the host's `grants` arrive.
- Each origin is normalized as in `origins` and must be one of them, and an origin may appear once. A
  header name follows the client's header rules above, is lowercased, and appears once per origin. A value
  must be a non-empty valid header value. Any violation is a usage error that does not echo a value.
- The header is added when the request is sent, to the first request and to every redirect hop, according
  to the origin of that hop. It is in no request value, so it is in no trace, `explain` output, `check`
  diagnostic (a diagnostic shows a granted scope with every value as `[hidden]`), or error message. A redirect
  to another granted origin carries that origin's headers and none of the program's, and a redirect that
  leaves the grant is not followed.
- A request whose headers include one the grant injects for its origin is denied, whichever way the
  name is cased: `PermissionDenied` at run time with a reason that names the header and no value. For a typed
  endpoint, the declared `headers` take the place of the program's, and a declared header the grant also
  injects (an endpoint with a JSON body sets `content-type`) is `out-of-scope` before the run when the endpoint
  is called with a literal input. A raw call's `headers` map is never a literal, so only the run-time denial
  covers it. A program therefore cannot override a credential, and the denial confirms that a credential
  exists for the header name the program tried, and nothing more.
- A response that echoes an injected value back, in its body or its headers, has that value replaced
  with `[redacted]` before the program or an error message sees it, as does the part of a value after its
  first space of at least 8 bytes, so `Bearer TOKEN` is hidden whole and as `TOKEN`. This catches a server
  that returns the request's headers, and not one that transforms them (base64, JSON escaping).

A request is allowed only when its URL's normalized origin is in `origins` and its method, compared
exactly and case-sensitively (`get` is not `GET`), is in `methods`. The URL is parsed by the same rules
and then refused if it carries userinfo (`user@`, and so `a.example@evil.example`), a backslash, a space
or control character, a path or query that is not ASCII or holds a `%` not followed by two hexadecimal
digits, or a scheme other than `http` and `https`. The fragment is dropped. The pack connects to the
normalized origin it checked, never to the text it was given. A denied request opens no socket.

**Request.** The client is h12tiny's blocking HTTP/1.1 client, with HTTPS through Rustls. HTTP/2 is not
used. It sends `user-agent: sx-packs/VERSION` unless the program sets one. The headers `host`,
`content-length`, `transfer-encoding`, `connection`, `upgrade`, `te`, and `trailer` belong to the client,
and a request that sets one, repeats a name that differs only in case, or names an invalid header is
`Invalid` before any socket opens. The client uses no proxy, cookie jar, or connection reuse. HTTPS
verifies against the operating-system trust store, as h12tiny documents, and on macOS `SSL_CERT_FILE` or
`SSL_CERT_DIR` selects the roots instead.

**Redirects.** A redirect is a new request. The pack follows `301`, `302`, `303`, `307`, and `308` with
one `Location` only when the target, resolved against the current URL, passes the same grant with the
method the next request will use, and at most 5 times. Otherwise it returns the redirect response as is,
status, `location`, and body, which is the safer default: a program sees where it was sent and decides.
`303` becomes a `GET` (or stays `HEAD`) without a body; `301` and `302` are followed only for `GET` and
`HEAD`, so a write is never silently turned into a read; `307` and `308` repeat the method and body. A
redirect to another origin carries none of the program's headers, so an `authorization` header never
leaves the origin it was sent to; a redirect inside the origin keeps them. A target that is relative,
malformed, carries userinfo, or is not `http` or `https`, or whose origin or method the grant does not
cover, is not followed.

**Typed endpoints.** A declarations file may declare an HTTP endpoint as an ordinary capability by giving
a `capabilities` entry an `http` object in place of a scope:

```json
{"id": "github.repo",
 "http": {"method": "GET", "url": "https://api.github.com/repos/{owner}/{name}", "query": [],
          "headers": {"accept": "application/vnd.github+json"}},
 "input": "Record {owner: String, name: String}",
 "output": "Record open {full_name: String, stargazers_count: Int}",
 "effect": "read"}
```

`id`, `http`, `input`, `output`, and `effect` are required, and `description` and `idempotent` behave as for
any capability (the default description is the method and URL template). An endpoint's scope type is the
one above, implied, and it is granted by the endpoint's own ID, so a grant file lists `github.repo`
with `origins` and `methods` like any other scoped capability. The `http` object has `method`, `url`, and
optional `query` and `headers`; an unknown key is an error. Every mistake below is a declaration error
that names the endpoint, found when the declarations load:

- The input is a record. `{field}` in the URL path names an input field of type `String`, `Int`, or `Bool`
  and must exist. Nothing but the path may hold a variable: the scheme, host, and port are literal, so the
  checker knows the endpoint's origin. The template has no query or fragment.
- `query` lists input fields sent as query parameters of the same name. A field may be `String`, `Int`,
  or `Bool`, an `Option` of one, which is omitted when `none`, or a `List` of one, which repeats the parameter.
- The remaining input fields are the JSON body, encoded as a record, for `POST`, `PUT`, and `PATCH`, with
  `content-type: application/json` unless the endpoint sets one; when there are none, there is no body.
  For any other method a field left over is an error, because the request would drop it.
- A field is not used twice, and `headers` are static strings that obey the client's header rules above.

A path or query value is percent-encoded except for letters, digits, `-`, `.`, `_`, and `~`, so a
variable never adds a segment or a parameter. An empty path value, `.`, and `..` are `Invalid`. A
response with a `2xx` status is read as JSON and decoded against the declared output with the type-directed
decoder of [Values and JSON](#values-and-json), so an open record drops fields it does not declare, and a
closed record, a missing field, or a wrong type is a failure that names the path, such as
`$.list[1].n: expected Int, found a string`. An output of `Unit` ignores the body. A decode failure is
`Invalid` for a `read` endpoint. For a `write` endpoint it is `Failed`, because `Invalid` is a refusal kind
and the server already acted. The same rule makes a non-UTF-8 raw response `Invalid` for `GET`, `HEAD`,
`OPTIONS`, and `TRACE` and `Failed` for every other method.

The scope check for an endpoint compares its fixed origin and method with the grant, so a literal call to
an endpoint the grant does not cover is rejected by `check` with `out-of-scope`. It is the same function
the host applies to each request, and the pack applies it again before it opens a socket. Redirects of an
endpoint's request follow the rules above under the endpoint's own grant. A raw call whose input
passes a `headers` map is not a literal to the checker, because the checker reads records and lists
but not maps, so `explain` reports it as checked per request. An endpoint with no `Map` in its input is
checked before the run.

`sx serve` binds endpoint declarations too, so its checker and `check-scope` judge them; the host that
answers the requests must perform them under the rules here. A loader that binds no endpoints, which is
every other embedding of the declaration loader, refuses an entry with an `http` key.

**Errors.** One closed table maps every failure to a call error kind:

| Failure | Kind |
| --- | --- |
| The request is denied by the grant | `PermissionDenied` |
| A header is invalid or reserved | `Invalid` |
| A header the grant injects for the origin is set by the request | `PermissionDenied` |
| Connection refused, unreachable, or reset while connecting; DNS failure | `Unavailable` |
| A connect, write, header, or body read, or the whole request, exceeds the timeout | `Timeout` |
| A TLS handshake or certificate failure | `Failed` |
| A failed send, a malformed or truncated response, or a body larger than the cap | `Failed` |
| A raw response body that is not UTF-8 | `Invalid` for a safe method, else `Failed` |

For a typed endpoint the response status then decides, checked before the body is read, so a large error
page does not turn a `404` into `Failed`:

| Status | Kind |
| --- | --- |
| `2xx` | the decoded output |
| `400`, `422` | `Invalid` |
| `401`, `403` | `PermissionDenied` |
| `404` | `NotFound` |
| `409`, `412` | `Conflict` |
| `429` | `RateLimited` |
| `501` | `Failed` |
| every other `5xx` | `Unavailable` |
| anything else, including a redirect the pack did not follow | `Failed` |

`RateLimited` is a refusal kind, so the pack may report it only when the server did not perform the
request, and `429` means that by HTTP semantics. `503` does not, since a server may fail after acting,
which is why it is `Unavailable` and a write that is not idempotent is never retried on it. The same holds
for `500`, `502`, and `504`. The message names the URL without its query, so a token in a query
never reaches a trace, plus the status and up to 200 characters of the response body.

**Limits.** One 30 second deadline covers a request from its first connection through every redirect to
the last byte of the body. Each connection attempt also gets 10 seconds, but resolving a host name cannot be
interrupted, so a stalled resolver can run past the deadline. The response head is capped at 64 KiB by the
client, and the body at 8 MiB; a response over the cap fails without being returned. The `delay_ms`
budget does not bound a request.

**What the scope does not do.** It names origins, not addresses: a granted name can resolve to any
address, a private one included, so grant an origin only for a name you trust, and the pack does not
defend against DNS rebinding. It does not look at the program's headers, paths, or bodies, except to refuse the headers the grant
injects, and it cannot stop a granted method from doing harm on a granted origin. A granted origin receives
its credential on every request, including any request the program can build with a granted method, so grant a
credential only to an origin and methods the program may use it for.

### The sqlite pack

A declarations file registers it with `"packs": ["sqlite"]`. The pack bundles SQLite (rusqlite 0.41 with
SQLite 3.54.1 compiled in, no system library) and has the scope type `Record {roots: List<String>}` of
[the fs pack](#the-fs-pack). A database path follows the fs path rules: absolute, no NUL, resolved
through symlinks, and inside a root, with the roots canonicalized when the grant loads. A statement that
reads resolves the path as `fs.read-text` does, so a symlink inside the roots that leads inside them is
followed. A statement that writes resolves it as `fs.write-text` does: the directory must be inside a
root, and a database file that is itself a symlink is refused, so a link can never carry a write out of
the grant. SQLite is also asked not to follow a final symlink. The residual race between the check and
the open is the fs pack's.

**`sqlite.execute`** takes `{database: String, sql: String}` and returns `Unit`. It is a `write` that is
not idempotent. It runs one or more statements, such as `CREATE TABLE` and `CREATE INDEX`, in a single
transaction, so a failing statement undoes the earlier ones, and it creates the database file when it is
missing. A statement that has parameters or returns rows is a failure, since this capability binds and
returns nothing. A literal `database` outside the grant is rejected by `check` with `out-of-scope`.

`sx serve` binds statement declarations as it binds endpoints, so its checker and `check-scope` judge them, and the
host that answers the requests must perform them under the rules here. A loader that binds neither refuses an entry
with an `sqlite` key.

**Typed statements.** A declarations file declares a statement as an ordinary capability by giving a
`capabilities` entry a `sqlite` object:

```json
{"id": "store.add-repo",
 "sqlite": {"database": "/data/store.db",
            "statement": "INSERT INTO repo(name, stars) VALUES (:name, :stars)"},
 "input": "Record {name: String, stars: Int}",
 "output": "Int",
 "effect": "write",
 "idempotent": false}
```

`id`, `sqlite`, `input`, `output`, and `effect` are required, and `description` and `idempotent` behave as
for any capability (the default description is the statement text). The scope type is implied and the
statement is granted by its own ID, like an endpoint. The `sqlite` object has `database`, an absolute
path, and `statement`, one SQL statement; an unknown key is an error. A mistake below is a declaration
error that names the statement, found when the declarations load, so it needs no database to exist:

- The input is a record. Every `:name` in the SQL, found outside string literals, quoted identifiers, and
  comments, is a parameter that binds the input field of the same name, and every input field must be
  the name of a parameter. A parameter may repeat. `?`, `?NNN`, `@name`, and `$name` are refused so that
  nothing binds by position or by a second spelling.
- An input field is a `String`, `Int`, `Float`, `Bool`, `Bytes`, or an `Option` of one, which binds
  `NULL` when `none`. A `Bool` binds `0` or `1`. A value is bound, never spliced into the SQL.
- The output is `Int` or `List<Record {...}>`. An `Int` is the number of rows the statement changed
  (`sqlite3_changes`, so rows changed by triggers do not count), which requires `effect: write`. A list
  is the rows of a statement that returns columns: a `SELECT`, or a write with `RETURNING`. Each
  declared field is typed like an input field.

**Rows.** A row decodes into the record by column name, in the record's field order. SQLite is
dynamically typed, so the pack is strict: `String` reads `TEXT` that is UTF-8, `Int` reads `INTEGER`,
`Float` reads `REAL` and also an `INTEGER` of magnitude at most 2^53, `Bool` reads `INTEGER` `0` or `1`,
and `Bytes` reads `BLOB`. `NULL` is `none` for an `Option` column and a failure for any other. A column
the record does not declare is a failure unless the record is open (`Record open {...}`), which drops it;
a declared field with no column, a column name that appears twice, a type mismatch, and a non-finite
`REAL` are failures that name the row and column, such as `row 2, column 'stars': expected Int, found
TEXT`. The same holds when the statement's shape disagrees with its output. A result of more than
100,000 rows is a failure, not a truncation; add a `LIMIT`.

**Transactions and connections.** Each call opens its own connection and runs in one transaction, `BEGIN
IMMEDIATE` for a `write` and `BEGIN` for a `read`, and commits only when the output decoded. Any failure
rolls the whole call back, including a write whose `RETURNING` rows did not decode. A `read` opens the
database read-only, never creates it, and is refused when SQLite reports that its statement would
change the database. Foreign keys are enforced. `ATTACH` is disabled, so a program cannot reach a
database outside the grant, and `VACUUM INTO` fails because it cannot run inside a transaction. The
schema is untrusted, extension loading is off, and defensive mode is on. A call waits up to 5 seconds
for a lock.

**Errors.** One closed table maps every failure to a call error kind:

| Failure | Kind |
| --- | --- |
| The database is outside the grant, SQLite reports a read-only database or a denied operation | `PermissionDenied` |
| A constraint fails (unique, primary key, not null, check, foreign key), including at commit | `Conflict` |
| The database is busy or locked past the timeout | `Unavailable` |
| A read of a database that does not exist, or a write whose directory does not exist | `NotFound` |
| A path that is a directory | `Invalid` |
| A syntax error, an unknown table or column, SQLite's type mismatch or too-big errors, a statement that changes the database declared as a read, or a row that does not decode, in a `read` | `Invalid` |
| The same failures in a `write` | `Failed` |
| An input that does not match its type, a file that is not a database, an I/O error, a full disk, more rows than the cap, any other SQLite error | `Failed` |

`Conflict`, `PermissionDenied`, `NotFound`, and `Invalid` are refusal kinds, and each is reported only
when the transaction rolled back, so a write that reports one made no change. A decode failure of a
write is `Failed`, as for an http endpoint, though it is rolled back as well. The message carries the
database path and SQLite's message, never the bound values.

### MCP tools

`sx mcp import` turns the tools of a Model Context Protocol server into declarations, and `sx mcp run`
runs a program against that server. Neither is a pack: an imported tool is an ordinary capability whose
declaration carries an `mcp` object, so `check`, `explain`, and `sx serve` need nothing new, and only
`sx mcp run` performs it.

```
sx mcp import --name NAME -- COMMAND [ARG ...]      > tools.json
sx mcp import --name NAME --tools SAVED_TOOLS_LIST  > tools.json
sx mcp run program.sx --declarations tools.json --allow mcp-NAME.TOOL -- COMMAND [ARG ...]
```

**Protocol.** Sx speaks the stdio transport of MCP revision `2025-06-18`: one JSON-RPC 2.0 message per
line on the server's standard input and output. It sends `initialize` with that `protocolVersion` and empty
client capabilities, then the `notifications/initialized` notification. A server that answers with another
version is accepted only if it is `2025-03-26` or `2024-11-05`, which share the subset below, and
otherwise the connection is refused. The subset is `tools/list` with `cursor` and `nextCursor`,
`tools/call` with `name` and `arguments`, and `notifications/cancelled` after a timeout. Sx reads a
tool's `name`, `description`, `title`, `inputSchema`, `outputSchema`, and the `readOnlyHint`,
`destructiveHint`, and `idempotentHint` annotations, and a result's `content`, `structuredContent`, and
`isError`. It answers a server's `ping` and refuses every other server request (sampling, roots,
elicitation) with `-32601`. Resources, prompts, subscriptions, `list_changed` notifications, progress,
logging, and the HTTP transports are not implemented. A line over 16 MiB, or a line that is not JSON, ends
the connection. The server inherits `sx`'s environment, is found through `PATH`, and keeps its standard
error on `sx`'s. The command line is the user's own authority, so only the grant limits what a program may
ask of the server.

**Names.** A server `NAME` (lowercase letters and digits joined by `-`, starting with a letter) and a tool
become the capability `mcp-NAME.TOOL`. The reader allows exactly one `.` in a capability ID, so
`mcp.NAME.TOOL` is not writable and the namespace is `mcp-NAME`. A tool name is written in kebab case: an
uppercase letter after a lowercase letter or digit starts a new word, `_`, `.`, and `-` separate words, and
everything is lowercase, so `listIssues` is `list-issues` and `get_user.profile` is `get-user-profile`. A
name with another character, a first character that is not a letter, or an empty word is skipped. The rule
is not injective over all strings (`readFile` and `read_file` meet), so two tools in one import that
produce one name, or one tool listed twice, are an error that names both. Enums are named
`mcp-NAME.ToolInputPath` or `mcp-NAME.ToolOutputPath`, from the tool's words and the property path
(`mcp-demo.EnumsInputMode`).

**Types.** The mapping is a whitelist. Annotation keywords (`title`, `description`, `default`,
`examples`, `deprecated`, `$comment`, `$schema`, `$id`) and keywords that only narrow a value
(`minLength`, `maxLength`, `pattern`, `minimum`, `maximum`, `exclusiveMinimum`, `exclusiveMaximum`,
`multipleOf`, `minItems`, `maxItems`, `uniqueItems`, `minProperties`, `maxProperties`, and `format` other
than `byte` and `binary`) are ignored: the server enforces them. Any other keyword skips the tool.

| JSON Schema | Sx type |
| --- | --- |
| `string`, `integer`, `number`, `boolean` | `String`, `Int`, `Float`, `Bool` |
| `string` with `enum` of distinct non-empty strings | a new enum with those variants, named by the string |
| `array` with one `items` schema | `List<T>` |
| `array` with `prefixItems`, `items: false`, and `minItems` equal to its length | `Tuple<T1, ..., Tn>` |
| `object` with `properties` | `Record {...}`. A property not in `required` is `Option<T>`. Closed for an input. For an output, open unless `additionalProperties` is `false` |
| `object` with no `properties` and an `additionalProperties` schema | `Map<String, V>` |
| `object` with neither, as the whole `inputSchema` | `Record {}` |
| `type` list or `anyOf` of exactly one schema and `null` | `Option<T>` |
| local `$ref` to `#/$defs/NAME` or `#/definitions/NAME` | the definition, inlined, when it does not recurse |

A tool is skipped, with its reason on standard error, for `oneOf`, `allOf`, `not`, `anyOf` that is not
the nullable pattern, `patternProperties`, `const`, `if`/`then`/`else`, `contentEncoding`,
`format: byte`, any keyword not listed above (including `nullable`), a boolean schema, a recursive,
remote, or dangling `$ref`, a `type` list that is not one type and `null`, a free-form nested `object`, an
`array` without `items` or with an unfixed `prefixItems`, an `enum` that is not strings, `properties`
beside an `additionalProperties` schema, a `required` name that is not a property, a property name that
is not letters, digits, `_`, and `-` starting with a letter, an `outputSchema` that is not an object, a
missing `inputSchema`, a schema nested more than 64 levels, a type that has no JSON encoding (an
`Option` of an `Option`, a `Map` of `Option`), and an input property that is both required and nullable,
because `none` is omitted from the arguments and could not send the `null`. A tool without an
`outputSchema` returns `String`, the concatenation of its `text` content blocks. The skip is the whole
tool: nothing is approximated, and the rest of the server's tools import.

**Effects.** `readOnlyHint: true` is a `read`. Otherwise `destructiveHint: false` together with
`idempotentHint: true` is an idempotent `write`. Anything else, including a tool with no annotations or
with one hint, is a `write` that is not idempotent. The hints are the server's claim and the importer cannot
verify them: a server that mislabels a mutating tool as read-only lets a program call it in `par`.

**Declarations.** The importer prints a declarations object with `enums` and `capabilities`. Each
capability has `id`, `effect`, `idempotent` for an idempotent write, a one-line `description` (the first
line of the tool's description, else its title), `input`, `output`, and
`"mcp": {"server": NAME, "tool": ORIGINAL_NAME, "result": "text" or "structured"}`. It declares no scope
and grants nothing: `--allow mcp-NAME.TOOL` or `--grants` does. Several imports merge by concatenating
`enums` and `capabilities`; a repeated capability ID is a loader error. `--tools FILE` reads a saved
`tools/list` result, an object with a `tools` list, and refuses one that still has a `nextCursor`; it
imports the same declarations a live server of the same tools gives. Every capability an import writes
loads through the ordinary declaration loader before it is printed, and one that would not load is skipped.

**Running.** `sx mcp run FILE --declarations FILE [--allow ID] [--grants FILE] [--input NAME=JSON]
[--format text|json] [--name NAME] [--timeout-ms MS] -- COMMAND [ARG ...]` checks the program like
`sx check`, so a program that is rejected, or that calls no tool of the server, never starts it. Native
capabilities (`fs`, `http`, `process`) in the same program run as in `sx run --grants`. The declarations may
hold the tools of several servers; `--name` chooses the one this command line starts, and a call to another
server's tool fails with `no pack implements`. Each request becomes one `tools/call` with its own
JSON-RPC id on the one connection, so the machine's concurrency budget bounds the calls in flight, and
answers may arrive in any order. A property whose value is `none` is left out of `arguments` (but
a `null` element of a list is sent), and an optional property the server leaves out of `structuredContent`
reads as `none`. The server is stopped when the run ends, however it ends: Sx closes its standard
input, waits up to 300 ms, and then sends `SIGKILL` to its process group, which also ends anything it
started. The call timeout, which also bounds `initialize`, is 60 seconds, or `--timeout-ms`. A timed-out call sends
`notifications/cancelled`, and a later answer is dropped.

| Situation | Read | Write |
| --- | --- | --- |
| Result with `isError: true`; the message is its text | `Failed` | `Failed` |
| The result does not decode against the `outputSchema`, has no `structuredContent` when it declares one, or has a non-text content block when it declares none | `Invalid` | `Failed` |
| JSON-RPC error `-32602` (invalid params, including an unknown tool) | `Invalid` | `Failed` |
| Any other JSON-RPC error | `Failed` | `Failed` |
| The server exited, closed its output, wrote a line that is not JSON, or the pipe broke | `Unavailable` | `Failed` |
| No answer within the timeout | `Timeout` | `Timeout` |

`Invalid` is a refusal kind, so it is reported only for a read, where nothing took effect. A write
whose answer cannot be understood may already have happened, so it is `Failed`, as in the
[http pack](#the-http-pack). A server that cannot start, answers `initialize` with an unsupported version,
or does not answer it in time is a command-line error before any call, exit status 64 for `run` and 2 for
`import`.

**Explain.** `sx explain program.sx --declarations tools.json` lists each imported tool the program calls
with its effect (`read`, `write, idempotent`, or `write, not idempotent`), `allowed` or `not allowed`, and
the literal input of each call site. It starts no server.

**What this does not do.** It does not check at run time that the server still serves the declared tools or
schemas: a tool that changed shape shows up as `Invalid` or `Failed` on the call. It does not confine the
server, which runs as the user and can do anything the user can. It validates an input against the Sx type,
not against the full schema, so a narrower constraint (`minimum`, `pattern`) is the server's to enforce.

### Serving tools over MCP

`sx mcp serve` is the other direction: it exposes the capabilities the binary performs as an MCP server on
its standard input and output, so a client in another language reaches the same performers that
`sx run --grants` uses. `sx mcp describe` prints the same set in Sx terms.

```
sx mcp serve    --declarations FILE [--grants FILE] [--allow ID ...]
sx mcp describe --declarations FILE [--grants FILE] [--allow ID ...]
```

**What is served.** Exactly the granted capabilities that the binary performs natively: the pack
capabilities and the declared HTTP endpoints, and any kind added later through the same performer lookup.
Grants load as for `sx run`, so a scoped capability needs its scope in `--grants`. A tool that is not granted
is not listed, and calling it is the JSON-RPC error `-32602`. A granted capability with no native
implementation, such as an imported MCP tool or a plain declared capability, is not served and is named on
standard error. A granted capability whose types have no JSON Schema, an ID that is not a valid tool name,
and a grant list with nothing to serve are usage errors. The tool name is the capability ID, `fs.read-text`
or `github.repo`, and tools are listed in ID order on one page.

**Protocol.** The stdio transport and the subset of [MCP tools](#mcp-tools), as a server. `initialize` answers
with the client's `protocolVersion` when it is one of the three Sx accepts, else `2025-06-18`, and declares
`tools` with `listChanged: false`. The methods are `ping`, `tools/list` (a `cursor` is `-32602`, because none is
issued), `tools/call`, and the `notifications/cancelled` notification; any other request is `-32601`. A line that is not JSON is `-32700` with a
null id, and a line over 16 MiB ends the connection. The server does not require `initialize` first.

**Types.** The schema of a tool is the inverse of the import table. `inputSchema` is the input record, and a
record field of type `Option<T>` is a property outside `required` with the schema of `T`, which is how the
importer reads it back.

| Sx type | JSON Schema |
| --- | --- |
| `Bool`, `Int`, `Float`, `String` | `boolean`, `integer`, `number`, `string` |
| `Bytes` | `string` with `contentEncoding: base64` |
| `Unit` | `null` |
| `List<T>` | `array` with `items` |
| `Tuple<T1, ..., Tn>` | `array` with `prefixItems`, `items: false`, and `minItems` and `maxItems` of `n` |
| `Map<String, V>` | `object` with `additionalProperties` |
| `Record {...}` | `object` with `properties`, `required`, and `additionalProperties: false`; a `Record open` omits `additionalProperties` |
| `Option<T>` as an output field, or anywhere but an input field | `anyOf` of the schema of `T` and `{"type": "null"}`; an output field is also in `required`, because the result always carries it |
| enum without payloads | `string` with `enum` |
| enum with payloads | `oneOf` of the plain variants as one `string` enum and, per payload variant, `{"tag", "value"}` as the kernel encodes it |
| `Result<T, E>` | `oneOf` of `{"ok": T}` and `{"err": E}` |
| validated nominal | the schema of its base with the validation summary as `description` |

**Results.** The result follows the output type. A `String` output has no `outputSchema`, and the result is
one `text` block holding the string. A `Record` or `Map` output has an `outputSchema`, and the result carries
`structuredContent` and the same JSON as a `text` block. Any other output, such as `List<fs.Entry>`,
`Option<fs.Stat>`, or `Unit`, is wrapped: its `outputSchema` is an object with one required property `result`,
the result is `{"result": VALUE}`, and the tool carries `"_meta": {"sx/result": "wrapped"}`, which a client
reads to unwrap. Encoding is the kernel's, so a `none` field is `null`.

**Annotations.** A read is `readOnlyHint: true`. A write that is idempotent is `readOnlyHint: false`,
`destructiveHint: false`, `idempotentHint: true`, and any other write is `readOnlyHint: false`,
`destructiveHint: true`, `idempotentHint: false`, the three combinations `sx mcp import` reads back as the same
effect.

**Calls.** A `tools/call` takes the input as `arguments`. An omitted optional property reads as `none`, and so
does `null`. The arguments are decoded against the input type, screened by the grant's scope check, and
performed by the same function `sx run` uses, with the grant's prepared scope. Each call runs on its own
thread, up to 64 at a time, and answers arrive in completion order; at the limit the server stops reading
until a call finishes. A `notifications/cancelled` for a call in flight drops its answer and does not interrupt
the performer. When the client closes the server's input, the server lets the calls in flight finish, which
each pack bounds, and exits with status 0.

**Errors.** A failed call is a result with `isError: true`, a `text` block `KIND: message`, and
`structuredContent` `{"kind": KIND, "message": TEXT}`, with `KIND` one of the eight call error kinds.

| Situation | Kind |
| --- | --- |
| The arguments do not decode against the input type | `Invalid` |
| The scope check denies the call | `PermissionDenied` |
| The performer fails | the kind of its own table: [fs](#the-fs-pack), [process](#the-process-pack), [http](#the-http-pack) |

A missing or unknown tool name and non-object `arguments` are the JSON-RPC error `-32602`, because no call
was made. `sx mcp run` reads any `isError` result as `Failed`, so the kind survives only for a client that
reads `structuredContent`.

**Round trip.** `sx mcp import --name NAME -- sx mcp serve ...` reproduces the input and output types, the
effect, and the idempotence of every tool whose types the import table can read: the capability
`github.repo` comes back as `mcp-NAME.github-repo`. Four things do not come back. A wrapped output imports as
`Record {result: T}`, and a `Unit` or zero-length tuple makes the importer skip the tool, because
`type: null` alone has no Sx type. `Bytes` is skipped, because `contentEncoding` is not on the whitelist. An
open input record imports closed, because every imported input is. Enums are renamed from the tool and the
property path.

**`describe`.** Prints `{"enums", "types", "capabilities"}`: the capabilities `serve` would list with
`id`, `effect`, `idempotent`, `description`, `input`, and `output` as Sx type strings, and the enums and named
record types they use, in the shape of a declarations file. A client that generates types from Sx
declarations, such as the TypeScript declarations of `bench/codemode/typescript.py`, reads this instead of
parsing `sx help`.

## Execution

The machine is a step function over a state held as plain data. Each run ends in one of three
outcomes.

- `Completed(value)`: the program finished.
- `Suspended(requests, continuation)`: the program needs capability results.
- `Failed(error)`: the program stopped. See [Failures](#failures).

Evaluation runs in fibers. The program starts in one fiber, and each `par` branch and each active
`par-map` element runs in its own child fiber. A fiber's path lists the branch or element index at
each level, so the second element of a `par-map` inside the first `par` branch has path `0.1`.
Within a fiber, evaluation is sequential.

A request's ID is `r`, the fiber path, a colon, and the request's sequence number on that path:
`r:1`, `r.0:2`, `r.0.1:1`. IDs never depend on completion order, and no two requests in one run
share one. A suspension holds every request that is ready, up to the `concurrency` budget. Any
others wait for the next suspension, lowest IDs first.

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
| `loop-limit` | A `loop` reached its `:max` and its body asked to `recur` again | No |
| `budget-exhausted` | A budget ran out. The error names the budget. | No |
| `arithmetic` | Overflow, division by zero, a `Float` result that is infinite or not a number, negative `take` count, or a core function whose arithmetic fails, such as a `decimal` result beyond 38 digits | No |
| `invalid-input` | A core function refused an input outside its domain; the message says why, as in `yaml.get` on a flow collection or `text.split` with an empty separator | No |
| `out-of-range` | `first` of an empty list, `nth` past the end, or `round`, `floor`, or `ceil` beyond the `Int` range | No |
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

`RateLimited`, `NotFound`, `PermissionDenied`, `Invalid`, and `Conflict` are the refusal kinds.
A host reports a refusal kind only when the operation did not take effect. A host that cannot be
sure, such as after a dropped connection or a response it could not read, reports `Timeout`,
`Unavailable`, or `Failed`. The checker relies on this to allow `retry` of a write that is not
idempotent on refusal kinds only. See [retry](#retry).

When a branch of `par` or `par-map` fails, the form fails with the failure of the lowest-numbered
failed branch, once every fiber has run as far as it can without new responses. Its other branches
stop. The host still answers their outstanding requests, and the machine discards those answers.
Effects that already ran are not rolled back.

## Inputs

A host may declare typed inputs for a program, each a name and a type such as
`owners: List<String>`. The program refers to an input by name, like a `let` binding, and a `let`
may shadow it. The host supplies a value for every declared input, in the
[JSON encoding](#values-and-json), before the run starts, and a value that does not match its type
rejects the run before any request. An undeclared name is an `unknown-name` error. `sx explain`
lists the inputs a program uses.

A host declares inputs in its declarations (`inputs` maps names to type text). A script declares its own, in its [header](#scripts).

A host maps command-line options to inputs. In the repomod host, `discover --owner acme --owner beta`
reaches `discover.sx` as `owners`, a `List<String>`, and `status --offline --dirty` reaches
`status-offline.sx` as `only-dirty`, a `Bool`.

## Scripts

A program may be a script:

```
(script {:inputs {:owners "List<String>" :limit "Int"}}
  (take limit owners))
```

`script` takes a header record and a body, and only the outermost form of a program can be one. The
header's only key is `:inputs`, a record from input names to types. Each type is a string literal in
the declaration type language, parsed by the same parser as a declaration's types and resolved against
the declared types, enums, and core types, so a core's type such as `decimal.Decimal` needs its core
registered. The body sees each input by name as in [Inputs](#inputs), and a `let` may shadow it.

A header declares no authority. Which capabilities a script may call is still the host's grant, and a
`call` to an ungranted capability is `unauthorized` as in any program. A header whose type has no JSON
encoding, such as `Option<Option<Int>>`, is rejected as `no-encoding`.

The checker reports these header errors, each pointing at the offending text and naming `sx help script`:

| Code | Cause |
| --- | --- |
| `script-position` | `script` anywhere but the outermost form |
| `arity` | `script` without exactly a header and a body |
| `script-header` | A header that is not a record literal, or an `:inputs` value that is not a record literal |
| `script-key` | A header key other than `:inputs`. A key within edit distance 2 of `:inputs` is suggested. |
| `input-type` | A type that is not a string literal, or a string the type parser rejects. The parser's message is the detail. |
| `reserved-name` | An input named like a form: a special form, `none`, or `script` |
| `no-encoding` | A type with no JSON encoding |
| `duplicate-field` | The same input, or the same header key, written twice. The reader reports it, as for any record. |

An input whose type is wrong stays in scope with no type of its own, so a use of it adds no further error.

When a host also declares inputs, the two declarations must agree exactly, with the same names and the
same types. A disagreement is a usage error, not a diagnostic, and it names each difference. A host that
declares no inputs agrees with any header.

### Running a script

`sx run report.sx --input 'owners=["acme"]' --input limit=2` decodes each `--input NAME=JSON` against
its declared type with the [type-directed decoder](#values-and-json), so a script needs no declarations
file when it calls no capability. A value that does not decode, a name that is not an input, and a
required input with no value are usage errors. The last lists every required input and its type. An
`Option` input may be left out and is then `none`. `sx explain` lists each declared input with its type,
used or not, and `--format json` adds `input_types`. The protocol's [`start`](#host-protocol) message
takes a script unchanged, with `inputs` as JSON, and refuses with `invalid-input` in the same cases. A
host-declared program keeps its stricter rule that every declared input has a value. `sx help script`
documents the form with a runnable example.

## Values and JSON

Capability inputs and outputs, program results, and fixtures use one JSON encoding. Decoding
always knows the expected type, so every encoding below is unambiguous.

| Type | JSON |
| --- | --- |
| `Bool` | `true` or `false` |
| `Int` | An integer in the signed 64-bit range. A fraction, an exponent, or `true` is rejected. |
| `Float` | A JSON number, with or without a fraction or exponent: `21` and `21.5` both decode. A number outside the finite 64-bit range is a mismatch. Encoding writes a finite number. |
| `String` | A string. A lone surrogate, which is not a Unicode scalar value, makes it a mismatch, as a map key does. |
| `Bytes` | A string of standard base64 (RFC 4648 alphabet, `+` and `/`) with `=` padding, canonical: the padding bits are zero. The URL-safe alphabet, missing padding, whitespace, and any other character are mismatches. |
| `Unit` | `null` |
| `List<T>` | An array |
| `Tuple<T1, ..., Tn>` | An array of exactly `n` elements |
| `Option<T>` | `null` for `none`, the encoding of `x` for `(some x)` |
| `Result<T, E>` | `{"ok": x}` or `{"err": e}` |
| `Record {k: T, ...}` | An object with exactly the declared fields, named without the colon |
| `Record open {k: T, ...}` | An object with at least the declared fields. Decoding drops the others. Encoding writes the declared fields only. |
| `Map<String, V>` | An object with any keys, each value encoded as `V` |
| Core enum, variant without a value | The variant name as a string, for example `"Missing"` |
| Core enum, variant with a value | `{"tag": "OnBranch", "value": x}` |
| Core nominal | The encoding of its base type |
| Core-validated nominal | A string. Decoding hands the string to the core, which accepts it and canonicalizes it, or refuses it with a reason. A number, or any other JSON, is a mismatch. |

`Option<T>` has no JSON encoding when `T` itself can encode as `null`, which is when `T` is `Unit`
or another `Option`. `none` and `(some x)` would both be `null`. `Map<String, Option<T>>` has none
either, because a `null` value and a missing key would collide. The checker rejects a capability
input or output, a program input, or a program result whose type contains such a type.

Decoding a capability response fails with `invalid-host-response` when a value does not match
the expected type. A missing field, an extra field, and `null` for a non-`Option` type are all
mismatches, except that an open record accepts extra fields. Nothing is dropped, defaulted, or converted. The error gives the path to the bad
value, the expected type, and what was found, for example
`$.repos[3].name: expected String, found number`. A map value's path names its key as a JSON
string, for example `$.totals["c-ada"]: expected Int, found a string`. A map decodes its entries in
ascending key order, so the first error is the same every time.

A `Bytes` mismatch names the path like any other, for example
`$.avatar: expected Bytes (standard base64 with padding), found a string that is not valid base64`.
Encoding writes `Bytes` as canonical base64, so a value read from one tool and sent to another is
unchanged. `Bytes` compare with `=` and `not=` byte by byte. They are not a sort key, and `str` does not
write them. Functions that build, slice, or convert `Bytes` belong in a later codec core.

Encoding writes record fields in declaration order and map keys in ascending order. Comparisons of JSON values, in fixtures and
in parity tests, ignore object key order.

## Host protocol

`sx serve` lets a host in any language run programs over standard input and output. The host
writes one JSON message per line, and the kernel writes one JSON message per line back. Both
sides use the [JSON encoding](#values-and-json) for values. A line longer than 16 MiB, a blank line,
and a line the kernel cannot read are never fatal: blank lines are ignored, and the others get an
`error` message. The kernel serves one execution at a time and any number of `explain` messages.

The first message the host sends carries `"version": 1`. Later messages may repeat it, and any
other number is an `unsupported-version` error. Unknown fields are errors, so a misspelled option
never passes silently.

Host to kernel:

| `type` | Fields | Meaning |
| --- | --- | --- |
| `start` | `program`, `allow` or `grants` or both, optional `declarations`, `inputs`, `budgets` | Check the program and run it. `allow` lists capability IDs granted without a scope, and `grants` is the object `--grants` reads, from capability ID to scope value or `null`. See [Grants and scopes](#grants-and-scopes). `inputs` maps each declared input name to its value. `budgets` maps budget names (`steps`, `calls`, `delay_ms`, `source_bytes`, `nodes`, `depth`, `concurrency`, `value_size`, `applications`, `instantiations`) to counts. |
| `explain` | `program`, optional `declarations`, `allow`, `grants` | Check the program against every declared capability and report its authority and scope verdicts without running it. |
| `check-scope` | `id` | While requests are outstanding, ask for the scope verdict of the call request `id`, under the running execution's grants. |
| `respond` | `responses` | Answer outstanding requests. Each response is `{"id": ID, "output": VALUE}` or `{"id": ID, "error": {"kind": KIND, "message": TEXT}}`. |
| `cancel` | none | Stop answering. The machine resumes with the responses it has, so every unanswered request fails the run with `invalid-host-response`. |

`declarations` is the object that `sx` reads with `--declarations`: `enums`, `types`,
`capabilities`, and `inputs` (names mapped to type text). It also takes `cores`, a list of core
IDs that the kernel registers before the rest, so a capability can name a core type such as
`git.Checkout`, and `packs`, a list of capability packs registered after the cores. The kernel
binary links the `text`, `semver`, `git`, and `github` cores and the `fs`, `http`, and `process` packs.

Kernel to host:

| `type` | Fields | Meaning |
| --- | --- | --- |
| `rejected` | `diagnostics`, `rendered` | The program did not check, so nothing ran. `diagnostics` holds the [structured diagnostics](#diagnostics) (`code`, `construct`, `span`, `message`, `detail`, `fix`, `help`), and `rendered` the text a person reads. |
| `requests` | `requests` | The run is suspended. Each request is `{"id", "kind": "call", "capability", "input"}` or `{"id", "kind": "delay", "milliseconds"}`. A delay is answered with `"output": null` once the host has waited. |
| `completed` | `value`, `trace` | The program finished. `value` is the result and `trace` the [trace](#execution). |
| `failed` | `failure`, `trace` | The program stopped. `failure` is `{"kind", "message", "span", "request"}`, where `span` and `request` may be `null`. |
| `explained` | `tools`, `inputs`, `result` | For each capability the program can call: `id`, `effect` (`read`, `write, idempotent`, or `write, not idempotent`), `allowed`, and the literal `inputs` of its calls, with `"computed at run time"` for arguments the program computes, and for a capability with a scope, `verdicts`. Also the inputs the program uses and its result type. For a script, `inputs` lists every declared input and `input_types` maps each to its type. Same as `sx explain --format json`. |
| `scope-verdict` | `id`, `verdict`, and `reason` when denied | The answer to `check-scope`: `allowed`, `denied` with the pack's reason, or `unchecked` when the capability's scope came from a declarations file, so no registered check can judge it and the host must. |
| `error` | `code`, `message` | The message was not valid, so the kernel did nothing with it. |

After `requests` the host answers in any order, in as many `respond` messages as it likes, with
any subset each time. The kernel resumes the machine once every outstanding request has a
response, or as soon as a response names an unknown or repeated ID, which fails the run with
`invalid-host-response` as [Execution](#execution) says. Another `requests` message, or the end of
the run, follows. While requests are outstanding the kernel reads only `respond`, `cancel`,
`check-scope`, and `explain`. A host applies the run-time scope check by sending `check-scope` for
each call before performing it, and answering a denied one with a `PermissionDenied` error that
carries the reason. An `error` message does not end the execution: the host may send a corrected message.
If the input ends first, the kernel sends an `end-of-input` error and exits with status 1.

Error codes: `malformed-json` (not JSON, not UTF-8, or nested too deeply),
`line-too-long`, `invalid-message` (not an object, no or unknown `type`, a missing, unknown, or
mistyped field, or a response that is not exactly one of `output` and `error`),
`unsupported-version`, `unexpected-message` (a `respond`, `cancel`, or `check-scope` with nothing
outstanding, a `check-scope` that names no outstanding call, or a `start` while one is),
`invalid-declarations`, `unknown-capability` (`allow` or `grants` names a capability the
declarations do not define), `invalid-grants` (`grants` is not an object, a scoped capability has no
scope or an unscoped one has one, a scope does not decode, or a pack refuses it, such as a missing
root), `invalid-input` (a missing, undeclared, or mistyped input, which
rejects the run before any request), `invalid-budget`, `end-of-input`, and `internal` (a kernel
bug, after which the execution is abandoned and the session continues).

An exchange, with the host's lines marked `>`:

```
> {"version": 1, "type": "start", "program": "(call store.get {:id \"a\"})", "allow": ["store.get"], "declarations": {...}}
{"type": "requests", "requests": [{"id": "r:1", "kind": "call", "capability": "store.get", "input": {"id": "a"}}]}
> {"type": "respond", "responses": [{"id": "r:1", "output": {"id": "a", "size": 1}}]}
{"type": "completed", "value": {"id": "a", "size": 1}, "trace": [...]}
```

## Diagnostics

The checker reports every error it finds, up to 20, followed by a count of the rest. A local
function is checked at each call, so one mistake in its body can be found many times: diagnostics
with the same code, span, and message are reported once, the first. The reader
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

The reader's bracket diagnostics read the indentation of the lines after an opener, because a
model that loses count of brackets still indents correctly. Blank lines and lines holding only a
comment are skipped.

- `unclosed` says where to close. For an opener at some column, find the first later line whose
  indentation is at most that column. The fix adds the closer at the end of the last line before
  it, or at the end of the program when no later line is that shallow.
- `unexpected-close` with no open bracket says that the closer closes nothing. When a top-level
  form closed earlier and every line from there to this closer is indented deeper than that form's
  opener, it names that earlier closer as the one that closed too soon, and the fix removes it.
- `unexpected-close` where a closer meets the wrong open bracket keeps the replacement fix, unless
  the closer matches an outer opener and indentation shows the inner opener should have ended
  before it. Then the fix adds the inner opener's closer at the end of its last line, and the
  reader goes on as if that closer were there.

A diagnostic's message follows these rules:

- Say what is wrong before saying why.
- Name types as Sx writes them, for example `List<String>`, never interpreter class names.
- Never use type-theory terms such as "unify" or "judgment".
- Suggest a name only when it is within edit distance 2 of a name in scope.

The checker's codes are:

| Group | Codes |
| --- | --- |
| Reading | `unclosed`, `unexpected-close`, `bad-literal`, `bad-symbol`, `int-range`, `float-range`, `bad-escape`, `record-key`, `record-missing-value`, `duplicate-field`, `empty-program`, `multiple-expressions`, `missing-result` |
| Names | `unknown-name`, `unknown-function`, `unknown-capability`, `unknown-field`, `missing-field`, `unknown-variant`, `reserved-name` |
| Calls | `arity`, `type-mismatch`, `unresolved-type`, `not-a-record`, `no-encoding`. `bad-literal` also reports a literal argument that a core rejects at check time, as `re.find` does for a pattern that does not compile. |
| Forms | `condition-not-bool`, `missing-else`, `branch-mismatch`, `fn-position`, `bad-pattern`, `bad-option`, `recur-position`, `recur-arity`, `loop-no-result` |
| Match | `non-exhaustive`, `unreachable-clause` |
| Effects | `unauthorized`, `out-of-scope`, `parallel-write`, `par-map-limit`, `retry-write`, `wait-write` |
| Script | `script-position`, `script-header`, `script-key`, `input-type`. See [Scripts](#scripts). |
| Size | `too-large`, naming the `source-bytes`, `nodes`, or `depth` budget |
| Habits | `habit`, naming the construct. The reader detects `nil`, `capitalized-boolean`, `quote`, `reader-macro`, `anonymous-fn-shorthand`, `set-literal`, `threading`, `js-operator`, `js-object-key`, `method-call`, and `namespace-slash`. The checker detects `defn`, `keyword-call`, `keyword-value`, `argument-order`, `assoc`, `let-body`, `while`, and `loop-without-max`. |

A habit diagnostic names the language the construct comes from and gives the Sx form. For
example, `(:name repo)` gets `Fix: (get repo :name)`.
A keyword used as a map key, as in `(get m :name)`, is the `keyword-value` habit with the fix `"name"`.
`assoc` and `dissoc` given a keyword, as in `(assoc r :archived true)`, are the `assoc` habit, with
`assoc-in`, `update`, and `merge`: a record has fixed fields, so build a new record literal.

A runtime failure is reported in the same format. It adds the error class from
[Failures](#failures), and, for capability errors, the request ID and capability. Its span points
at the form that failed.

## Help

`sx help` prints the guide: every form, the prelude in one line per function, and a few complete
examples. `sx help NAME` prints one entry, where `NAME` is a form, a prelude function, a qualified
core function or type, or a capability. An entry has:

- The signature, and for capabilities, the effect class and any scope type.
- A one-sentence description.
- At least one example with its result.

The registry generates core and capability entries from their declarations. Every example is a
test, so a help entry cannot drift from the interpreter.

### The guide for one host

`sx guide --declarations FILE [--grants FILE] [--allow ID ...]` prints what a model needs to write
one program against those declarations. It has these sections, in order:

1. The kernel guide that `sx help` prints.
2. How TypeScript habits map to Sx.
3. A section for each pack family whose tools are granted: files, programs, web services (raw or
   typed endpoints), declared SQL statements, and MCP tools. Each section gives the Sx idioms with
   an example, then notes on how those tools behave.
4. A section for each core the declarations list, with its functions.
5. Each granted tool, with its effect, its description, the method and URL of a typed endpoint, a
   call template written in Sx, and its input and result shapes.
6. A checklist of the mistakes models make most often.

A tool that is not granted does not appear, and neither does the section of a pack with no granted
tool. `--notes` prints only the behavior notes. They describe the tools and not Sx, so a host can
give them to a model that writes another language against the same tools. Every example in the
sections is checked against the tools it names by a test.

### Cores and assurance

A core registers through the Python API with `Registry.add_core`. `sx help CORE` prints a core's
version, its assurance, and the signature of each function, and `sx cores` lists every registered
core with the levels it has reached. `--format json` gives the same for a host. The plain `sx`
command registers no cores, because cores are Python packages that a host registers. A host
embeds the command with `sx.cli.main(argv, new_registry)`, where `new_registry` builds its
registry, so `sx-repomod help text` and `sx-repomod cores` answer for the cores that host
registers. The Rust `sx` also registers the built-in cores that a declarations file lists under
`cores`.

A core reports the evidence that exists for it, level by level, and never a single "verified"
flag. A level that has no evidence says `none`.

| Level | Evidence |
| --- | --- |
| `unverified` | The interface was validated. Every registered core has this. |
| `conformance-tested` | Named fixture and parity tests pass. |
| `specified` | A Lean model of the core exists. |
| `proved-properties` | Named theorems about that model check. |

A core declares its evidence as `Evidence(level, references)`, where the references name the tests,
the model file, or the theorems. The registry rejects a claim that names nothing, a second claim
at one level, a claim of `unverified`, and `proved-properties` without `specified`. A test requires
every named conformance test to exist. A theorem is about the Lean model only. It says nothing
about the Python or Rust code, or about real Git and GitHub behavior, without separate
conformance evidence.

## The decimal core

`decimal` is a Rust core, registered by listing `"decimal"` under `cores` in the declarations. It adds
the core-validated nominal `decimal.Decimal` and the nullary enum `decimal.Rounding` with the variants
`HalfEven`, `HalfUp`, `Down`, `Up`, `Floor`, and `Ceiling`.

A `decimal.Decimal` is an exact decimal number with at most 38 significant digits and at most 38
fractional digits. A literal is an optional `-`, one or more digits, and optionally a `.` with one or
more digits. There is no `+`, exponent, space, or bare point, and leading zeros are accepted. The core
keeps every value in canonical form: trailing fractional zeros are dropped and zero has no sign, so
`1.50` is `1.5`, `-0` is `0`, and `=` on decimals is numeric equality. An integer keeps its zeros,
so `100.00` is `100`. A value is a string in JSON, such as `"12.50"`, which decoding validates and
canonicalizes through the core: a JSON number, a string that is not a literal, and a literal beyond the
digit limits are decode errors with the path, as `invalid-host-response` or an invalid input. A
decimal is never a sort key, and `<` does not order decimals; `decimal.compare` does.

| Function | Type |
| --- | --- |
| `decimal.parse` | `String -> Option<decimal.Decimal>`, `none` for anything that is not a literal within the limits |
| `decimal.show` | `decimal.Decimal -> String`, the canonical text |
| `decimal.format` | `decimal.Decimal Int -> String`, exactly that many places, rounded half to even. Zero rounds to `0.00`, never `-0.00`. |
| `decimal.from-int` | `Int -> decimal.Decimal` |
| `decimal.add decimal.sub decimal.mul` | `decimal.Decimal decimal.Decimal -> decimal.Decimal`, exact |
| `decimal.div` | `decimal.Decimal decimal.Decimal Int decimal.Rounding -> decimal.Decimal`, the quotient rounded to that many places |
| `decimal.round` | `decimal.Decimal Int decimal.Rounding -> decimal.Decimal` |
| `decimal.compare` | `decimal.Decimal decimal.Decimal -> Int`, `-1`, `0`, or `1` |
| `decimal.to-float` | `decimal.Decimal -> Float`, the nearest `Float` |

Places are 0 to 38. The modes: `HalfEven` and `HalfUp` round a tie to the even neighbor and away from
zero; `Down` and `Up` go toward and away from zero; `Floor` and `Ceiling` go toward negative and
positive infinity. A tie only matters to the two half modes.

A place count outside 0 to 38, division by zero (`0` divided by `0` included), and a result beyond
either limit fail the program with `arithmetic`. A result is judged in canonical form: `9999999999999999999999999999999999999.5` plus `0.5` is `10000000000000000000000000000000000000` and fits, while
`99999999999999999999999999999999999999` plus `1` does not. A core reports such a failure
as `CoreError::Arithmetic`, which the machine turns into `arithmetic` with the function's name. A
core that refuses an input outside its domain returns `CoreError::InvalidInput`, which becomes
`invalid-input`; any other `CoreError::Failed` is `internal`.

`fixtures/cores/decimal.json` holds the conformance vectors, generated from Python's `decimal` module
by `rust/sx-cores/scripts/decimal_vectors.py`. A test regenerates them and fails when the committed
file is stale. The Lean reference runs decimal programs with its own implementation of the core, so
the agreement check compares two implementations.

## The yaml core

`yaml` is a Rust core, registered by listing `"yaml"` under `cores`. It reads and edits YAML text in
place and has no dependency: a parser for a documented subset finds each scalar's byte span, and an
edit replaces that span and nothing else.

| Function | Type |
| --- | --- |
| `yaml.get` | `String List<String> -> Option<String>`, the scalar at the path, or `none` when the path is absent |
| `yaml.set` | `String List<String> String -> String`, the text with the scalar at the path replaced |
| `yaml.keys` | `String List<String> -> List<String>`, a mapping's keys in document order, or a sequence's indexes |

A path lists mapping keys from the root. Where the node is a sequence, a component is a decimal index
without a sign or leading zeros (`"0"`, `"12"`), so `["containers" "0" "image"]` reads the first
container's image. A key is matched on its decoded text, so a quoted key matches its unquoted spelling.
The empty path names the document root. A path that does not exist, including one that runs through a
scalar or past the end of a sequence, is absent: `yaml.get` returns `none` and `yaml.keys` returns `[]`.
A null or empty value (`key:`) reads as the empty string, and a quoted empty string reads the same.
`yaml.get` and `yaml.set` on a mapping or sequence, `yaml.keys` on a scalar, and `yaml.set` on an
absent path are failures, since each is a mistake the program should not carry past.

**The subset.** Block mappings and block sequences, nested to any depth, with plain, single-quoted,
and double-quoted scalars as keys and values; comments on their own line or after a value; blank lines;
`\n` and `\r\n` line ends; one optional `---` before the content; a sequence under a key at the key's own
indentation or deeper; and a sequence item that opens a mapping (`- name: web`) or another sequence.
Double-quoted scalars decode the YAML escapes (`\n`, `\"`, `\\`, `\xNN`, `\uNNNN`, `\UNNNNNNNN`, `\N`, `\L`,
`\P`, and the rest), and single-quoted scalars decode `''`. A plain scalar ends at the end of its line or
at ` #`, and a scalar is always read as a string: `3`, `true`, and `null` are the text `3`, `true`, and `null`.

Everything else is refused with a failure that names the line and the construct, and the whole text is
checked on every call, so a refused construct anywhere fails even a read of a different key:

| Refused | Failure |
| --- | --- |
| Flow collections (`[a, b]`, `{a: 1}`) | `flow collections` |
| Anchors, aliases, tags | `anchors`, `aliases`, `tags` |
| Block scalars (`\|`, `>`), a plain or quoted scalar that continues on the next line, a scalar placed on the line after its key, an escaped line break | `multi-line scalars` |
| A second document (`---` or `...` after the content) | `multiple documents or document end markers` |
| Directives, content after `---`, a byte order mark | `directives`, `content on the document start line`, `a byte order mark` |
| Tabs in indentation | `tabs used for indentation` |
| Complex keys (`? key`) | `complex keys` |

Text that is not valid YAML in the subset is refused the same way, with a message about what is wrong:
a duplicate key in one mapping, a line indented deeper than the structure above it, content that does not
fit the structure, `a: b: c`, text after a closing quote, an undefined escape, and a plain scalar that starts
with `@`, a backtick, `,`, `]`, or `}`.

**How a failure reaches the program.** A refusal is `CoreError::InvalidInput`, so the machine stops the
program with the `invalid-input` failure kind and the message `yaml.get: line 3: flow collections are
outside the supported YAML subset, so the text is left alone`. `attempt` does not catch it, like every
`invalid-input` failure.

**Quoting.** `yaml.set` writes the new value in the style of the scalar it replaces: a double-quoted
scalar stays double-quoted, a single-quoted one stays single-quoted (double-quoted instead when the value
has a newline, a control character, or another line break, which single quotes cannot hold), and a plain one
stays plain when that is safe. Plain text is unsafe for an empty value, a value with leading or trailing
whitespace, a tab or control character, `: ` or ` #` inside it, a trailing `:`, a leading `,[]{}#&*!|>'"%@`
or backtick, a leading `-`, `?`, or `:` followed by a space or nothing, or a leading `---` or `...`; the
value is double-quoted then. A plain scalar that is text also stays text: replacing `tag: stable` with
`1.10` writes `tag: "1.10"`, because plain `1.10` would be read as a number by other tools. When the old
scalar was a number, a boolean, a null, or a date, or was empty, the new value is written plain whenever
the syntax allows, so `replicas: 3` set to `5` is `replicas: 5`. An empty value (`key:`, `-`) gains the
value after the colon or dash and keeps a trailing comment: `key: # c` becomes `key: v # c`.

An edit replaces the bytes of one scalar, so the text before and after it, comments, blank lines,
indentation, key order, line ends, and the scalar's own trailing spaces and comment are unchanged.

## The re core

`re` is a Rust core, registered by listing `"re"` under `cores`. It matches regular expressions with the
[`regex-lite`](https://crates.io/crates/regex-lite) engine, so a workflow can read compiler diagnostics,
Git output, and logs without splitting on punctuation. Every function is pure, and the pattern is always
the first argument, as in Clojure's `re-find`.

| Function | Type |
| --- | --- |
| `re.find` | `String String -> Option<List<String>>`, the first match anywhere: the whole match, then each capture group |
| `re.find-all` | `String String -> List<List<String>>`, every non-overlapping match, each as `re.find` lists it |
| `re.matches` | `String String -> Option<List<String>>`, as `re.find`, only when the pattern matches the whole string |
| `re.test` | `String String -> Bool`, whether the pattern matches anywhere |
| `re.replace` | `String String String -> String`, pattern, input, replacement: every match replaced |
| `re.split` | `String String -> List<String>`, the input cut at every match |

```
(match (re.find #"main\.c:(\d+):(\d+): error: (.*)" "main.c:9:18: error: expected ';'")
  (some g) {:line (nth g 1) :message (nth g 3)}
  none {:line "" :message ""})
```

**Argument order.** The pattern comes first in all six functions, including `re.replace` and `re.split`,
although `text.replace` and `text.split` take the string first. Two strings and a pattern are all `String`,
so a swapped call type-checks; one rule for the whole core is the order a model can keep, and it is the
order of `re-find`, `re-matches`, and `re-seq`, which models know from Clojure.

**Groups.** `(nth m 0)` is the whole match and `(nth m 1)` is group 1, numbered by opening parenthesis, as in
JavaScript's `match`. A group that did not take part in the match, such as `(b)?` that matched nothing or the
unused side of `(a)|(b)`, is the empty string, so it cannot be told from a group that matched empty text.
`re.find` and `re.matches` always return the whole-match element even when the match is empty.

**Matches.** `re.find-all` scans left to right and never overlaps matches. A match that can be empty is
found at each position that fits and not right after a match that ended there: `x*` on `axb` gives `""`,
`x`, `""`. `re.matches` anchors the pattern at both ends, and the pattern is compiled on its own first,
so an invalid pattern is refused as itself. `re.split` returns the text between matches, with an empty
string where a match touches either end; capture groups add no pieces, `(re.split #"," "")` is `[""]`,
and a pattern that matches the empty string cuts between every character, with an empty piece at each end.

**Replacement.** In the replacement, `$1` and `${1}` write a numbered group, `${name}` a named group
(`(?P<name>...)`), and `$$` a dollar sign. `$1x` names a group called `1x`, so write `${1}x`. A group that
does not exist or did not take part writes nothing. A backslash in the replacement is a backslash.

**Pattern literals.** A pattern is an ordinary `String`. Write it with the `#"..."` form, which keeps
backslashes, as in `#"\d+"`; in a plain `"\d+"` the reader refuses the unknown escape and points to
`#"..."`. A literal pattern, a `#"..."` or a `"..."` string written directly as the pattern argument, is
compiled when the program is checked, and a pattern that does not compile is a `bad-literal` diagnostic at
the literal with the engine's message. A computed pattern, such as `(str "^" prefix)`, is compiled when the call
runs, and one that does not compile stops the program with the `invalid-input` failure kind, which `attempt`
does not catch. Only the first argument is a pattern: the input and the replacement may contain any
characters.

**Limits.** A pattern may nest groups at most 50 deep and may compile to at most 1 MiB of program
(`regex-lite`'s size limit, set explicitly); a pattern beyond either is refused like an invalid one. Matching
is linear in the input times the pattern and never backtracks exponentially.

**What the syntax lacks.** `regex-lite` is a subset of what JavaScript and Rust's `regex` accept. It has
literals, `.` (any character except a newline, unless `(?s)`), classes such as `[a-z]` and `[^,]`,
`\d \w \s \b` and their negations, anchors `^ $ \A \z` (`$` matches only at the end unless `(?m)`),
alternation, groups, non-capturing groups `(?:...)`, named groups `(?P<name>...)` and `(?<name>...)`, the greedy and lazy repeats
`* + ? {n,m}`, and the flags `i m s x U`. It has no lookahead or lookbehind (`(?=` `(?!` `(?<=` `(?<!`), no backreferences (`\1`),
no `\p{...}` Unicode property classes, no class intersection or difference, and no atomic groups. `\d`,
`\w`, `\s`, `\b`, and `(?i)` are ASCII-only: `\w` does not match `é`, and `(?i)é` does not match `É`;
write the characters in a class, such as `[a-zA-Zé]`. `.` and classes work on whole characters, so `.`
matches one emoji.

`rust/sx-cores/tests/re_core.rs` holds the table of expected results, including non-ASCII input, empty
matches, and unmatched groups. The Python interpreter and the Lean reference have no `re` core, and Python
does not read the `#"..."` form.

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
| `value-size` | Size of any intermediate or final value. A map counts one for itself plus the size of each value, like a record. |
| `applications` | Function argument applications |
| `instantiations` | Local function bodies the checker types, one per call site |

The host enforces wall-clock time outside the machine.

## Example

`discover` from the repomod host is a complete program. It reads the catalog, asks GitHub about
each owner in parallel, and compares the two lists by case-folded name. The host passes `owners`
as an [input](#inputs), and `catalog.select`, `catalog.owner`, and `github.owner-repos` are
capabilities. The full text is in `hosts/repomod/src/sx_repomod/programs/discover.sx`, next to
`status-offline.sx`. Its core of comparing names looks like this:

```
(let [found (sort-by text.casefold (distinct-by text.casefold (flatten pages)))
      found-keys (map text.casefold found)
      known-keys (map text.casefold known)]
  {:uncatalogued (filter (fn [name] (not (contains? known-keys (text.casefold name)))) found)
   :missing (sort-by text.casefold
              (filter (fn [name] (not (contains? found-keys (text.casefold name)))) known))})
```
