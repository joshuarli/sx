The company directory at `{{dir_origin}}` knows every employee. A local SQLite database, `{{root}}/people.db`, holds a copy in its `people` table. Bring the copy up to date.

1. List the employees with the `people.list` tool, which takes an `offset` and a `limit` and returns the `total` number of employees and the `people` of that page, each with an `id`. Ask for pages of 20: the first request has offset 0 and each later one starts where the previous page ended. Ask for no page that starts at or beyond `total`.
2. Fetch the details of each listed employee with the `people.get` tool, exactly one fetch per employee, and never more than 4 fetches in flight at once. A fetch can fail:
   - an employee that has left since the listing was made is answered "not found" (HTTP 404). Call that employee `gone`, and do not fetch again;
   - the service sometimes refuses with "rate limited" (HTTP 429). Fetch again after waiting 50 milliseconds, and if that is refused too, once more after waiting 150 milliseconds. If the third request is also refused, call that employee `failed`. Do not repeat a fetch after any other failure.
3. The database stores each email in lowercase with no surrounding spaces, so normalize the email from the directory that way before you compare or store it. Names and teams are stored as they are, and a missing team is stored as NULL.
4. Compare each employee you fetched with the database row of the same `id`, which you can read with `people.all`:
   - no row: insert it with `people.insert`;
   - a row that differs in the name, the normalized email, or the team: change it with `people.update`;
   - an identical row: leave it alone, and issue no write for it.
   Leave the row of a `gone` or `failed` employee as it is. Delete with `people.delete` every row whose `id` was not in the listing at all.

Return a JSON object with these fields, each list in ascending `id` order:

- `listed`: the number of employees in the listing
- `inserted`, `updated`, `deleted`: the ids of the rows you inserted, updated, and deleted
- `unchanged`: the number of fetched employees whose row needed no change
- `gone`, `failed`: the ids of the employees in those states
