## Before you answer

Check the program against each line:

1. The program is a list of `def` and `defn` forms, one step each, ending with the result. No
   definition is longer than about ten lines; a longer one becomes two, or a helper `defn`. Each
   definition closes all of its brackets on its last line.
2. Every tool call whose failure the task mentions, such as a missing file, directory, or record, is
   inside an `attempt`, and the program matches `(ok v)` and `(err e)`.
3. Every record in a list has the same fields. A field that only some records have becomes an `Option`
   for all of them.
4. Every `if` has an else, and every `cond` ends with `:else`.
5. Each `loop` has `{:max N}` large enough for the task, and ends by returning a value instead of
   calling `recur`.
6. `par-map` and `par` call only tools whose effect is `read`. Writes run in `map`, `def`, or `loop`, in
   the order the task requires.
7. A `retry` of a write that is not idempotent names refusal kinds only, such as `:on [RateLimited]`.
8. The result has exactly the fields and order the task asks for, with strings for labels and Ints for
   counts.
