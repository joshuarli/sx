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
| `allow` | The capability IDs the host authorizes |
| `budgets` | Optional budget overrides, by budget name |
| `responses` | Fixture responses, in the format of `bench/codemode/README.md` |
| `steps` | Instead of `responses`: each step lists the request IDs the machine must suspend with, and the responses to resume it with, as `[id, {"output": ...}]` or `[id, {"error": ...}]` pairs |
| `expect` | `{"completed": JSON}`, `{"failed": KIND}`, or `{"rejected": [CODE, ...]}` |
| `trace` | The logical trace: each request, delay, and response, in the order the machine produced them |

A rejected case has no trace, because a rejected program never runs.

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
