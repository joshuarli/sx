# Run fixtures

Each `.json` file is one semantic case that every implementation of Sx must reproduce: this Python
interpreter now, and the Lean evaluator and Rust port later. `declarations.json` holds the tools
and types the cases share.

| Field | Meaning |
| --- | --- |
| `description` | What the case shows, in one sentence |
| `program` | The source text |
| `declarations` | The declarations file, relative to this directory |
| `allow` | The capability IDs the host authorizes |
| `budgets` | Optional budget overrides, by budget name |
| `responses` | Fixture responses, in the format of `bench/codemode/README.md` |
| `steps` | Instead of `responses`: each step lists the request IDs the machine must suspend with, and the responses to resume it with, as `[id, {"output": ...}]` or `[id, {"error": ...}]` pairs |
| `expect` | `{"completed": JSON}`, `{"failed": KIND}`, or `{"rejected": [CODE, ...]}` |
| `trace` | The logical trace: each request, delay, and response, in the order the machine produced them |

A rejected case has no trace, because a rejected program never runs.

`lean/agree.py` also takes each case rejected with `retry-write`, checks its program against the same
declarations with every write marked idempotent, flips the flags back in the exported IR, and requires
the Lean checker to reject that IR, because no program that Python accepts exercises the Lean rule.
