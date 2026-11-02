# Sx

Sx is a small language for calling tools and combining their results. A program is one
expression, and its value is the program's result. Sx looks like Clojure, but it is not Clojure:
only the forms and functions below exist.

## Values

- `true`, `false`
- Integers: `42`, `-7`. Floats: `1.5`, `2e3`; never mixed with integers: `(float n)` converts,
  `(round x)` `(floor x)` `(ceil x)` give an Int, and `(/ a b)` divides Floats.
- Bytes: opaque binary data from tools, base64 in JSON. `(count b)` is its length; `(= a b)` compares.
- Strings: `"text"`, with escapes `\"`, `\\`, `\n`, `\r`, `\t`.
- Lists: `[1 2 3]`
- Records: `{:name "ada" :age 36}`. Field names are keywords. Commas are optional. Keywords
  only name fields. Use strings, such as `"failing"`, for labels.
- Tuples: `[1 "a"]`, a bracket literal whose elements have different types or that is later
  taken apart as `[a b]`.
- Optional values: `(some x)` or `none`. There is no `nil` or `null`. A tool field typed
  `Option<T>` holds `(some x)` or `none`. Wherever an optional is expected, a plain value works
  too, so `(if found? name none)` is fine. `(if x then else)` on an optional name `x` tests
  whether it is present, and inside `then`, `x` is the plain value. `(get r :field default)`
  reads an optional field, giving `default` for `none`. A record literal may leave out optional
  fields; they are `none`.
- Tool enums: write a variant as `Passing`. Where an enum is expected, the string `"Passing"`
  works too, and an `if` or `match` may return a string in one branch and an enum in another.
- Branches may return records with different fields; a field only some write is optional.
- Results: `(ok x)` or `(err e)`, produced by `attempt`.
- Durations, only in `retry`, `wait-until`, and `sleep`: `250ms`, `5s`, `2m`, `1h`.
- Comments start with `;`.

## Forms

```
(let [a expr1
      b expr2             ; later bindings see earlier ones
      _ expr3             ; run for effect, discard the value
      f (fn [x] ...)]     ; a local function, called as (f x)
  body)                   ; exactly one body expression
                          ; form names (attempt, match, ok, ...) cannot be bound: use attempts
(if condition then else)  ; condition is true or false, or an optional name
(cond test1 result1 test2 result2 :else other)   ; first true test wins; :else is required
(get record :field)       ; read a record field; (get record :field default) for an optional
(call tool.name {:field value})          ; call a tool with a record
(fn [x] body)             ; only as an argument to map, filter, and the like
(match value
  (some x) result         ; patterns: (some x) none (ok x) (err e)
  none other              ;           Variant (Variant x) for tool enums
  _ fallback)             ;           _ or a name matches anything; 1 "a" true match equal values
(par expr1 expr2)         ; evaluate tool reads in parallel, returns a tuple
(let [[a b] (par e1 e2)] body)           ; take a tuple apart
(par-map {:limit 8} (fn [x] ...) xs)     ; map over a list in parallel, reads only
(attempt expr)            ; (ok value), or (err {:kind K :message m}) if a tool call failed
(retry {:delays [1s 2s 4s]} expr)        ; run again after a transient failure
(retry {:backoff [100ms 5s] :within 30s :on [Timeout]} expr)
(wait-until {:within 2m :every 5s} (fn [v] done?) expr)
(sleep 10s)               ; wait a literal duration, as one step: [_ (sleep 10s)]
(loop {:max 20} [x init y init2]         ; bounded loop
  (if (done? x) result (recur next-x next-y)))
```

- `retry` evaluates `expr` again after each delay while it fails with a retried kind. Without
  `:on` it retries `Timeout`, `Unavailable`, and `RateLimited`. When the delays run out, the last
  failure ends the program. Wrap the `retry` in `attempt` to handle that failure instead. Retry
  a write that is not idempotent only with refusal kinds: `:on [RateLimited]`, not `Timeout`.
