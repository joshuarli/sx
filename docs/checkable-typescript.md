# Checkable TypeScript, draft v0

This is the reference specification for a TypeScript front end to Sx. It is written for two readers: the
implementer of the front end, and the author of the model-facing guide, who turns it into a short
manual. It says what the front end accepts, what each accepted construct lowers to, and what it rejects.
`docs/language.md` defines the Sx forms the lowering targets; this document does not repeat them.

A program is TypeScript with its annotations erased. The front end parses it, rejects whatever is outside
this specification, and builds a `syntax::Node` tree, with spans into the TypeScript source, that
`checker::check` consumes as it does for a program read from Sx text. Nothing below the tree changes
except what [the new kernel surface](#js-mode-and-the-new-kernel-surface) lists. The IR the checker emits is what
Lean judges.

## The fidelity rule

An accepted program means exactly what the JavaScript left after erasing its annotations means. The front
end does not run `tsc`, and a program `tsc` would reject but this specification accepts still means its
erased JavaScript. A construct whose JavaScript meaning Sx cannot reproduce is rejected with a
diagnostic, and is never reinterpreted.

Two exceptions are the only places the lowered program may differ from JavaScript, and both fail the run
instead of continuing with a different answer:

- **A trap** is a case where JavaScript produces a value that Sx cannot hold: `NaN`, an infinity, `undefined`
  from an out-of-range read, a number beyond ±2^53, a lone surrogate. The run stops with a failure that
  `try` cannot catch. [Traps](#traps) lists every one.
- **A refinement** is a case where JavaScript leaves the answer to timing, such as which of two failing
  branches of `Promise.all` is reported, or the order in which concurrent reads start. The lowered program
  picks one answer that JavaScript also allows: the failure of the lowest-numbered branch.

A program that traps has no JavaScript meaning to compare against. The differential tests of Phase 19
treat a run in which Sx traps as agreeing only when the fixture says the trap is expected.

## Traps

| Trap | JavaScript produces | Failure class |
| --- | --- | --- |
| `Int` result outside ±2^53, or an `Int` decoded outside it | A rounded double | `arithmetic` |
| `/`, `%`, or another `Float` operation that is infinite or not a number | `Infinity`, `NaN` | `arithmetic` |
| `xs[i]`, or destructuring past the end of a list | `undefined` | `out-of-range` |
| `Math.floor`, `ceil`, `round`, `trunc` of a value outside the `Int` range | A large double | `out-of-range` |
| A string slice or index that cuts a surrogate pair | A lone surrogate | `arithmetic` |
| `parseInt(s, 10)` when `s` has no leading digits | `NaN` | `arithmetic` |
| `s.value` of a rejected `Settled` | `undefined` | `out-of-range` |
| `loop` reaching its `max` | Not applicable, `loop` is a builtin | `loop-limit` |

A trap is not an exception. `catch` does not see it, `retry` does not repeat it, and a program cannot
test for it first except by the guard the failure class names.

## Programs

A program is a sequence of statements, the body of an `async` function. Top-level `await` and `return`
are allowed. The value of the program is the value of the `return` it reaches, and a program that reaches
none returns `null` (the Sx `unit`). The result must have a [JSON encoding](language.md#values-and-json);
`undefined`, functions, and `Map` values with `Option` entries do not.

Declarations of functions are hoisted as Sx `defn` forms are: a top-level function may be used before its
declaration, and a cycle among declarations is rejected. `import`, `export`, and `require` are rejected.

The output of a run is its result, the `logs` that `console.log` wrote in order, and the trace of tool
calls. See [console.log](#output).

## Grammar

The accepted language is the following, and nothing else. A construct in the left column that does not
appear here is rejected with the code `ts-unsupported`, naming the construct. Rows in the diagnostics
table give a more specific code and fix for the constructs models are likely to write.

### Statements

| Statement | Accepted form |
| --- | --- |
| Declaration | `const` or `let` with an identifier, object pattern, or array pattern. An initializer is required unless every path assigns the name before it is read (`let t; try { t = await f(); } catch { t = ""; }`); otherwise `ts-uninitialized`. |
| Expression | A call, `await` call, assignment in a [mutation pattern](#mutation), `++` or `--` |
| `if` | `if (c) S`, `if (c) S else S`, any nesting |
| `for…of` | `for (const p of e) S` with `const` or `let` and an identifier or pattern |
| Counted `for` | `for (let i = a; i < b; i++) S`, see [Loops](#loops) |
| `try` | `try { } catch { }` or `catch (e) { }`; no `finally` |
| `throw` | `throw new Error(e)` where `e` is a string, or `throw e` for the `catch` parameter |
| `return` | `return`, `return e` |
| `break`, `continue` | In a `for…of` or counted `for`, unlabeled |
| Block | `{ S* }` |
| Function declaration | `function f(p, …) { }`, `async function`; not recursive |
| Type declaration | `type T = …` and `interface T { … }`; erased |
| Empty | `;` |

### Expressions

| Expression | Accepted form |
| --- | --- |
| Literals | numbers (see [Numbers](#numbers)), strings, `true`, `false`, `null`, template literals with `${e}` |
| Array | `[e, …]` and `[...e, …]` for arrays |
| Object | `{ k: e, k, "k": e, … }` with identifier or string keys that are not integer-like, no duplicates, no computed keys, no spread, no methods |
| Names | identifiers, and `tools` only as `tools.namespace.name(…)` |
| Member | `e.k`, `e["k"]` with a string literal, `e[i]`, and the same with `?.` under the [optional-chain rule](#optional-values) |
| Call | `e(…)` of a [builtin](#builtins), a user function, a tool, or a method in the builtins table |
| `await` | `await e` where `e` is a call; see [Async](#async) |
| Arrow | `(p, …) => e`, `(p, …) => { }`, `async` arrows; as an argument or the initializer of a `const` |
| Unary | `!`, `-`, `+` on numbers, `await`, `typeof` (see below) |
| Binary | `+ - * / %`, `< <= > >=`, `=== !==`, `== null`, `!= null`, `&&`, `||`, `??` |
| Conditional | `c ? a : b` |
| Assignment | `=`, `+=`, `-=`, `*=`, `/=`, `%=`, only as a statement in a [mutation pattern](#mutation); `||=`, `&&=`, `??=` are rejected |
| Type syntax | `as T` directly on `JSON.parse(…)`; annotations on bindings, parameters, and returns; generic arguments on `Array`, `Record`, `Promise`; all erased except in `JSON.parse` |
| Parentheses | `( e )` |

Patterns are identifiers, `{ a, b: c, d: [e, f] }`, and `[a, b, [c, d]]`, with no defaults, no rest, and no
computed keys. Patterns appear in declarations, `for…of` heads, and parameters.

### Type annotations

An annotation on a binding, parameter, or return is parsed and erased. Its text never changes what a
program means or whether it is accepted. The annotation grammar is the following, and an annotation
outside it is `ts-annotation`:

```
Type = "string" | "number" | "boolean" | "null" | "Int" | "Float" | StringLiteral
     | Type "[]" | "Array<" Type ">" | "readonly " Type "[]" | "[" Type {"," Type} "]"
     | "Record<string," Type ">" | "{" {Key ["?"] ":" Type [";"|","]} "}"
     | Type "|" Type | Name | "Promise<" Type ">" | "unknown" | "any" | "void"
```

The annotation of a `JSON.parse` target is the one place a type is read. See [JSON](#json).

## Numbers

A JavaScript number is a double. Sx has `Int` and `Float`. A program under this front end runs in
**JS mode**, a flag of the checker that the front end sets, and that makes the following hold. Every
rule elaborates into the existing IR: a promotion is the `float` function, and a range check is the new
`safe-int` node.

| Rule | Meaning |
| --- | --- |
| Literals | A digits-only literal (`7`, `0x1f`, `1_000`) is an `Int`. A literal with `.` or an exponent (`1.5`, `2e3`, `1.0`) is a `Float`. An `Int` literal beyond 2^53 is `ts-unsafe-integer`. |
| Safe integers | Every `Int` in a run lies in ±2^53. An `Int` result outside it, from `+`, `-`, `*`, or a decode, is the `arithmetic` trap. So every `Int` is exactly the double JavaScript holds. |
| Promotion | Where the checker expects `Float` and finds `Int`, the `Int` is converted. This holds for operands, call arguments, record fields, and branch results. The reverse never happens: a `Float` where an `Int` is expected is `js-float-as-int`, with the fix `Math.floor`, `Math.ceil`, `Math.round`, or `Math.trunc`. |
| Arithmetic | `+ - *` on two `Int`s is `Int`, with the safe-integer trap. If either operand is `Float`, both are `Float`. `/` is always `Float`, so `6 / 3` is `2.0`, and a use of it as an index is `js-float-as-int`. Unary `-` keeps the type. |
| Remainder | `%` on two `Int`s is `rem`: the result takes the sign of the dividend, as in JavaScript. A zero divisor is the `arithmetic` trap, where JavaScript gives `NaN`. `%` with a `Float` operand is `js-float-remainder`. |
| Comparison | `< <= > >= === !==` on `Int` and `Float` convert the `Int`. The conversion is exact because every `Int` is within ±2^53. `0.1 + 0.2 === 0.3` is `false` as in JavaScript. |
| Joins | Where branches, list elements, record fields, or a loop-carried variable meet with `Int` and `Float`, the join is `Float` and the `Int` side is converted. A mutated variable that starts as `0` and receives a `Float` is a `Float` throughout. |
| Formatting | A number written into a string by `+`, a template, `String(n)`, or `n.toString()` is `str`, which writes `Int` in decimal and `Float` as JavaScript's `String(x)`. `1.0` is `"1"`. |
| Rounding | `Math.floor`, `ceil` give `Int` from `Float`, and the identity from `Int`. `Math.trunc(x)` is `(if (< x 0.0) (ceil x) (floor x))`. `Math.round(x)` rounds halves toward positive infinity, not away from zero as Sx `round` does: it lowers to `(let [f (floor x)] (if (>= (- x (float f)) 0.5) (+ f 1) f))`, which is exact. |
| `Math.abs`, `max`, `min` | `abs` of an `Int` or `Float`; `max` and `min` of two arguments. They lower inline with their arguments bound once. Spread arguments are rejected. |
| Negative zero | Unobservable: `-0` is `0`, and no accepted construct tells them apart. |

`NaN`, `Infinity`, `Number.MAX_SAFE_INTEGER`, `isNaN`, `Number(…)`, `parseFloat`, `**`, and bitwise operators
are rejected.

## Strings

Sx strings are UTF-8 and the `text` core counts code points, where JavaScript counts UTF-16 code units
and compares by code unit. They differ only for characters beyond U+FFFF. The front end never uses
`text.length` or the `text` indexing functions. It uses the `js` core of [the new kernel surface](#js-mode-and-the-new-kernel-surface),
whose functions count and index UTF-16 code units and trap, as the table above says, where a cut would
split a surrogate pair. Equality is the same for both. In JS mode `<`, `<=`, `>`, `>=` and sorting on strings compare by UTF-16
code unit, not by code point.

`+` with a string operand is concatenation, `(str a b)`; the other operand must be a `String`, `Int`,
`Float`, or `Bool`. Any other operand type is `js-coercion`, because JavaScript would write
`[object Object]` or join array elements. `+` with two numbers is addition. A template literal is `(str …)`
with the same operand rule. `trim`, `toLowerCase`, and `toUpperCase` are `js` functions because the
`text` core's whitespace set and case mapping follow Python, not ECMAScript.

## Equality and truthiness

`===` and `!==` accept operands that are both primitives: `String`, `Int`, `Float`, `Bool`, or an `Option`
of one, where the other side may be the value, and `null` is `none`. Comparing arrays or objects is
reference equality in JavaScript and structural equality in Sx, so it is `js-equality`. `==` and `!=` are
accepted only against `null`, and mean the same as `=== null` here because no accepted value is `undefined`.

A condition (`if`, `?:`, `!`, and the operands of `&&` and `||`) is any expression, as in JavaScript,
and its truthiness is the type-directed `truthy` form:

| Type | `truthy` |
| --- | --- |
| `Bool` | itself |
| `String` | not the empty string |
| `Int`, `Float` | not zero |
| `Option<T>` | present, and `truthy` of the value when `T` is `Bool`, `String`, `Int`, or `Float` |
| `List`, `Tuple`, `Record`, `Map` | `true` (an empty array is truthy in JavaScript) |
| `Unit` | `false` |

`a && b` and `a || b` return one of their operands, as in JavaScript, so both sides have one type after
joins: `(let [a a] (if (truthy a) b a))` and `(let [a a] (if (truthy a) a b))`. `b` is evaluated only
when JavaScript would evaluate it.

## Optional values

`null` is `none`. `undefined` does not exist: the literal, `=== undefined`, `typeof x === "undefined"`, and
`void` are `ts-undefined`. A tool's optional output field and an absent value are `null` in the
JavaScript a program means. An optional input field may be omitted or `null`; `null` is omitted from
the call.

| Source | Lowering |
| --- | --- |
| `x ?? d` | `(get x d)` when `x` is a bare name or an optional field, else `(match x (some v) v none d)`; `d` runs only when `x` is absent |
| `x == null`, `x === null` | `(not x)` as a presence test |
| `x != null`, `x !== null` | `x` as a presence test |
| `x?.f` | `(match x (some v) (get v :f) none none)` |
| `x?.[i]`, `x?.f()` | the same, with the index or the method |

The result of `?.` is `undefined` in JavaScript when `x` is `null`, and no accepted value is `undefined`.
So a `?.` chain may appear only where `undefined` and `null` behave alike:

- as the left operand of `??` or `||`;
- as an operand of `== null` or `!= null`;
- as an operand of `===` or `!==` against a literal that is not `null`;
- as a condition: `if`, `?:`, `!`, or the left of an `&&` that is itself a condition;
- as the value of a property of the object literal passed to a tool, where `null` and `undefined` are both left out;
- as the initializer of a `const` or `let` whose every use is one of the above.

`=== null` on a chain is rejected, because `undefined === null` is false. Any other use is `ts-optional-chain`,
with the fix `x == null ? null : x.f`. A plain `.f` on an `Option` is a `type-mismatch`, in TypeScript terms
"possibly null".

`typeof e` is decided by the type of `e`, which is static: `"string"` for a `String`, `"number"` for an
`Int` or `Float`, `"boolean"` for a `Bool`, `"object"` for a list, tuple, record, or map, and for an `Option`
the type of the value when present and `"object"` when absent (`typeof null`). It is elaborated by the checker.
`typeof` of a chain, a comparison with `"undefined"`, and `instanceof` are rejected. `Array.isArray(e)` is
likewise `true` for a list or tuple and `false` for any other type.

## Lowering of statements

The front end translates a statement list into one Sx expression by continuation: `S(statements)` is the
expression for the first statement with `S(rest)` as its continuation. Every name a program declares is
bound once, in a `let`, so the lowered code is single-assignment. A reassignment becomes a new binding of
the same name, as the [mutation rules](#mutation) say.

| Statement | Lowering |
| --- | --- |
| `const p = e; rest` | `(let [p e] S(rest))`; adjacent declarations share one `let` |
| `e; rest` | `(let [_ e] S(rest))` |
| `if (c) A else B; rest` | When neither branch assigns a variable or exits: `(let [_ (if (truthy c) S(A) S(B))] S(rest))` with both branches' last values joined to `unit`. When `A` or `B` assigns variables, the `if` returns their new values and `rest` sees them: `(let [[x y] (if c [x1 y1] [x y])] S(rest))`. |
| `if (c) { …; return v } rest` | When a branch ends in `return`, the other branch and `rest` are the else branch: `(if (truthy c) S(A) S(B; rest))` |
| `return e` | the value of the enclosing function or program, only in tail position as above or through the [exit slot](#loops) |
| `function f(p) { … }` | a hoisted `defn`; see [Functions](#functions) |
| `type`, `interface` | erased |

Branches of an `if` that end in `throw` have no value and join with any type.

### Loops

| Source | Lowering |
| --- | --- |
| `for (const p of xs) { B }` with no assignment to an outer variable | `(let [_ (map (fn [p] S(B)) xs)] S(rest))` |
| The same, assigning outer variables `v1 … vn` | `(let [[v1 … vn] (reduce (fn [[v1 … vn] p] S(B; [v1 … vn])) [v1 … vn] xs)] S(rest))`; one variable is bound without the tuple |
| `for (let i = a; i < b; i++) B` | the `for…of` form over `(range a b)` with the loop variable `i` |
| `<=` | the bound is `(+ b 1)` |
| `i += k`, `i -= k` with `k` an integer literal | `(range a b k)` or `(range a b (- k))` |
| `i--` with `>` or `>=` | `(range a b -1)` |
| `continue` | the loop body so far returns `[v1 … vn]` at that point: `if (c) continue; R` is `(if c [v1 … vn] S(R))` |
| `break` | a flag `done` joins the carried variables: `(reduce (fn [[done v1 …] p] (if done [done v1 …] S(B))) [false v1 …] xs)`; `break` is `[true v1 …]`. Iterations after `break` evaluate nothing, so no later call is made. |
| `return e` inside a loop | the flag and a `ret` slot: the loop carries `[done ret v1 …]` and `return e` is `[true (some e) v1 …]`; after the loop `(match ret (some r) r none S(rest))` |

A counted `for` is accepted only when:

1. it declares one `let` counter initialized by an expression `a`;
2. the test is `i < b`, `i <= b`, `i > b`, or `i >= b`, and the update is `i++`, `i--`, `i += k`, or `i -= k` with
   `k` a positive integer literal, in the direction the test implies;
3. the body never assigns `i`;
4. `b` is loop-invariant: a literal, or an expression over names that the body does not assign and calls
   nothing with an effect. `i < xs.length` is accepted when the body does not assign `xs` or push to it.

JavaScript evaluates `b` on every iteration, and `(range a b)` evaluates it once. Rule 4 makes those the
same. A counter that is a `Float` is `ts-counted-loop`.

`while`, `do … while`, `for (;;)`, and a `for` outside the shape above are `ts-while`, whose fix names the
two bounded forms:

- polling or retrying a call: `retry({ … }, () => call)` and `waitUntil({ … }, poll, done)`;
- paging or walking until a condition: `loop({ max: N }, state, (s) => … next(s2) … done(v))`.

`for…of` accepts only a list. A string, a `Map`, a `Set`, or a generator is `js-for-of-source`. `for…in` is
`ts-for-in`.

### Mutation

JavaScript lets a program change a variable, an array, or an object in place. Sx values are immutable. The
front end accepts the following patterns and rewrites each into a new binding, and rejects every other
assignment.

| Pattern | Rewrite | Condition |
| --- | --- | --- |
| `x = e`, `x += e`, `x -= e`, `x *= e`, `x /= e`, `x %= e`, `x++`, `x--` as a statement | `x` is rebound to the new value | `x` is a `let` of the same function |
| `v.push(e1, …, ek)` as a statement | `v` is rebound to `(concat v [e1 … ek])` | `v` is an owned array |
| `v.f = e`, `v["k"] = e`, `v.f.g = e` as a statement | `v` is rebound to `(put v "f" e)`, nested for deeper paths | `v` is an owned object |
| `m[k] = e` with a computed key | `m` is rebound to `(put m k e)` | `m` is an owned object whose type is a `Map` |
| `v.sort(c)`, `v.reverse()` as a statement or an expression | `v` is rebound to the sorted or reversed list, which is also the value of an expression | `v` is an owned array; see [Builtins](#builtins) |
| `++` or `--` in the update of a counted `for` | part of the loop form | |

An **owned** variable is a `const` or `let` whose initializer is an array literal, an object literal, or an
expression that builds a fresh value: `[]`, `{}`, `.map(…)`, `.filter(…)`, `.slice(…)`, `[...a]`,
`Object.fromEntries(…)`, `JSON.parse(…)`, `await Promise.all(…)`, or a tool call. Each tool call returns a new value. `put` is the new form that updates a record field the record already has, or sets
a `Map` key.

An assignment is accepted only when all of the following hold. The first failing check gives the code:

1. **The pattern is in the table.** Assignment inside a larger expression (`a[i++]`, `x = y = 0`,
   `if ((n += 1) > 3)`), destructuring assignment, `??=`, `||=`, `&&=`, and a write to a parameter or to a value
   that is not owned are `ts-mutation-pattern`. `pop`, `shift`, `unshift`, `splice`, `fill`, `copyWithin`, and
   `Object.assign` are `ts-mutating-method`.
2. **The variable does not escape into a closure.** A function declaration, or an arrow that is stored in a
   `const`, may not mention an assigned variable at all. An arrow written inline as an argument to a
   [builtin](#builtins) may read an assigned variable, because the builtin runs it before it returns, and it
   sees the binding that is current at that point, which is what JavaScript shows it. No function may assign
   an outer variable. Otherwise `ts-mutation-escape`.
3. **An owned variable is not aliased.** The name of an owned variable may not be stored, while a later
   mutation of it can run, in another binding, an array or object literal, the argument of `push`, or the
   argument of a user function. It may be the receiver of a builtin that builds a new value, a `for…of`
   source, the argument of a tool call or of `console.log` or `JSON.stringify`, or the operand of `return`.
   A store is safe when no mutation of the variable appears after it in the function body and it is not
   inside a loop that contains a mutation. Otherwise `ts-mutation-escape`.
4. **A `try` does not lose an assignment.** An assignment inside a `try` block must follow the last operation
   in the block that can throw: a tool call, a `throw`, `JSON.parse`, or a call of a user function that does
   either. A `catch` that ran after an earlier assignment would otherwise see a value the lowered `attempt`
   no longer has. Otherwise `ts-mutation-in-try`.
5. **A parallel branch does not assign.** The function passed to `parMap` and the callbacks of
   `Promise.all` and `Promise.allSettled` assign nothing outside themselves. Otherwise `ts-mutation-escape`.

The rewrite carries the new value through every join. An assignment in one branch of an `if`, in a `for…of`
body, or in a `try` or `catch` block makes the variable part of the tuple that construct returns, as in
the [loop table](#loops).

### Errors

A failed tool call throws a `CallError`, an `Error` with `kind` and `message`. The kinds are those of
[Call errors](language.md#call-errors).

| Source | Lowering |
| --- | --- |
| `try { T } catch (e) { C }; rest` | `(let [r (attempt S(T; value))] (match r (ok v) S(rest with v) (err e) S(C; rest)))`, where `T` and `C` return the variables they assigned, as in a join |
| `try { T } catch { C }` | the same with `_` for the error |
| `throw new Error(m)` | `(fail m)` |
| `throw e`, in the `catch` that binds `e` | `(fail e)`, which raises the same `CallError` |
| `e.message`, `e.kind` | `(get e :message)`, `(get e :kind)`; `e.kind` compares with the kind names as strings |
| `String(e)`, `` `${e}` ``, `e + ""` | `(str "CallError: " (get e :message))` |

`attempt` catches a failed call, a failed `wait-until`, a `fail`, and a core's refusal; see
[Failures](language.md#failures). That matches what a JavaScript `catch` sees for the accepted constructs,
once the JavaScript environment defines `CallError` as an `Error` subclass with `name` `"CallError"` and own
enumerable properties `kind` and `message`, in that order. So `JSON.stringify(e)` is `{"kind":…,"message":…}`
and `String(e)` is `CallError: message`, which is what Sx writes for the record.

An error that `new Error` throws has no `kind`, prints as `Error: message`, and stringifies to `{}`. A refusal
from `JSON.parse` has a message from the JavaScript engine that Sx cannot reproduce. So a `catch` is
either **tool-only** or **mixed**: it is mixed when its `try` block contains a `throw`, a `JSON.parse`, or a call
of a function that does.

- In a tool-only `catch`, the parameter `e` may be used as a value (stored, logged, returned, stringified)
  and through `e.message`, `e.kind`, and `String(e)`.
- In a mixed `catch`, `e` may be used as `e.message`, and as a condition (`e && e.message`), and as `throw e`.
  When the block has a `JSON.parse`, `e.message` is also rejected. Every other use is `ts-error-use`.
- `e` is always truthy, so a condition on it is accepted anywhere.
- `catch (e)` may not be destructured, and `finally` is `ts-finally`.
- A `break`, `continue`, or `return` inside `T` or `C` lowers through the same exit flags as in a loop.
- `throw` of anything but `new Error(string)` or the `catch` parameter is `ts-throw`. A thrown error that
  nothing catches stops the program with the failure class `thrown`, and its message.
- A trap, a `loop-limit`, and a budget failure are not errors in this sense and are never caught.

A `retry` or `waitUntil` that gives up throws the last `CallError`, which a `catch` sees.

### Functions

| Source | Lowering |
| --- | --- |
| `function f(a, b) { B }`, `const f = (a, b) => B` | `(defn f [a b] S(B))` at the top level; inside a function body or block, `(let [f (fn [a b] S(B))] …)` |
| `f(x, y)` | `(f x y)` |
| `xs.map(x => B)` | `(map (fn [x] B) xs)` |

A function has identifier or pattern parameters, with no defaults, no rest parameter, and no `this`. It is
called directly or passed as an argument to a builtin. A function that calls itself is `ts-recursion`.
A function that is returned, stored in an object or array, assigned to a variable, or passed to a tool is
`ts-function-value`: Sx cannot return or store a function. `async` is accepted anywhere and means nothing
beyond [Async](#async). Parameter and return annotations are erased.

### Async

Sx calls complete in order, and `await` is part of every call. So `await e` lowers to the lowering of `e`,
and `await` is accepted only in front of a tool call, a call of a user function, `sleep`, `retry`, `loop`,
`waitUntil`, `parMap`, `Promise.all`, or `Promise.allSettled`. A call of an async function or a tool
without `await` that is not directly an argument of `Promise.all` or `Promise.allSettled`, and any
`Promise` stored in a variable, is `ts-promise-value`.

| Source | Lowering |
| --- | --- |
| `await Promise.all([a, b, c])` | `(par a b c)`; `const [x, y, z] = …` is a tuple pattern |
| `await Promise.all(xs.map(f))`, `f` an arrow or function | `(par-map {:limit 16} f xs)` |
| `await Promise.allSettled([a, b])` | `(par (attempt a) (attempt b))`, a tuple of `Result` |
| `await Promise.allSettled(xs.map(f))` | `(par-map {:limit 16} (fn [x] (attempt (f x))) xs)` |

Every branch of these must have only `read` effects, which the checker already enforces with
`parallel-write`. A program that runs writes together writes them in a `for…of` with `await`; the
diagnostic says so. A `console.log` in a branch is `ts-log-in-parallel`, because the order of logs in
concurrent branches is not defined by JavaScript and a log must keep the program's order.

An element of an `allSettled` result is a `Settled<T>`, a value of the `js` core with the JavaScript shape:
`{"status":"fulfilled","value":v}` or `{"status":"rejected","reason":{"kind":…,"message":…}}` when written as
JSON. `s.status` is `"fulfilled"` or `"rejected"`; `s.value` is the value, and the `out-of-range` trap when the
promise was rejected, where JavaScript has `undefined`; `s.reason` is an `Option<CallError>`, present when
rejected. The elements lower to the `Result` of `attempt` and the accessors to `js.settled-status`,
`js.settled-value`, and `js.settled-reason`.

```ts
const [a, b] = await Promise.allSettled([tools.fs.readText({ path: p }), tools.fs.list({ path: d })]);
const text = a.status === "fulfilled" ? a.value : "";
```

A `Settled` may be passed whole to `JSON.stringify`, `console.log`, `return`, or stored, when no branch of
the `allSettled` can throw a plain error: that is, when the branches contain no `throw`, `JSON.parse`, or call
of a function that does, so every `reason` is a `CallError` with the JSON above. Otherwise the whole value is
`ts-settled-escape`, and only `.status`, `.value`, and `.reason.message` are accepted. Spreading a `Settled`
is `ts-spread`.

The limit 16 is the number of branches in flight and changes no result. JavaScript starts all branches at
once; for reads, the number in flight is unobservable except as latency. `parMap` sets another limit.

### Objects, arrays, and maps

An object literal is a `Record` with the fields it names, in the order it writes them. A static read
`o.f` or `o["f"]` is `(get o :f)`, and the checker rejects a field the record does not have. An object
whose keys are computed is a `Map<String, V>`. A `Map` arises from `Object.fromEntries(pairs)`, from a tool or
`JSON.parse` that declares `Record<string, V>`, and from `{}` that a program fills with `m[k] = v`.

JavaScript lists the keys of an object in insertion order, with integer-like keys first in numeric order.
A `Map` in Sx is always in ascending key order and does not remember insertion order, and a record read
from a tool has the key order of the type, not of the JSON the tool sent. So a program may not observe
the order of a `Map`, or of a record it did not build:

| Source | Accepted as | Else |
| --- | --- | --- |
| `m[k]` on a `Map` | `(get m k)`, an `Option` | |
| `m[k] = v` | `(put m k v)` | |
| `Object.keys(m).sort()` | `(sort-by identity (keys m))` | |
| `Object.keys(m).length` | `(count m)` | |
| `Object.keys(m)`, `Object.values(m)`, `Object.entries(m)` otherwise | | `ts-map-order` |
| `Object.keys(r)`, `Object.entries(r)` on a record | | `ts-record-enumeration`; write the fields |
| `JSON.stringify(x)` or `console.log(x)` where `x` holds a `Map` | | `ts-map-order` |
| `JSON.stringify(x)` or `console.log(x)` where `x` holds a tool's record | the record's fields in the type's declared order | the tool's order is assumed to be that order |

A `Map` as a program result or as a tool argument is fine, because a JSON object has no order that a reader
may rely on. An `Option` entry in a `Map` has no JSON encoding and is rejected by the checker.

A list is immutable; `push`, `sort`, and `reverse` follow the [mutation table](#mutation). Spread in an array
literal is `concat`; spread in a call and object spread are `ts-spread`. `xs[i]` is `(nth xs i)` with `i` an
`Int`, and past the end it is the `out-of-range` trap. `xs.length` is `(js.length xs)`. A destructuring of a
list takes `(nth xs 0)`, `(nth xs 1)`, and a destructuring of a `Tuple` takes its elements.

### Output

`console.log(a, b, …)` appends one string to the run's `logs`, then continues. The string is each argument
formatted and joined by single spaces. A `String` is itself, a number is `str`, a `Bool` is `true` or `false`,
`null` is `null`, and a record or list is `JSON.stringify` of it. This is the definition of the host's
`console.log`; it is not Node's `util.inspect`. Other `console` methods are `ts-console`.

`logs` is returned next to `result` in `run`'s `structuredContent`, with `trace`. Entries are in the order the
`console.log` calls ran, which is the order of the program because `console.log` is not allowed in a
parallel branch. A failed run keeps the logs up to the failure. Lowering: `(log a b)`, a form of type `Unit`
that evaluates its arguments and appends the text.

### JSON

`JSON.stringify(x)` is `(js.stringify x)`: compact JSON with record fields in the order of the record's type,
which is the order of the object literal that built it, then the `to-json` text rules for strings and numbers.
`JSON.stringify(x, null, 2)` is `(js.stringify x 2)`, indented by that many spaces as JavaScript does. A
replacer function, a replacer array, a string indent, and `undefined` values are `ts-json`. A `Map` is
`ts-map-order`, as above.

`JSON.parse(text)` yields a value the checker must type, and there is no `any`. The one accepted forms are
`JSON.parse(e) as T` and `const x: T = JSON.parse(e)`, where `T` is in the annotation grammar and is
none of `unknown`, `any`, or a union other than `X | null` and string literals. A type alias or interface
that names a type is followed. `number` means `Float`, and `Int` means `Int`; the generated declarations define both as
aliases of `number`. The lowering is `(js.parse T text)`, which decodes with the [type-directed decoder](language.md#values-and-json):
a field missing, an extra field, or a value of another type makes the call fail with `invalid-input`, as a
refusal that `catch` sees. JavaScript would not fail on a mismatched document, so the annotation is a
checked cast, and the program means what it means when the document has the declared shape. An unannotated
`JSON.parse` is `ts-json-parse-type`.

## Builtins

A method call needs the type of its receiver, and the checker resolves it: the front end emits a call of the
`js` core function, which has one signature for each receiver type it supports. A call of a method that is
not in this table is `ts-unknown-builtin`, naming the receiver and the method. `R` is the receiver.

| Source | Lowers to | Difference from JavaScript, or condition |
| --- | --- | --- |
| `R.length` | `(js.length R)` | List: `count`. String: UTF-16 code units. |
| `R.map(f)` | `(map f R)` | `f` takes `(x)` or `(x, i)`; the index form is `(map (fn [[i x]] …) (enumerate R))`. The same holds for `filter`, `flatMap`, `find`, `some`, `every`, and `forEach`. A third parameter is `ts-callback-arity`. `reduce` takes `(acc, x)`. |
| `R.filter(f)` | `(filter f R)` | same |
| `R.flatMap(f)` | `(flatten (map f R))` | `f` returns an array |
| `R.find(f)` | `(js.find f R)` | `Option<T>` |
| `R.some(f)`, `R.every(f)` | `(any? f R)`, `(all? f R)` | |
| `R.reduce(f, init)` | `(reduce f init R)` | `init` is required; without it, `ts-reduce-init` |
| `R.forEach(f)` | `(let [_ (map f R)] unit)` | statement only |
| `R.includes(x)` | `(contains? R x)` | `R` is a list of primitives; strings: `(text.contains? R x)` |
| `R.indexOf(x)` | `(js.index-of R x)` | `-1` when absent; list of primitives or strings |
| `R.join(sep)` | `(js.join sep R)` | `sep` is required. Elements are strings or numbers; an `Option` element is `ts-unknown-builtin`. |
| `R.slice(a, b)` | `(js.slice R a b)` | List or String; JavaScript's negative and clamped indices; string cuts trap on a surrogate pair |
| `R.concat(xs)` | `(concat R xs)` | one array argument |
| `R.sort()` | `(sort-by identity R)` | strings only, by UTF-16 code unit. Numbers sort as text in JavaScript, so `.sort()` on numbers is `ts-sort-comparator`. |
| `R.sort((a, b) => k(a) - k(b))` | `(sort-by (fn [a] k(a)) R)` | the comparator must be `k(a) - k(b)` or `k(b) - k(a)` with `k(b)` the same text as `k(a)` under the renaming of `a` to `b`, over numbers; descending is `(sort-by (fn [a] (- k(a))) R)`. The sort is stable in both. Others are `ts-sort-comparator`; `localeCompare` is `ts-sort-comparator` because it depends on the locale. |
| `R.reverse()` | `(js.reverse R)` | see [Mutation](#mutation) |
| `Array.isArray(x)` | `true` or `false` by the type of `x` | |
| `Array.from`, `Array(n)`, `new Array` | | `ts-unknown-builtin`, `ts-unsupported-class` |
| `R.startsWith(s)`, `R.endsWith(s)` | `(text.starts-with? R s)`, `(text.ends-with? R s)` | one argument |
| `R.split(sep)` | `(text.split R sep)` | `sep` is a non-empty string; no limit; a regular expression is `ts-regexp` |
| `R.trim()` | `(js.trim R)` | ECMAScript whitespace |
| `R.toLowerCase()`, `R.toUpperCase()` | `(js.to-lower R)`, `(js.to-upper R)` | Unicode default case mapping of the host's tables |
| `R.padStart(n, s)`, `R.padEnd(n, s)` | `(js.pad-start R n s)`, `(js.pad-end R n s)` | `n` in UTF-16 units; `s` required |
| `R.replace(a, b)` | `(js.replace-first R a b)` | `a` is a string, not a regular expression; `b` containing `$` fails with `invalid-input` |
| `R.replaceAll(a, b)` | `(text.replace R a b)` | the same restriction on `b`, checked when `b` is a literal and at run time otherwise |
| `String(x)`, `x.toString()` | `(str x)` | `String`, `Int`, `Float`, `Bool`. `toString(radix)` is rejected. |
| `parseInt(s, 10)` | `(js.parse-int s)` | the radix is required and is `10`; leading whitespace and a sign are read and the longest digit prefix is the value; no digits is the `arithmetic` trap |
| `Number.isInteger(x)` | `(js.is-integer x)` | |
| `Math.floor`, `ceil`, `round`, `trunc`, `abs`, `max`, `min` | see [Numbers](#numbers) | |
| `Object.keys(m).sort()`, `Object.keys(m).length`, `Object.fromEntries(p)` | see [Objects](#objects-arrays-and-maps) | `fromEntries` is `(into-map p)` |
| `JSON.stringify`, `JSON.parse` | see [JSON](#json) | |
| `console.log` | `(log …)` | see [Output](#output) |
| `Promise.all`, `Promise.allSettled` | see [Async](#async) | |
| `sleep`, `retry`, `loop`, `parMap`, `waitUntil` | see [Bounded forms](#bounded-forms) | |

Not available, each with its code: regular expressions and `match`, `test`, `exec` (`ts-regexp`); `Date`,
`Date.now`, and every `Date` method (`ts-date`); `Math.random`, `setTimeout`, `setInterval`, `Date.now`
(`ts-nondeterministic`); `Set`, `Map`, `WeakMap`, `Symbol`, `BigInt`, typed arrays (`ts-unsupported-class`);
`Object.assign`, `Object.freeze`, `Object.entries` outside the table above (`ts-unknown-builtin`);
`instanceof` (`ts-instanceof`); `toLocaleString`, `localeCompare`, `Intl` (`ts-locale`);
`toFixed`, `parseFloat`, `Number(…)`, `NaN`, `Infinity`, `isNaN`, and `Array.from` (`ts-unknown-builtin`).

## Bounded forms

Four builtins give the model the bounded forms Sx has. Each is a call whose first argument is an object
literal with literal values, because the checker must read the bounds before the program runs. A value that is
not a literal is `ts-bound-not-literal`. Durations are integer milliseconds and become Sx duration literals.

| Builtin | Lowering |
| --- | --- |
| `retry({ delaysMs: [1000, 2000], on: ["Timeout"] }, () => e)` | `(retry {:delays [1s 2s] :on [Timeout]} e)` |
| `retry({ backoffMs: [100, 5000], withinMs: 30000 }, () => e)` | `(retry {:backoff [100ms 5s] :within 30s} e)` |
| `waitUntil({ everyMs: 15000, withinMs: 600000 }, () => e, (v) => p)` | `(wait-until {:within 10m :every 15s} (fn [v] p) e)` |
| `await sleep(ms)` | `(sleep 10s)` |
| `parMap({ limit: 8 }, xs, (x) => e)` | `(par-map {:limit 8} (fn [x] e) xs)` |
| `loop({ max: 50 }, { cursor: null, events: [] }, ({ cursor, events }) => B)` | `(loop {:max 50} [cursor none events []] S(B))` |

`retry` re-evaluates `e` after a failure with a retried kind and returns its value; after the last attempt the
last `CallError` is thrown. Its default kinds, the `retry-write` rule for non-idempotent writes, and the
idempotence requirement are those of [retry](language.md#retry). `waitUntil` returns the first value for which
`p` is true, and throws a `Timeout` `CallError` when `withinMs` runs out. `e` must have only `read` effects.
`parMap` runs `f` on the elements with at most `limit` in flight and returns the results in order; `f` must have
only `read` effects. The five accept `on`, `delaysMs`, `backoffMs`, `withinMs`, `everyMs`, `limit`, `max` and
no other key.

`loop` replaces `while (true)` and `do … while`. Its second argument is an object literal whose keys name the
loop variables and whose values are their initial values. The function takes the same names as a pattern.
It returns through `next({ cursor, events })`, which repeats with those values, or `done(value)`, which
ends the loop with `value`. Both appear only in tail position of the function, as `return next(…)` or
`return done(…)`, and `next` must name every key. `next(x)` is `(recur …)` in key order and `done(v)` is `v`. A
body whose every path ends in `next` is `loop-no-result`. Reaching `max` with another `next` is the
`loop-limit` trap, which is a failure and not an exception.

The retry and wait builtins are the only repetition other than `for…of` and the counted `for`. The
callbacks follow the closure and mutation rules above.

## Tools

A granted capability `NAMESPACE.NAME` is `tools.namespace.name(input)`. Each part is the camel case of its kebab
case: `fs.read-text` is `tools.fs.readText`, `mcp-github.list-issues` is `tools.mcpGithub.listIssues`. A name
that does not exist is `unknown-tool`, with a suggestion within edit distance 2. The call is `(call fs.read-text input)`, and
its value is the declared output: a wrapped output is unwrapped, a text output is the string, and
nothing is a `CallToolResult`. A failure throws the `CallError`. `input` is a value of the declared record
type; an empty input is written `{}` or omitted.

`tools` is not a value. Any other use of the name, an index into it, a copy of it, a method of a namespace taken
without a call, and the discovery calls of codemode (`searchTools`, `describeTool`, `ALL_TOOLS`) are
`ts-tools-escape`.

| Sx type | Declaration in the `.d.ts` |
| --- | --- |
| `Bool`, `String`, `Bytes` | `boolean`, `string`, `string` (base64) |
| `Int`, `Float` | `Int`, `Float`, aliases of `number` that the file defines |
| `Unit` | `null` |
| `List<T>`, `Tuple<A, B>` | `T[]`, `[A, B]` |
| `Map<String, V>` | `Record<string, V>` |
| `Record {k: T, …}` and open records | `{ k: T; … }`; an `Option` field is `k: T | null` in an output |
| `Option<T>` | output `T | null`; input field `k?: T | null` |
| Enum without payloads | a union of string literals |
| Enum with payloads | `{ tag: "A"; value: T } | "B"` |
| `Result<T, E>` | `{ ok: T } | { err: E }` |
| Core nominal | its base type |

The generated file is the text below, in this order: the aliases, the named types and enums, `CallError`,
the `Tools` interface with one member per namespace, and the declarations of the builtins. Each tool
carries a doc comment with its description and its effect, `[read]`, `[write, idempotent]` or
`[write, not idempotent]`, and the behavior notes of `sx guide --notes` follow the file.

```ts
type Int = number;
type Float = number;
interface CallError extends Error { kind: "Timeout" | "Unavailable" | "RateLimited" | "NotFound" | "PermissionDenied" | "Invalid" | "Conflict" | "Failed"; message: string }
interface Tools {
  fs: {
    /** Reads a file as text. [read] */
    readText(input: { path: string }): Promise<string>;
    /** Lists a directory. [read] */
    list(input: { path: string }): Promise<{ name: string; kind: "File" | "Directory" }[]>;
  };
}
declare const tools: Tools;
declare function retry<T>(options: { delaysMs?: number[]; backoffMs?: [number, number]; withinMs?: number; on?: CallError["kind"][] }, body: () => Promise<T>): Promise<T>;
declare function waitUntil<T>(options: { everyMs?: number; backoffMs?: [number, number]; withinMs: number }, poll: () => Promise<T>, done: (value: T) => boolean): Promise<T>;
declare function parMap<T, U>(options: { limit: number }, items: T[], body: (item: T) => Promise<U>): Promise<U[]>;
declare function loop<S extends object, R>(options: { max: number }, state: S, body: (state: S) => Step<S, R>): Promise<R>;
declare function next<S>(state: S): Step<S, never>;
declare function done<R>(value: R): Step<never, R>;
declare function sleep(ms: number): Promise<void>;
```

This extends the declarations of `bench/codemode/typescript.py`, which reads the output of `sx mcp describe` as
Sx type strings. The changes are the `Int` and `Float` aliases in place of `number`, the `null` union for
optional input, the `Result` form, and the builtins.

## JS mode and the new kernel surface

JS mode is a flag of `check`, set by the front end, that changes the checker in seven ways. None changes an
Sx program, because the flag is off for them.

1. Numbers: the promotion, join, and safe-integer rules of [Numbers](#numbers).
2. `+` accepts a `String` operand and means `str`; a mixed `String` and non-primitive operand is `js-coercion`.
3. `=` accepts only primitive operands and promotes `T` to `Option<T>`; otherwise `js-equality`.
4. `<`, `<=`, `>`, `>=` and string sort keys compare by UTF-16 code unit.
5. A condition is any type and elaborates through `truthy`.
6. A function of the `js` core may have several signatures, and the checker chooses by the type of its first
   argument.
7. `typeof e` and `Array.isArray(e)` are decided by the static type of `e`.

| Addition | Where | Needed for |
| --- | --- | --- |
| `(fail m)`: `m` is a `String`, or a `CallError` to raise again. Any type. Failure class `thrown`, caught by `attempt` as a `CallError` of kind `Failed` with message `m`. | kernel, Rust and Lean | Phase 19, required |
| `(log e …)`: appends text to the run's `logs`, `Unit`. The run outcome gains `logs`. | kernel, Rust and Lean | Phase 19, required |
| `unit`: the `Unit` value, for programs with no result | kernel, Rust and Lean | Phase 19, required |
| `(range start end)`, `(range start end step)`: `List<Int>`; `end` is excluded; `step` is a nonzero integer literal | prelude, Rust and Lean | Phase 19, required |
| `(put target key value)`: a record with the field replaced, or a map with the key set | kernel, Rust and Lean | Phase 19, required |
| `(truthy e)`, elaborated by the checker | checker only | Phase 19, required |
| `safe-int` IR node, `(safe-int e)`: the `arithmetic` trap outside ±2^53 | IR, Rust and Lean | Phase 19, required |
| `(enumerate xs)`: `List<Tuple<Int, T>>` | prelude, Rust and Lean | Phase 19, required |
| `js.length`, `js.slice`, `js.index-of`, `js.join`, `js.reverse`, `js.find`, `js.settled-status`, `js.settled-value`, `js.settled-reason` | `js` core, Rust and Lean | Phase 19, required |
| `js.trim`, `js.to-lower`, `js.to-upper`, `js.pad-start`, `js.pad-end`, `js.replace-first`, `js.parse-int`, `js.is-integer` | `js` core | Phase 19, required |
| `js.stringify`, `js.parse` (typed decode) | `js` core | Phase 19, required |
| `retry`, `wait-until`, `sleep`, `loop`, `par`, `par-map` with durations from milliseconds | exist | none |
| Dates: parsing, `toISOString`, `getTime`, and no `Date.now` | a `date` core | deferred |
| Regular expressions with ECMAScript semantics | a `js` core addition or translator | deferred |
| `Set` and `Map` classes, `Object.entries` of a `Map` in insertion order | would need ordered maps | deferred |
| `parseFloat`, `toFixed`, `localeCompare`, `Intl` | `js` core | deferred |
| `pop`, `shift`, `splice` | rewrite with `Option` results | deferred |
| Object spread `{...r, k: v}` | `put` over a record with added fields | deferred |
| `switch`, `finally`, labeled statements | | deferred |

## Diagnostics

A diagnostic has the parts of [Diagnostics](language.md#diagnostics): code, span, excerpt, message, detail, fix,
and help. The span is a line and column range in the TypeScript text, and the excerpt is the TypeScript line.
`help` names a section of the guide, `sx help typescript`. Front-end codes begin `ts-` and are reported in source order together with the
checker's, up to 20.

**The checker's diagnostics in TypeScript terms.** The checker's codes (`type-mismatch`, `unknown-field`,
`non-exhaustive`, `unauthorized`, `out-of-scope`, `parallel-write`, `retry-write`, `condition-not-bool`,
and the rest) keep their codes. Each node carries the span of the TypeScript it came from, so the span is
the TypeScript expression. The message is rewritten: `List<T>` is `T[]`, `Option<T>` is `T | null`,
`Record {a: T}` is `{ a: T }`, `Map<String, V>` is `Record<string, V>`, `Tuple<A, B>` is `[A, B]`, and an
enum is a union of its string literals; `(get r :f)` is `r.f`; `call` is a call of `tools.ns.name`; the fix is
rewritten as TypeScript or dropped when it has no TypeScript form. A call that the policy refuses, or that
`explain` lists, names `tools.ns.name` and the span of the call. The failure of a run keeps the span of the
call that failed.

**The front end's diagnostics.**

| Code | Rejects | Message | Fix |
| --- | --- | --- | --- |
| `ts-syntax` | text that is not TypeScript | the parser's message | |
| `ts-unsupported` | a construct not in the grammar | `X is not supported in checkable TypeScript` | |
| `ts-import-export` | `import`, `export`, `require`, `import()` | `Programs have no modules` | use the tools directly |
| `ts-class` | `class`, `this`, `new` other than `new Error`, getters | `Classes are not supported` | use object literals |
| `ts-generator` | generators, `yield`, `for await` | `Generators are not supported` | use `for…of` |
| `ts-enum` | `enum`, `namespace`, `declare` | `X is not supported` | use a union of string literals |
| `ts-while` | `while`, `do`, `for(;;)`, uncounted `for` | `Loops must be bounded` | `retry`, `waitUntil`, `loop`, or `for…of` |
| `ts-counted-loop` | a counted `for` that breaks rules 1 to 4 | which rule | |
| `ts-for-in` | `for…in` | `for…in is not supported` | `Object.keys(m).sort()` |
| `ts-switch` | `switch` | `switch is not supported` | `if` chain |
| `ts-label` | labels, `with`, `debugger` | | |
| `ts-finally` | `finally` | `finally is not supported` | write the cleanup in both branches |
| `ts-recursion` | a function calling itself or a later one that calls it | `A function may not call itself` | `loop` |
| `ts-eval` | `eval`, `Function`, `new Function`, dynamic `import` | `Code cannot be built at run time` | |
| `ts-tools-escape` | `tools` not directly called, indexed, passed, aliased | `tools can only be called as tools.namespace.name(…)` | |
| `unknown-tool` | a tool or namespace that is not granted | `No tool X` | the nearest names |
| `ts-operator` | `**`, bitwise, `in`, `instanceof`, `delete`, `void`, comma, `??=`, `||=`, `&&=` | `X is not supported` | |
| `ts-loose-equality` | `==`, `!=` other than against `null` | `Use === and !==` | |
| `ts-instanceof` | `instanceof` | `Types are static; instanceof is not supported` | compare a field |
| `ts-uninitialized` | a `let` without an initializer that is read before every path assigns it | `This name may be read before it is assigned` | give it a value |
| `ts-undefined` | `undefined`, `void 0`, `=== undefined`, `typeof x === "undefined"` | `undefined does not exist; absent values are null` | `== null` |
| `ts-optional-chain` | a `?.` result used outside `??`, `== null`, `!= null`, a condition | `The result of ?. is undefined, not null` | `x == null ? null : x.f` |
| `ts-unsafe-integer` | an integer literal beyond 2^53 | `This literal is not a safe integer` | |
| `ts-key` | integer-like or computed object keys, duplicates | `This key cannot be a record field` | |
| `ts-spread` | object spread, spread in a call | `Spread is supported only in array literals` | |
| `ts-param` | default, rest, `this` parameters; pattern defaults and rest | `Defaults and rest are not supported` | |
| `ts-regexp` | regular expression literals, `RegExp`, `match`, `test`, `exec` | `Regular expressions are not supported` | `includes`, `startsWith`, `split` |
| `ts-date` | `Date` | `Date is not supported` | |
| `ts-nondeterministic` | `Math.random`, `setTimeout`, `setInterval`, `Date.now` | `A program must be deterministic` | `sleep` |
| `ts-locale` | `toLocaleString`, `localeCompare`, `Intl` | `Output depends on the locale` | |
| `ts-unsupported-class` | `Set`, `Map`, `Symbol`, `BigInt`, typed arrays | `X is not supported` | use arrays and records |
| `ts-unknown-builtin` | a global or method not in the table | `No builtin X on Y` | the nearest builtin |
| `ts-callback-arity` | a callback with a parameter the builtin does not supply | | |
| `ts-reduce-init` | `reduce` without an initial value | `reduce needs an initial value` | |
| `ts-sort-comparator` | a comparator not of the accepted forms; `.sort()` on numbers | the accepted forms | `k(a) - k(b)` |
| `ts-map-order` | observing a `Map`'s order | `The order of a Map's keys is not preserved` | `Object.keys(m).sort()` |
| `ts-record-enumeration` | `Object.keys`, `entries`, `values` of a record | | name the fields |
| `ts-annotation` | a type annotation outside the grammar | | |
| `ts-cast` | `as` other than on `JSON.parse`, `!`, `satisfies`, `<T>x` | `A cast cannot be checked` | |
| `ts-json` | a replacer or string indent | | |
| `ts-json-parse-type` | `JSON.parse` without a supported type | `JSON.parse needs the type of the document` | `JSON.parse(t) as { … }` |
| `ts-promise-value` | a promise stored, returned, or unawaited | `Await calls where they are made` | |
| `ts-log-in-parallel` | `console.log` in a parallel branch | | |
| `ts-console` | a `console` method other than `log` | | `console.log` |
| `ts-settled-escape` | a `Settled` used whole when a branch can throw a plain error | `This error is not a CallError, so it cannot be printed` | `.status`, `.value`, `.reason.message` |
| `ts-function-value` | a function that is returned, stored, assigned, or passed to a tool | | |
| `ts-recursion` | see above | | |
| `ts-mutation-pattern` | an assignment not in the mutation table | the pattern and why | |
| `ts-mutating-method` | `pop`, `shift`, `unshift`, `splice`, `fill`, `copyWithin`, `Object.assign` | `X changes an array in place` | build a new array |
| `ts-mutation-escape` | a mutated variable in a closure or stored while it can still change | which variable and use | |
| `ts-mutation-in-try` | an assignment in a `try` before a throwing operation | | |
| `ts-throw` | `throw` of anything but `new Error(string)` or the `catch` parameter | | |
| `ts-error-use` | a `catch` parameter used other than as [Errors](#errors) allows | which use and why | `e.message` |
| `ts-bound-not-literal` | a non-literal option of a bounded builtin | `The bound must be a literal` | |
| `js-float-as-int` | a `Float` where an `Int` is needed (checker, JS mode) | `Expected Int, found Float` | `Math.floor(x)` |
| `js-float-remainder` | `%` with a `Float` | | |
| `js-coercion` | an operand JavaScript would convert implicitly | `X + Y converts an object to text` | |
| `js-equality` | `===` on arrays or objects | `=== compares references here` | compare fields |
| `js-for-of-source` | `for…of` over a non-list | | |

## The classifier

`bench/subset/classify.mjs` implements the syntactic rules above, so its verdict on a script is whether the
front end would accept it. A script is **accepted** when it triggers none of the front end's codes, and
**accepted without rewriting** when it also uses no reassignment, property write, in-place array method,
`try`, `throw`, or counted `for`. It first applies the mechanical rewrite of codemode's API to the subset's:

| codemode | subset |
| --- | --- |
| `tools.mcp__server__ns_tool_name(x)` | `tools.ns.toolName(x)` |
| `tools.read(x)`, `tools.grep(x)`, other pi built-ins | `tools.pi.read(x)` |
| `r.structuredContent`, `r.structuredContent?.result` | `r` |
| `r.content[0].text`, `r.content.map(c => c.text).join(s)` | `r` |
| `r.structuredContent ?? r.content`, `r \|\| r.content` | `r` |
| `r.isError` | `false` |
| `text(x)` | `console.log(x)` |

A use of `.content`, `.structuredContent`, or `.isError` that the rewrite does not cover stays, and the
classifier rejects it as `ts-codemode-residue`. That code measures the rewrite, not the subset. A call
to `store` or `load`, codemode's cross-run storage, is `ts-unknown-builtin`.

The classifier cannot see types, so it does not apply the rules that need them: `js-float-as-int`,
`js-coercion`, `js-equality`, `js-for-of-source`, the record half of `ts-map-order`, the checks
that an optional is not read bare, the `Settled` accessors, and the traps. Its share is an upper bound on
the share the front end accepts, by the number of scripts those rules would reject. It treats every
`JSON.parse` as untyped because the corpus is JavaScript.

The tests in `bench/subset/classify.test.mjs` cover each code with a script that triggers it and one near it
that does not.

On the 512 tool-calling scripts of Phases 16 and 17, 416 (81%) are accepted and 286 (56%) are accepted without
rewriting. The rules that reject the most scripts are `ts-unknown-builtin` (25, mostly codemode's `store`
and `Set` and `Map` methods), `ts-optional-chain` (19), `ts-regexp` (12), `ts-spread` (11, mostly
`{...settled}`), `ts-codemode-residue` (11), and `ts-while` (10). `bench/subset/summary.py` prints the full table.
