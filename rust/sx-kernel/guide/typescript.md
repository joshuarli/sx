## Coming from TypeScript

You know TypeScript well. Each habit below has an Sx form. There are no statements, assignments, or
mutation: every form is an expression with a value, and the program's last value is its result.

| TypeScript | Sx |
| --- | --- |
| `const a = f(x); const b = g(a); return b;` | `(let [a (f x) b (g a)] b)` |
| `await api.ns.tool({ a: 1, b: "x" })` | `(call ns.tool {:a 1 :b "x"})` |
| `r.field` | `(get r :field)` |
| `r.maybe ?? fallback` | `(get r :maybe fallback)`, when `:maybe` is an `Option` |
| `{ repo, ahead: n }` | `{:repo repo :ahead n}` |
| `{ ...r, state: "done" }` | `{:repo (get r :repo) :state "done"}`, a new record with every field |
| `c ? a : b`, `if (c) {...} else {...}` | `(if c a b)`, and the else is required |
| `if / else if / else` | `(cond c1 a c2 b :else d)` |
| `switch (e.kind) { case "NotFound": ... }` | `(match (get e :kind) NotFound a _ b)` |
| `xs.map(x => f(x))` | `(map (fn [x] (f x)) xs)` |
| `xs.filter(p)`, `xs.find(p)` | `(filter p xs)`, `(first (filter p xs))` after checking `(count ...)` |
| `xs.reduce((acc, x) => acc + x, 0)` | `(reduce (fn [acc x] (+ acc x)) 0 xs)` |
| `xs.some(p)`, `xs.every(p)` | `(any? p xs)`, `(all? p xs)` |
| `xs.length`, `s.length` | `(count xs)`, `(text.length s)` |
| `xs.includes(x)` | `(contains? xs x)` |
| `[...a, ...b]`, `[...xs, x]` | `(concat a b)`, `(concat xs [x])` |
| `xs.flatMap(f)` | `(flatten (map f xs))` |
| `xs.slice(0, n)`, `xs[i]` | `(take n xs)`, `(nth xs i)` |
| `xs.sort((a, b) => b.n - a.n \|\| a.name.localeCompare(b.name))` | `(sort-by (fn [r] [(- (get r :n)) (get r :name)]) xs)` |
| `new Map()`, `m.set(k, v)`, `m.get(k)` | `{}`, `(assoc m k v)` returns a new map, `(get m k)` is an `Option` |
| `Object.entries(m)`, `Object.fromEntries(pairs)` | `(entries m)`, `(into-map pairs)` |
| `` `${a}/${b}` `` | `(str a "/" b)` |
| `s.split("\n")`, `s.trim()`, `s.startsWith(p)` | `(text.split s "\n")`, `(text.trim s)`, `(text.starts-with? s p)` |
| `parseInt(s, 10)` | `(text.to-int s)`, an `Option<Int>` |
| `try { a } catch (e) { b }` | `(match (attempt a) (ok v) ... (err e) b)` |
| `await Promise.all(xs.map(async x => ...))` | `(par-map {:limit 8} (fn [x] ...) xs)`, for reads |
| `for (const x of xs) { await write(x); }` | `(map (fn [x] (call ...)) xs)`, one at a time, in order |
| `while (...) { ...; i++ }` | `(loop {:max 100} [i 0 acc []] (if done acc (recur (+ i 1) (concat acc [x]))))` |
| `await sleep(1000)` | `[_ (sleep 1s)]` as a `let` step |
| `x === null`, `x !== undefined` | `(match x (some v) ... none ...)` |

### Where Sx differs on purpose

- **A list holds one type.** TypeScript lets you build `{ok: true, value}` in one branch and `{ok: false}`
  in another, and narrows on `if (r.ok)`. In Sx, records joined from branches with different fields
  make each field that some lack an `Option` in all of them, and `(if (get r :ok) ...)` does not narrow
  it. Build every record with the same fields, keep the `attempt` result itself in the record and
  `match` it later, or split into separate lists before building records:

  ```sx
  (let [skus ["A-1" "B-2"]
        looked (par-map {:limit 8}
                 (fn [sku] {:sku sku :found (attempt (call shop.item {:sku sku}))})
                 skus)
        known (keep (fn [r] (match (get r :found) (ok item) (some {:sku (get r :sku) :item item}) (err e) none))
                    looked)
        unknown (keep (fn [r] (match (get r :found) (ok item) none (err e) (some (get r :sku))))
                      looked)]
    {:known (map (fn [k] (get (get k :item) :name)) known) :unknown unknown})
  ```

- **Nothing is mutated.** Where TypeScript pushes onto an array in a loop, Sx maps, filters, or reduces,
  or carries the growing value through `loop` and `recur`.
- **Optional values are explicit.** `none` is not `null`: a field or map entry that may be missing has an
  `Option` type, and you take it apart with `match`, a default, or `(if x ...)` on a name.
- **A tool failure stops the program** unless an `attempt` encloses the call, like an exception nobody
  catches. Wrap every call whose failure the task tells you to handle.
- **There is no regular expression.** Parse with `text.split`, `text.starts-with?`, and
  `text.to-int`, and check each part: `"main.c:9:18: error: x"` split on `": "` gives
  `["main.c:9:18" "error" "x"]`, and the first part split on `":"` gives `["main.c" "9" "18"]`.
