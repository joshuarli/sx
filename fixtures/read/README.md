# Reader fixtures

Each fixture is a source text in `NAME.sx` and the reader's full output in `NAME.expected`: `ok:`
and the canonical form when the text reads, otherwise the rendered diagnostics.

## Programs that Python does not read

A fixture for syntax that only Rust implements starts with the line `; python: no`. It must be the
first line, or follow other comment lines at the top of the file. Python's reader tests do not collect a
marked fixture, and the Rust reader test compares it with its `.expected` file as for every fixture.
The marker is a comment in the source, so a diagnostic's line numbers count it.
