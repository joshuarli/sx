# Checker fixtures

Each fixture is a program in `NAME.sx` and the checker's full output in `NAME.expected`. An empty
`.expected` file means the program must check with no diagnostics.

The first line of every program is `; task: TASK-ID`. The checker reads the types, enums, cores,
and capabilities of that task in `bench/codemode/tasks/`.

A fixture may instead start with `; declarations: fixtures/run/declarations.json`, which checks against
the types and capabilities the run fixtures share, for types such as `Bytes` that no task declares.

Most programs are model answers from the syntax pilot, copied verbatim. Each one is either a
construct the pilot made legal, or a mistake that stays an error, with the diagnostic the checker
must print for it. A few later programs are short and written to pin one diagnostic.

A fixture whose second line is `; allow: ID ...` checks with only those capabilities authorized.
Otherwise every capability the task declares is authorized.

`test_every_specified_diagnostic_code_has_a_pinned_fixture` fails when a code listed in
[docs/language.md](../../docs/language.md#diagnostics) has no fixture here or in `fixtures/read`.
