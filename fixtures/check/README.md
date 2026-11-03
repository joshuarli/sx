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

A fixture may also carry `; grants: PATH`, after the `; allow:` line when there is one, naming a grants
file relative to the repository root, as `sx check --grants` takes it. Its capabilities are granted with
their scopes, and only those, plus any `; allow:` IDs. The `fs` fixtures declare
`fixtures/check/fs-declarations.json` and grant `fixtures/check/fs-grants.json`, whose root,
`/usr/share`, exists and is no symlink on both macOS and Linux, so the expected output is the same on
both. Only Rust reads grants, so such a fixture is also marked `; python: no`.

`test_every_specified_diagnostic_code_has_a_pinned_fixture` fails when a code listed in
[docs/language.md](../../docs/language.md#diagnostics) has no fixture here or in `fixtures/read`.

## Programs that Python does not check

A fixture for a feature that only Rust and Lean implement carries the header line `; python: no`.
The line is the last header line: it follows the `; task:` or `; declarations:` line, and the
`; allow:` and `; grants:` lines when there are any. Python's fixture tests do not collect a marked fixture. The
Rust check test compares its rendered diagnostics with the `.expected` file, as for every fixture.
The command-line comparison against Python skips it.

A `; declarations:` line may name a declarations file that lists `cores`. `fixtures/check/decimal-declarations.json`
registers the `decimal` core for the fixtures that need its types. Only Rust loads cores from declarations.

A reader treats the whole file as the program, so the header lines are comments in it and a
diagnostic's line numbers count them.

A fixture checked in the JavaScript mode of the checker carries the header line `; dialect: js` and must also carry `; python: no`. It needs the `js` core, so it names `fixtures/run/declarations/js.json`.
