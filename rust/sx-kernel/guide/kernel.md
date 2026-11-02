# Sx

Sx is a small language for calling tools and combining their results. Sx looks like Clojure, but only
the forms and functions below exist.

## Programs

A program is a list of definitions, one per step, followed by one expression, the result:

```sx
(def users (call people.list {:team "infra"}))
(defn active? [u] (get u :active))
(def names (map (fn [u] (get u :name)) (filter active? users)))
(sort-by identity names)
```

`(def name expr)` binds a value and `(defn name [params] body)` binds a function. `def`s run in order
and see earlier names, and a `defn` may use any `defn` or `def` of the program, wherever it is written.
`(def _ expr)` runs a step for its effect. A function cannot call itself. Keep each
definition short: a step that grows long becomes two definitions or a helper `defn`.

## Values

- `true`, `false`. Integers: `42`, `-7`. Strings: `"text"`, with escapes `\"`, `\\`, `\n`, `\r`, `\t`.
- Floats: `1.5`, `2e3`. They never mix with integers: `(float n)` converts, `(round x)`,
  `(floor x)`, and `(ceil x)` give an Int, and `(/ a b)` divides Floats. Bytes are opaque.
- Lists: `[1 2 3]`. Tuples: `[1 "a"]`, a bracket literal with mixed types or taken apart as `[a b]`.
- Records: `{:name "ada" :age 36}`. Keywords only name fields; use strings such as `"failing"`
  for labels. Tool enum variants are written `Passing`; one that is not a capitalized name, such as
  `rush`, is written as a string: `"rush"`. Name your own values in lowercase, since a capitalized
  name that is also a variant is an error.
- Optional values: `(some x)` or `none`. There is no `nil` or `null`. A plain value works where an
  optional is expected. `(if x then else)` on an optional name tests whether it is present, and
  `x` is the plain value inside `then`. `(get r :field default)` reads a field and gives `default`
  when the field is `none`.
- Results: `(ok x)` or `(err e)`, produced by `attempt`.
- Durations, only in `retry`, `wait-until`, and `sleep`: `250ms`, `5s`, `2m`, `1h`.
- `unit` is the Unit value, for a branch or program with no result.
- Comments start with `;`.

## Forms

```
(let [a expr1
      b expr2             ; later bindings see earlier ones
      _ expr3             ; run for effect
      f (fn [x] ...)]     ; a local function, called as (f x)
  body)                   ; one body
(if test then else)       ; test is true or false, or an optional name
(when test expr)          ; (some expr) when test holds, else none: (def _ (when failed (call ...)))
(do e1 e2 last)           ; runs each in order, gives last
(cond t1 r1 t2 r2 :else r)
(get record :field)
(call tool.name {:field value})
(fn [x] body)             ; as an argument or a binding
(match value
  (some x) a              ; patterns: (some x) none (ok x) (err e) Variant (Variant x),
  none b                  ;           literals such as 1 or "a",
  _ c)                    ;           and _ or a name, which match anything
(par e1 e2)               ; tool reads in parallel, as a tuple: (def [a b] (par e1 e2))
(par-map {:limit 8} (fn [x] ...) xs)     ; map in parallel, reads only
(attempt expr)            ; (ok value), or (err {:kind K :message m}) if a tool call failed
(retry {:delays [1s 2s 4s]} expr)
(retry {:backoff [100ms 5s] :within 30s :on [Timeout]} expr)
(wait-until {:within 2m :every 5s} (fn [v] done?) expr)
(sleep 10s)               ; as a step: (def _ (sleep 10s))
(fail "message")          ; stops the program; attempt catches it as kind Failed, and (fail e) raises a caught e again
(log "text" value)        ; adds a line to the run's logs, gives unit: (def _ (log "found" n))
(loop {:max 20} [x init y init2]
  (if (done? x) result (recur next-x next-y)))
```

- `retry` runs `expr` again after each delay while it fails with a retried kind, by default
  `Timeout`, `Unavailable`, and `RateLimited`. When the delays run out, the failure ends the
  program unless an `attempt` around the `retry` catches it. Retry a write that is not
  idempotent only on refusals such as `:on [RateLimited]`.
- `wait-until` runs `expr` until the function returns `true` for its value, waiting each interval
  in between, and returns that value. After `:within` it fails with kind `Timeout`.
- `loop` binds like `let`. `(recur v1 v2 ...)` as the last step runs the body again with new
  values; any other result ends the loop. More than `:max` runs ends the program with an error. A
  binding may start as `[]` and grow through `recur`.
- Error kinds: `Timeout`, `Unavailable`, `RateLimited`, `NotFound`, `PermissionDenied`,
  `Invalid`, `Conflict`, `Failed`: `(match (get e :kind) Timeout ... _ ...)`.

## Functions

- `(+ a b)`, `(- a b)`, `(* a b)`, `(quot a b)`, `(rem a b)`, and `(- x)` negates.
  `(= a b)`, `(not= a b)`, `(< a b)`, `(<= a b)`, `(> a b)`, `(>= a b)`. `(not x)`, `(and a b)`, `(or a b)`.
- `(str a b ...)` joins strings, numbers, booleans, and enum values.
- `(to-json x)` writes any value as compact JSON text, with record fields and map keys in ascending
  order, for `fs.write-text` or a tool that takes JSON text.
- `(count xs)`, `(first xs)`, `(rest xs)`, `(nth xs i)`, `(take n xs)`, `(concat xs ys)`,
  `(flatten xss)`, `(contains? xs x)`, `(identity x)`. `first` and `nth` stop the program on a list
  that is too short.
- `(map f xs)`, `(filter f xs)`, `(any? f xs)`, `(all? f xs)`, `(reduce f init xs)`, and
  `(keep f xs)`, which keeps the `some` results of `f`, unwrapped.
- `(sort-by f xs)` sorts ascending by an Int, String, or tuple key. By `:n` descending, then name:
  `(sort-by (fn [r] [(- (get r :n)) (get r :name)]) rs)`.
- `(distinct-by f xs)` keeps the first element per key. `(group-by f xs)` returns `[key items]`
  tuples in first-seen order.
- Maps, `Map<String, V>`, list in key order. `(get m k)` is optional and `(get m k default)` is
  not. `(assoc m k v)`, `(dissoc m k)`, `(keys m)`, `(vals m)`, `(entries m)` as `[key value]`
  tuples, `(into-map pairs)`. `{}` is an empty map.
- `(put r "field" v)` is a record with one field replaced, keeping its type; on a Map it is `assoc`.
- `(range start end)` and `(range start end step)` list the Ints from `start` up to but not including `end`,
  with a nonzero integer literal `step`. `(enumerate xs)` pairs each element with its index from 0.
- `(safe-int n)` is `n` when it lies within plus or minus 2^53, and stops the program otherwise.
- `f` is a `(fn [x] ...)`, a `defn` name, or a name such as `text.trim` or `+`.

## Examples

Each host's uptime, or `-1` when the lookup fails:

```sx
(defn uptime [host]
  (match (attempt (call metrics.uptime {:host host}))
    (ok m) {:host host :uptime (get m :seconds)}
    (err e) {:host host :uptime -1}))
(par-map {:limit 8} uptime ["db1" "db2" "web1"])
```

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
