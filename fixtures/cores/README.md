# Core conformance vectors

Each `CORE.json` file holds input and output pairs for every function of one core: `text`,
`semver`, `git`, `github`, and `decimal`. The Python cores produced the first four and are now frozen,
and the Rust crate [`rust/sx-cores`](../../rust/sx-cores) must reproduce every one. `decimal` has no
Python core: `decimal.json` comes from Python's `decimal` module, as described under
[The decimal vectors](#the-decimal-vectors). The vectors are the shared
definition of what each core function does until the Lean model covers the cores.

| Field | Meaning |
| --- | --- |
| `core` | The core ID |
| `version` | The core version the vectors were generated from |
| `vectors` | The vectors, one per line |

| Vector field | Meaning |
| --- | --- |
| `function` | The function ID, such as `text.casefold` |
| `args` | The arguments, in the [JSON form of their declared types](../../docs/language.md#inputs) |
| `result` | The result in the same form, present unless the call fails |
| `fails` | Present instead of `result` when the Python function raises. The only failure is `"ValueError"`, from `text.split` with an empty separator, which the Rust core reports as `invalid-input`. |

`scripts/core_vectors.py` in the crate builds the vectors by decoding each argument with the
function's declared type, calling the registered implementation, and encoding the result. The
corpus is fixed edge cases plus seeded random inputs from a generator that does not depend on the
Python version, so a file changes only when a core changes. Regenerate with:

```
uv run python rust/sx-cores/scripts/core_vectors.py
```

`tests/test_core_vectors.py` regenerates the vectors in memory and fails if a committed file
differs, so the vectors cannot drift from the Python cores.

## Where the corpus stops

The vectors stay inside the domain where the Python cores obey their own types, and the Rust
crate is defined only there.

- Strings are Unicode scalar values. Python strings can hold lone surrogates, and Rust strings
  cannot, so no vector contains one.
- `text.to-int` of more than 4300 digits raises in Python, because of the interpreter's integer
  string limit. The vectors stop at 4000 digits. Rust answers `none` for any number that does not
  fit in an Int.
- `semver.parse` of a component beyond the 64-bit Int range returns a version that breaks the
  `Int` type in Python. The vectors stop at `9223372036854775807`. Rust answers `none`.
- `text.casefold` and `text.trim` depend on the Unicode data of the running Python. The Rust
  tables are generated from it by `scripts/casefold_table.py`, and the same test file fails when
  the checked-in tables differ from the running Python.

## The decimal vectors

`decimal.json` has the same fields, with one difference: a vector that fails has `"fails": "arithmetic"`
instead of `"fails": "ValueError"`, and the Rust kernel must turn it into `CoreError::Arithmetic`.

`scripts/decimal_vectors.py` in the crate decides each result with Python's `decimal` module at 400 digits
of precision, so no operation is inexact. It rejects a literal that is not `-?[0-9]+(\.[0-9]+)?`, then
canonicalizes the value and rejects one with more than 38 significant digits or 38 fractional digits, the
limits of the core. A division is rounded once, to the places asked, by `Decimal.quantize` with the mode's
Python rounding. The corpus is fixed edge cases, every pair from a table of small values, and seeded
random literals from a generator that does not depend on the Python version. Regenerate with:

```
uv run python rust/sx-cores/scripts/decimal_vectors.py
```

`tests/test_decimal_vectors.py` regenerates the vectors in memory and fails if the committed file differs.
