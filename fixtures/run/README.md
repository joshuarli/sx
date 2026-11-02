# Run fixtures

Each `.json` file is one semantic case that every implementation of Sx must reproduce: this Python
interpreter now, and the Lean evaluator and Rust port later. `declarations.json` holds the tools
and types the cases share.

| Field | Meaning |
| --- | --- |
| `description` | What the case shows, in one sentence |
| `python` | Optional. `false` marks a case that the frozen Python prototype does not implement. Any other value, or no field, leaves the case unmarked. |
| `program` | The source text |
| `declarations` | The declarations file, relative to this directory |
| `inputs` | Optional. A JSON object from input name to value, in the [JSON encoding](../../docs/language.md#values-and-json). Only a script (`python: false`) takes inputs here. An `Option` input left out is `none`. |
| `allow` | The capability IDs the host authorizes |
| `budgets` | Optional budget overrides, by budget name |
| `responses` | Fixture responses, in the format of `bench/codemode/README.md` |
| `steps` | Instead of `responses`: each step lists the request IDs the machine must suspend with, and the responses to resume it with, as `[id, {"output": ...}]` or `[id, {"error": ...}]` pairs |
| `expect` | `{"completed": JSON}`, `{"failed": KIND}`, `{"rejected": [CODE, ...]}`, or `{"invalid-input": TEXT}` |
| `trace` | The logical trace: each request, delay, and response, in the order the machine produced them |

A rejected case has no trace, because a rejected program never runs. A case that expects `invalid-input` has none either: the inputs
do not decode, a name is not an input of the script, or a required input has no value, and the run never starts. `TEXT` must occur in
the message that the Rust kernel's `invalid-input` protocol error and the command line's usage error carry. Because the machine
binds only the inputs a program uses, such a case uses every input it declares.

Declarations that only Rust and Lean read, such as the ones that list `cores`, live in `declarations/`, so the case listing
finds only cases. `declarations/decimal.json` registers the `decimal` core and three capabilities that carry decimals.

## Cases that Python does not run

Python is frozen, so a feature that lands in Rust and Lean only gets cases with `"python": false` at
the top level of the case object. Python's fixture tests do not collect them. The Rust gates and
`lean/agree.py` still run every marked case, and judge it only against its own `expect` and `trace`,
never against live Python output. Rust checks and runs the program itself, so a marked case needs
no Python export. An unmarked case is also compared against Python, as before.

The Rust command-line comparison and `sx explain` comparison against Python skip marked cases,
because the fixture holds no CLI or `explain` output. `lean/agree.py` takes the IR of a marked case
from `sx check --emit ir` on the Rust binary, found as `SX_KERNEL` or under `rust/target`, with the
default budgets.

`lean/agree.py` also takes each case rejected with `retry-write`, checks its program against the same
declarations with every write marked idempotent, flips the flags back in the exported IR, and requires
the Lean checker to reject that IR, because no program that Python accepts exercises the Lean rule.