- `wait-until` evaluates `expr`, and repeats after each interval until the function returns
  `true` for its value, then returns that value. It fails with kind `Timeout` after `:within`.
- `loop` binds like `let`. `(recur v1 v2 ...)`, as the last step of the body, runs the body again
  with new values. Any other result ends the loop. `:max` caps the number of runs, and exceeding
  it ends the program with an error.
- Error kinds: `Timeout`, `Unavailable`, `RateLimited`, `NotFound`, `PermissionDenied`,
  `Invalid`, `Conflict`, `Failed`. Match on a kind with `(match (get e :kind) Timeout ... _ ...)`.

## Functions

- Arithmetic: `(+ a b)`, `(- a b)`, `(* a b)`, `(quot a b)`, `(rem a b)`
- Comparison: `(= a b)`, `(not= a b)`, `(< a b)`, `(<= a b)`, `(> a b)`, `(>= a b)`
- Logic: `(not x)`, `(and a b)`, `(or a b)`
- Arithmetic: `(- x)` negates.
- `(str a b c ...)` joins strings, integers, booleans, and enum values: `(str "n=" 3)` is `"n=3"`.
- `(count xs)`, `(first xs)`, `(rest xs)`, `(nth xs i)`, `(take n xs)`, `(concat xs ys)`,
  `(flatten xss)`, `(contains? xs x)`. `first` and `nth` stop the program on a list that is too
  short, so check `(count xs)` first when it may be.
- `(identity x)` returns `x`.
- Maps, `Map<String, V>`, are JSON objects with string keys, and always list in key order: `(get m "k")`
  is optional, `(get m "k" default)` is not, `(assoc m "k" v)`, `(dissoc m "k")`, `(keys m)`, `(vals m)`,
  `(entries m)` (`[key value]` tuples), `(into-map pairs)` (a later pair wins), and `count` and
  `contains?` (a key) take a map. `{}` starts an empty one: `(reduce (fn [m x] (assoc m ...)) {} xs)`.
- `(map f xs)`, `(filter f xs)`, `(any? f xs)`, `(all? f xs)`, `(reduce f init xs)`
- `(keep f xs)` applies `f`, which returns an optional, and keeps the `some` values unwrapped.
- `(sort-by f xs)` sorts ascending by a key: an integer, a string, or a tuple of keys compared in
  order. Sort by count descending, then name: `(sort-by (fn [r] [(- (get r :n)) (get r :name)]) rs)`.
- `(distinct-by f xs)` keeps the first element for each key.
- `(group-by f xs)` returns `[key items]` tuples in first-seen order. Take one apart with
  `(fn [[key items]] ...)`.
- A function argument `f` is a `(fn [x] ...)` or a function name such as `text.trim` or `+`.
- Tool tasks may list more functions, such as `text.split`. Call them as `(text.split s ",")`.

## Examples

Names of active users, alphabetical:

```sx
(let [users (call people.list {:team "infra"})]
  (sort-by (fn [name] name)
    (map (fn [u] (get u :name))
         (filter (fn [u] (get u :active)) users))))
```

Result: `["ada" "lin"]`

Each host's uptime, or `-1` when the lookup fails:

```sx
(par-map {:limit 8}
  (fn [host]
    (match (attempt (call metrics.uptime {:host host}))
      (ok m) {:host host :uptime (get m :seconds)}
      (err e) {:host host :uptime -1}))
  ["db1" "db2" "web1"])
```

Result: `[{:host "db1" :uptime 100} {:host "db2" :uptime 200} {:host "web1" :uptime -1}]`

Total size of every file across numbered pages:

```sx
(loop {:max 50} [page 1
                 total 0]
  (let [result (call storage.files {:page page})
        size (reduce (fn [sum f] (+ sum (get f :bytes))) 0 (get result :files))]
    (if (get result :last)
      (+ total size)
      (recur (+ page 1) (+ total size)))))
```

Result: `35`
