An issue tracker at `{{tracker_origin}}` lists issues in numbered pages. A chat service at `{{chat_origin}}` takes messages. Tell the on-call channel about every critical issue that is open and was not announced before. A SQLite database remembers which issues were announced.

1. Read the pages with the `issues.page` tool, with `per_page` 20 and `page` 1, 2, 3, and so on, one request after another. Each page lists up to 20 issues. The first page that has no issues ends the list; that empty page is fetched too, and no page after it is. Do not request any other page, and request each page once.
2. The tracker may list one issue on two consecutive pages. The later copy is the current one; the earlier copy is stale.
3. A current issue is a candidate when its `severity` is `critical` and its `state` is `open`.
4. The database `{{root}}/notified.db` has the table `notified` with the column `issue_id`, one row for each issue announced earlier. Read the announced issue ids with the `notified.ids` tool. A candidate whose id is in the table is already announced: do nothing for it.
5. For each other candidate, in ascending order of `id` and one at a time, post a message with the `chat.post` tool, with `channel` `#oncall` and `text` exactly `Critical issue #ID: TITLE`, for example `Critical issue #1043: Payment gateway timeout`. After the post succeeds, record the issue with the `notified.add` tool, with its `issue_id` and `title`.
6. The chat service sometimes fails a post with "unavailable" (HTTP 503). The message may or may not have been delivered, so do not repeat that post and do not record the issue. Carry on with the next candidate.

Return a JSON object with these fields:

- `pages`: the number of pages fetched, the empty page included
- `candidates`: the number of candidates
- `already`: the ids of the candidates that were already announced, in ascending order
- `notified`: the ids of the issues posted and recorded, in ascending order
- `failed`: the ids of the issues whose post failed, in ascending order
