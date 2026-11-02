# Reader fixtures

Each fixture is a source text in `NAME.sx` and the reader's full output in `NAME.expected`: `ok:`
and the canonical form when the text reads, otherwise the rendered diagnostics.

## Programs that Python does not read

A fixture for syntax that only Rust implements starts with the line `; python: no`. It must be the
first line, or follow other comment lines at the top of the file. Python's reader tests do not collect a
marked fixture, and the Rust reader test compares it with its `.expected` file as for every fixture.
The marker is a comment in the source, so a diagnostic's line numbers count it.

## Programs whose closers the reader infers

A fixture that starts a comment line with `; closers: infer`, among the comment lines at the top
of the file, is read with [closer inference](../../docs/language.md#closer-inference), as the
kernel reads a program for a host. Its `.expected` file is `ok:` and the canonical form of the
inferred program, then one `inferred:` line for each edited line. When the reader infers nothing, the
file is the rendered diagnostics for the text as written. Fixtures without the marker are read
strictly, so they keep testing the reader's own bracket diagnostics. The fixtures read without a checker, so a program whose best repairs disagree stays rejected there. A fixture named
`NAME-stays-rejected` is one that inference must refuse.

A file `NAME.model-repair` beside `NAME.sx` holds the program that a model wrote next, after the
reader's diagnostics for `NAME.sx`. The inferred program must read to the same canonical form as
the model's own repair. The `infer-corpus-` fixtures are programs that models wrote in the
benchmark, with the sandbox path replaced by `/srv/work`.
