## Before you answer

Check the program against each line:

1. Write one binding per line, and give a long step its own name or local function, so no line closes
   more than a few brackets. Count the brackets of every `let`: `(let [` opens two, `]` closes the
   bindings, and the body is followed by one `)`.
2. Every tool call whose failure the task mentions, such as a missing file, directory, or record, is
   inside an `attempt`, and the program matches `(ok v)` and `(err e)`.
3. Every record in a list has the same fields. A field that only some records have becomes an `Option`
   for all of them.
4. `get` takes a default only for an `Option` field or a map entry.
5. Every `if` has an else, and every `cond` ends with `:else`.
6. Each `loop` has `{:max N}` large enough for the task, and ends by returning a value instead of
   calling `recur`.
7. `par-map` and `par` call only tools whose effect is `read`. Writes run in `map`, `let`, or `loop`, in
   the order the task requires.
8. A `retry` of a write that is not idempotent names refusal kinds only, such as `:on [RateLimited]`.
9. The result has exactly the fields and order the task asks for, with strings for labels and Ints for
   counts.
