## Writing the program

The program is the body of an async function: `await` and `return` work at the top level, and the
returned value must be JSON. Annotations are erased and optional: `T[]`, `Record<string, T>`, `{ k: T }`, `T | null`; no index signature `{ [k: string]: T }`, and no `as` except `JSON.parse(t) as T`. The host checks the whole program before anything
runs, and rejects what it cannot give TypeScript's exact meaning. Each rule below is do / don't.

Calls and errors
- Call tools only as `tools.ns.name({ ... })` with `await`; `tools` is not a value (no `const t = tools`, no `tools[x]`).
- A failed call throws a CallError. `try { await tools.fs.readText({ path }) } catch (e) { e.message }`; no `finally`.
- Only `throw new Error("text")` or rethrow `e`. Do not throw other values.
- Reads may run together: `await Promise.all([a(), b()])`, `await parMap({ limit: 4 }, xs, (x) => ...)`. Do not put a write, a `console.log`, or an assignment in a parallel branch; write one by one in a `for (const x of xs)` with `await`.
- `Promise.allSettled` gives `.status`, `.value`, `.reason`. A tool-returned Settled reads the same way.

Loops
- Do not use `while`, `do`, `for (;;)` or `for...in`. Use `for (const x of xs)`, or a counted `for (let i = 0; i < n; i++)` that does not assign `i`.
- Poll or retry with `retry({ delaysMs: [100, 200] }, () => ...)` and `waitUntil({ everyMs: 1000, withinMs: 60000 }, () => poll, (v) => v.done)`; the bounds must be literals.
- Page with `loop({ max: 20 }, { cursor: null, all: [] }, ({ cursor, all }) => { ...; return next({ cursor: c2, all: all2 }); /* or */ return done(all2); })`. `next` must name every key.

Absent values and equality
- There is no `undefined`. Test with `x == null` or `x != null`, never `x === null` or `x !== null`.
- `x?.f` only as `x?.f ?? d`, `x?.f == null`, or a condition. Otherwise write `x == null ? null : x.f`.
- A null test narrows, as in TypeScript: `if (x == null) return …;` (or `continue`, `break`, `throw`) narrows `x` for the rest of the block; so do `x != null && x.f`, `x == null || x.f`, `x ? x.f : d`, and the else of `x == null`. A field of a name narrows too (`r.note != null && r.note.length`) until the name is assigned.
- `if (x)` on a string, number, or boolean proves it present only: its false branch is not narrowed, since `""`, `0`, and `false` are present.
- `===` compares strings, numbers, booleans only. Compare fields, not arrays or objects (`a.id === b.id`). `==` and `!=` only against `null`.
- A tool's optional field is `T | null`: test it (`r.note != null && r.note.length > 0`) or use `r.note ?? ""`.
- A payload enum is `{ tag: "A"; value: T } | "B"`: test `s === "B"` or `s.tag === "A"`, then read `s.value`. `s.value` before a tag test, and `s === "A"` for a variant with a value, are rejected. Write a field such as `age-seconds` as `r["age-seconds"]`.

Numbers and text
- An integer literal is an `Int`, `1.5` a `Float`. `/` always gives a `Float`: use `Math.floor(a / b)` where an index or `Int` is needed.
- An `Int` beyond 2^53 fails the run. `Math.floor/ceil/round/trunc/abs/max/min` work; `parseFloat`, `Number(x)`, `toFixed`, `NaN`, `**`, bitwise do not. `parseInt(s, 10)` needs the radix.
- `+` joins text only with a string, number, or boolean (`"n=" + n`); use `JSON.stringify(x)` for objects and arrays.
- `xs[i]` or a destructured name past the end fails the run; check `xs.length` first.
- No regular expressions, `Date`, `Set`, `Map`, `class`, `switch`, `import`, `Math.random`, or `localeCompare`. Use `includes`, `startsWith`, `split(sep)`, `replaceAll(a, b)`, `trim`, `toLowerCase`.

Arrays and objects
- `.sort()` is for strings only. For numbers or keys: `xs.sort((a, b) => a.n - b.n)`, exactly `k(a) - k(b)`.
- Objects joined by a branch, a `?:`, or an array need the same fields: `{ id, note: null }` in every branch.
- `const m = {}; for (const k of keys) { m[k] = v; }` makes a map: all values one type, `m[k] ?? d` reads it, `Object.keys(m).sort()` lists it. Use a variable key, not `m["a"] = v`. Do not log or stringify a map, or loop over its entries in order.
- Spread only a name whose fields are known: `{ ...row, done: true }`; not a call result, a map, or `?.`.
- Change variables only as statements: `x = e`, `x += 1`, `xs.push(e)`, `o.k = e` on an array or object you created. No `pop`, `shift`, `splice`, no assignment inside an expression or a closure that outlives the loop.
- Functions are not recursive, and are not returned, stored, or passed to a tool. Use `for...of` or `loop`.
- `JSON.parse(text) as { a: Int; b: string }` needs that annotation; a document of another shape throws.

Output
- `console.log(a, b)` appends one line to `logs`, which come back with the result; objects print as JSON. Only `console.log`.
- `return` the final answer; a program with no `return` returns null.
