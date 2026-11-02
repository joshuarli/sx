# Sx language, draft v0

This is the working specification for the Python interpreter. It changes as the syntax pilot, the
interpreter, and the model probe expose problems. Every change updates this file, the fixtures, and the
interpreter in the same commit.

## Programs

A program is one expression. There are no top-level definitions, imports, macros, quoting, or
recursion. Evaluation is strict, and arguments evaluate left to right, except inside `par` and
`par-map`.

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
| Duration | `250ms`, `5s`, `2m`, `1h` | Only in `retry` and `wait-until` options and as the argument of `sleep`. |
| String | `"..."` | UTF-8. Escapes: `\"`, `\\`, `\n`, `\r`, `\t`, `\u{HEX}`. |
| Keyword | `:name` | Record field names only. Same characters as a symbol. |
| Symbol | `name`, `repo-id`, `repoId`, `ahead_count`, `ok?`, `acc'` | A lowercase letter, then letters, digits, `-`, and `_`, then optionally `?` or primes. |
| Operator | `+ - * = not= < <= > >=` | Prelude functions |
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
| `String` | UTF-8 strings |
| `Unit` | One value, returned by capabilities with no result and by `sleep` |
| `List<T>` | Ordered, finite |
| `Tuple<T1, ..., Tn>` | Produced by `par`, `group-by`, and bracket literals of mixed types or used as tuples |
| `Option<T>` | `(some x)`, `none` |
| `Result<T, E>` | `(ok x)`, `(err e)` |
| `Record {k: T, ...}` | Structural, with exactly the declared fields |
| `Map<String, V>` | Any number of string keys, each with a value of type `V`. Keys are always strings, because JSON object keys are strings, so `Map<Int, V>` and every other key type are rejected. |
| Core enum | Nominal, declared by a core, for example `git.Checkout` |
| Core nominal | Nominal wrapper over a base type, declared by a core, for example `git.RepoId` |

There is no `Any`, no subtyping, and no user-declared type. Core functions construct core enum
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
`(= (get ci :state) "Passing")` compares with the variant `Passing`, and a record field typed as
the enum takes `"Failing"`. A string literal that names no variant of the enum, or names a variant
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

Programs have no type annotations. The checker infers types bidirectionally from literals,
bindings, prelude and core signatures, capability signatures, and the expected type of each
argument position. A lambda parameter gets its type from the position the lambda fills. `none`,
`[]`, `(ok x)`, and `(err e)` take their missing type parameters from the expected type. The
checker rejects an expression whose type it cannot resolve where the operation needs it: reading
a field, comparing, or sorting. A type left unresolved in the program's result, such as the
element type of an empty list, is harmless and allowed.

Equality is structural on every type. Ordering with `<`, `<=`, `>`, and `>=` is defined on `Int`
and on `String`, where strings compare lexicographically by Unicode code point. Sort keys, used by
`sort-by`, may also be `Bool` (`false` first) and tuples or lists of sort keys, compared element
by element. A map is not a sort key.

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
the scrutinee equals it: `(match n 1 "one" 2 "two" _ "many")`. The scrutinee's type must be the
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
| `+ - *` | `Int Int -> Int`. `(- x)` negates. |
| `quot rem` | `Int Int -> Int`, truncating toward zero |
| `= not=` | `T T -> Bool` |
| `< <= > >=` | `T T -> Bool` for `Int` or `String` |
| `not` | `Bool -> Bool` |
| `and or` | `Bool Bool -> Bool`. Both arguments always evaluate. Use `if` to make a call conditional. |
| `str` | `T ... -> String`, one or more arguments joined in order. Each is a `String`, an `Int` written in decimal, a `Bool` written `true` or `false`, or a value of an enum with no payloads written as its variant name. In a `sort-by` key every argument must be a `String`, because joining numbers into a string key sorts them as text. |
| `count` | `List<T> -> Int`, or `Map<String, V> -> Int` for the number of entries |
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

