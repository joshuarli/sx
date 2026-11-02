A task desk service offers three tools. `list-tasks` returns one page of tasks (each with `id`, `title`, `priority` of `urgent`, `high`, `normal`, or `low`, `area`, `estimate`, `state` of `open` or `closed`, and `assignee`, which is absent for an unassigned task) and a `next_cursor`, which is absent on the last page; it takes an optional `cursor`. `list-people` returns everyone who can take tasks, each with `name`, `skills` (a list of areas), `capacity`, and `load`. `assign-task` takes a `task_id` and an `assignee` name, assigns the task, and returns the person's new `load`; it fails when the person cannot take the task.

Assign the unassigned open tasks to people.

1. Call `list-people` once, at the start. Fetch every page of tasks with `list-tasks`: the first call has no `cursor`, each later call has the `next_cursor` of the previous page, and the pages are fetched one after another.
2. A task needs an assignee when its `state` is `open` and it has no `assignee`. Leave every other task alone. Handle the tasks that need an assignee one at a time, in this order: `urgent` first, then `high`, `normal`, and `low`; among equal priorities the task with the larger `estimate` first; among those the task whose `id` is smaller in ascending text order.
3. For a task, the people who may take it are those whose `skills` include the task's `area` and whose `capacity` minus `load` is at least the task's `estimate`, using the loads you know: those from `list-people`, with a person's load replaced by the `load` that `assign-task` returned after each successful assignment. If nobody has the skill, the task is `no-skill`. If people have the skill but none may take the task, it is `no-room`. Neither case calls `assign-task`.
4. Try the people who may take the task in ascending order of load, and by name in ascending text order among equal loads. Call `assign-task` for the first one. When it succeeds, the task is assigned to that person. When it fails, never call it again for that task and person, keep that person's load as it was, and try the next one. When every person has failed, the task is `refused`.
5. After all tasks are handled, call `list-people` a second time and report its loads.

Return a JSON object with these fields:

- `assigned`: `[{"task": ID, "assignee": NAME}, ...]` for the tasks that were assigned, in the order they were handled
- `unassigned`: `[{"task": ID, "reason": REASON}, ...]` for the tasks that were not, in the order they were handled, with `REASON` one of `no-skill`, `no-room`, or `refused`
- `loads`: `[{"name": NAME, "load": LOAD}, ...]` from the second `list-people`, in the order it lists them
