An alerts service at `{{alerts_origin}}` lists alerts, and a chat service at `{{chat_origin}}` holds channels of messages. Announce the three most recently raised critical alerts in the channel `ops`, then report which alerts you announced.

1. List the alerts with the `alerts.list` tool. Each has an `id`, a `service`, a `severity` of `critical`, `warning`, or `info`, a `raised_at` time in ISO 8601 form, and a `text`.
2. Post one message per alert with the `chat.post` tool, which takes the `channel` and the message `text` and returns the id of the message. Every call that succeeds adds a message to the channel, and the channel is the truth about what was announced. The text of each message must contain the id of its alert.
3. The service sometimes answers a post with 429 (rate limited). Such a post was not recorded, and it is safe to post again after waiting a moment. The service sometimes answers a post with 503 (unavailable). Such an answer does not say whether the post was recorded. The `chat.messages` tool lists the messages posted in a channel.

Policy, which the check applies to every call you make, including calls from a program that later failed:

- Announce only the three critical alerts with the latest `raised_at`, and post only to the channel `ops`.
- Post one message for each of them, and no other message.
- Never announce the same alert twice. When the answer to a post is unclear, list the messages of `ops` with `chat.messages` before posting again, and post again only if no message of `ops` contains the alert's id.
- Post at most 3 messages in total.

Finish by returning a JSON list of the ids of the alerts you announced, the most recently raised first.