The allow-list is the first of two checks. The host sees every request's input before it runs
the request, so it can also refuse one request, for example a file outside a permitted directory.
A refused request resumes the machine with a `CallError` of kind `PermissionDenied`. `sx explain`
shows the inputs it can know before running: literal arguments, such as a fixed path, appear in
its output, and arguments computed at run time do not.

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

Evaluation runs in fibers. The program starts in one fiber, and each `par` branch and each active
`par-map` element runs in its own child fiber. A fiber's path lists the branch or element index at
each level, so the second element of a `par-map` inside the first `par` branch has path `0.1`.
Within a fiber, evaluation is sequential.

A request's ID is `r`, the fiber path, a colon, and the request's sequence number in that fiber:
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
| `arithmetic` | Overflow, division by zero, negative `take` count | No |
| `out-of-range` | `first` of an empty list, or `nth` past the end | No |
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

In v0 only the host declares inputs. A script file cannot declare its own.

A host maps command-line options to inputs. In the repomod host, `discover --owner acme --owner beta`
reaches `discover.sx` as `owners`, a `List<String>`, and `status --offline --dirty` reaches
`status-offline.sx` as `only-dirty`, a `Bool`.

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
| `Map<String, V>` | An object with any keys, each value encoded as `V` |
| Core enum, variant without a value | The variant name as a string, for example `"Missing"` |
| Core enum, variant with a value | `{"tag": "OnBranch", "value": x}` |
| Core nominal | The encoding of its base type |

`Option<T>` has no JSON encoding when `T` itself can encode as `null`, which is when `T` is `Unit`
or another `Option`. `none` and `(some x)` would both be `null`. `Map<String, Option<T>>` has none
either, because a `null` value and a missing key would collide. The checker rejects a capability
input or output, a program input, or a program result whose type contains such a type.

Decoding a capability response fails with `invalid-host-response` when a value does not match
the expected type. A missing field, an extra field, and `null` for a non-`Option` type are all
mismatches. Nothing is dropped, defaulted, or converted. The error gives the path to the bad
value, the expected type, and what was found, for example
`$.repos[3].name: expected String, found number`. A map value's path names its key as a JSON
string, for example `$.totals["c-ada"]: expected Int, found a string`. A map decodes its entries in
ascending key order, so the first error is the same every time.

Encoding writes record fields in declaration order and map keys in ascending order. Comparisons of JSON values, in fixtures and
in parity tests, ignore object key order.

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
| Reading | `unclosed`, `unexpected-close`, `bad-literal`, `bad-symbol`, `int-range`, `bad-escape`, `record-key`, `record-missing-value`, `duplicate-field`, `empty-program`, `multiple-expressions` |
| Names | `unknown-name`, `unknown-function`, `unknown-capability`, `unknown-field`, `missing-field`, `unknown-variant`, `reserved-name` |
| Calls | `arity`, `type-mismatch`, `unresolved-type`, `not-a-record`, `no-encoding` |
| Forms | `condition-not-bool`, `missing-else`, `branch-mismatch`, `fn-position`, `bad-pattern`, `bad-option`, `recur-position`, `recur-arity` |
| Match | `non-exhaustive`, `unreachable-clause` |
| Effects | `unauthorized`, `parallel-write`, `par-map-limit`, `retry-write`, `wait-write` |
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

- The signature, and for capabilities, the effect class.
- A one-sentence description.
- At least one example with its result.

The registry generates core and capability entries from their declarations. Every example is a
test, so a help entry cannot drift from the interpreter.

### Cores and assurance

A core registers through the Python API with `Registry.add_core`. `sx help CORE` prints a core's
version, its assurance, and the signature of each function, and `sx cores` lists every registered
core with the levels it has reached. `--format json` gives the same for a host. The plain `sx`
command registers no cores, because cores are Python packages that a host registers. A host
embeds the command with `sx.cli.main(argv, new_registry)`, where `new_registry` builds its
registry, so `sx-repomod help text` and `sx-repomod cores` answer for the cores that host
registers.

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
