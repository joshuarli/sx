# Code Mode task corpus

Each task is a directory under `tasks/` with two files: `task.json` and `reference.mjs`. Check
every task with:

```sh
node bench/codemode/check_references.mjs
```

The checker validates each `task.json`, runs `reference.mjs` against the task's fixture
responses, and compares its result with `expected`.

## task.json

| Field | Type | Meaning |
| --- | --- | --- |
| `id` | string | Same as the directory name. Development tasks start with `dev-`, held-out tasks with `held-`. |
| `split` | `"development"` or `"held-out"` | Which set the task belongs to |
| `kinds` | list of strings | What the task exercises, from [Kinds](#kinds) |
| `domain` | `"repomod"` or `"other"` | Whether the task uses repository and CI tools |
| `sx-expressible` | boolean | `false` when any declared type uses `Map`, `Float`, or `Bytes`, otherwise `true` |
| `description` | string | The task as a user would state it. See [Descriptions](#descriptions). |
| `types` | object | Optional. Named record types, `"Name": "Record {...}"` |
| `enums` | object | Optional. `"ns.Name": {"Variant": null, "Other": "Type"}`. `null` means the variant carries no value. |
| `cores` | list | Pure function libraries available to the task: `"text"`, `"semver"`, or none |
| `capabilities` | list | Tools the task may call. See [Capabilities](#capabilities). |
| `responses` | list | Fixture responses. See [Responses](#responses). |
| `expected` | JSON | The correct final result |

### Types

Types are written in Sx type notation:

| Type | JSON |
| --- | --- |
| `Bool`, `Int`, `String` | boolean, integer, string |
| `Unit` | `null` |
| `List<T>` | array |
| `Option<T>` | `null`, or the value. `T` must not be `Unit` or another `Option`. |
| `Record {name: String, open-prs: Int}` | object with exactly these keys |
| a name from `types` | its record |
| `ns.Name` from `enums` | a variant without a value is its name as a string, `"Passing"`. A variant with a value is `{"tag": "Failed", "value": ...}`. |
| `Map<String, T>` | object with arbitrary keys. Sx v0 lacks it. |
| `Float` | a number with a fraction. Sx v0 lacks it. |
| `Bytes` | base64 string. Sx v0 lacks it. |

Use the type that fits the data. Do not avoid `Map`, `Float`, or `Bytes` because Sx lacks them.
Set `sx-expressible` to `false` instead.

Field names are lowercase words joined by `-`. Prefer one word.

### Capabilities

Each entry has:

| Field | Meaning |
| --- | --- |
| `id` | `namespace.name`, lowercase words joined by `-`, for example `github.list-prs` |
| `effect` | `"read"` or `"write"` |
| `idempotent` | Required for writes. `true` when repeating a call with the same input has the same effect as making it once. |
| `input` | A record type. Every field is required, so there is exactly one way to call it. |
| `output` | Any type |
| `description` | One sentence on what the tool does and returns |

A failing call raises an error with a `kind` and a `message`. The kinds are `Timeout`,
`Unavailable`, `RateLimited`, `NotFound`, `PermissionDenied`, `Invalid`, `Conflict`, and `Failed`.

### Responses

Each entry is `{"capability": ID, "input": {...}, "results": [...]}`. Each result is either
`{"output": VALUE}` or `{"error": {"kind": KIND, "message": TEXT}}`.

- The checker finds the entry whose `capability` and `input` match the call exactly.
- Results are served in order. For a read, the last result repeats once the list runs out, so a
  list of one result answers any number of calls.
- For a write, each result is served once. The task fails if a write runs more times than its
  results, or fewer.
- A call with no matching entry fails the run.

## reference.mjs

The default export is an async function that receives one object, `api`, and returns the result.

- `api.NAMESPACE.name(input)` calls a capability. Function names are camelCase, so
  `github.list-prs` is `api.github.listPrs`. Input and output keys stay exactly as declared, so a
  field `open-prs` is read as `pr["open-prs"]`. A tool whose input is `Record {}` may be called
  with no argument.
- A failed call throws an error with `.kind` and `.message`.
- `api.text` and `api.semver` hold the core functions listed in `cores.json`, when the task lists
  those cores.
- `await api.sleep(ms)` waits. Time is virtual, so sleeping costs nothing.

Write the solution a careful engineer would write. Do not golf it.

## Descriptions

A description states the task in plain language and fully specifies the result: its shape, field
names in backticks, and the order of every list. Two correct solutions must produce identical
JSON. When the task involves retries or polling, the description states the policy exactly, for
example "retry up to 3 more times, waiting 1, 2, then 4 seconds".

Describe the goal, not the steps. Name no programming language.

## Kinds

`filter-group`, `parallel`, `conditional-call`, `partial-failure`, `retry`, `polling`,
`pagination`, `schema-detail`, `core-composition`, `string`, `arithmetic`, `ad-hoc-logic`, `write`.

`schema-detail` marks a task whose correct answer depends on reading the declared types closely,
for example an `Option` field or a field whose name differs from the obvious one.

## The probe

`probe.py` compares Sx with TypeScript on these tasks. Each trial sends a model one arm's prompt
through the `pi` CLI, with every tool, skill, and context file disabled, and allows two repairs.

- The Sx arm gets a guide level from `sx help --level` and the task's tool listing. Its answer is
  checked and run on the Sx machine against the task's fixtures.
- The TypeScript arm gets TypeScript declarations generated from the task by `typescript.py`. Its
  answer is checked by `tsc` and run by `run_js.mjs` against the same fixtures, through `host.mjs`.
- A repair feeds back only what a real host would show: diagnostics, a type error, a runtime
  failure, or a tool rejecting its input. A wrong result shows nothing, so it ends the trial.
- A trial is correct when its result equals `expected`, its writes ran exactly as the fixtures
  require, and its waits match the reference solution's waits.

```sh
uv run python bench/codemode/probe.py --measure-guides --out .work/probe/measure
uv run python bench/codemode/probe.py --split development --guide minimal --guide standard --samples 2 --out .work/probe/dev
uv run python bench/codemode/probe.py --report --out .work/probe/dev
```
