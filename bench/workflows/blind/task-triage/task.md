A task tracker offers three tools. `list-tasks` returns one page of tasks (an optional `cursor` names the page; the answer holds the `tasks` of the page and a `next_cursor`, which is empty after the last page). `assign-task` gives a task to an engineer (`id`, `assignee`) and takes an optional `status` of `open` or `in-progress` to set at the same time. `add-comment` adds a comment (`id`, `text`) to a task. A call that the tracker refuses, such as assigning a locked task, fails.

Triage the unassigned bugs among the engineers `ines`, `omar`, `priya`, and `tariq`.

1. Read the whole task list: the first request has no cursor, and each later request has the `next_cursor` of the previous page. Request each page once and one after another.
2. A task needs triage when its labels include the label `bug` (exactly, in lowercase), it has no assignee, and its status is `open`. A task with another status, such as `blocked` or `done`, is left alone.
3. Triage the tasks in ascending order of the number in their id (`T-9` before `T-12`, which comes before `T-100`), one at a time. Give each one to the engineer with the lowest load, and the first of them in the order `ines`, `omar`, `priya`, `tariq` when several have the lowest load. An engineer's load starts as the number of tasks in the list assigned to them whose status is not `done`, and it grows by one for each task you assign to them successfully.
4. A task of priority `p0` is assigned with the status `in-progress` in the same `assign-task` call, and right after that call succeeds you add the comment `Triaged to NAME (p0)` to it, with `NAME` the engineer's name. Any other task is assigned with no `status`, and gets no comment.
5. If `assign-task` fails for a task, do not retry it and do not add a comment for it. Its engineer's load does not grow. Go on with the next task.

Return a JSON object with these fields:

- `pages`: the number of pages requested
- `seen`: the number of tasks in the whole list
- `triaged`: one object `{"id": ID, "assignee": NAME}` for each task you assigned successfully, in the order you did it
- `failed`: the ids of the tasks whose assignment failed, in the order you tried them
- `load`: one object `{"engineer": NAME, "load": N}` for each engineer in the order `ines`, `omar`, `priya`, `tariq`, with the load as it is after your assignments
