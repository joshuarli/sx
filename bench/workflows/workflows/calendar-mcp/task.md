A calendar service offers two tools, `get-availability` and `book-meeting`. Times are minutes after midnight, so `540` is 09:00, and all of them fall on the day `2026-03-02`.

`get-availability` takes a `person` and a `day` and returns that person's `busy` intervals (each with a `start`, an `end`, and a `title`) and, only for people who work different hours from the usual ones, their `working_hours` (`start` and `end`). A person without `working_hours` works from `540` to `1020`. An interval covers the times from its `start` up to but not including its `end`, so an interval ending at `600` and one starting at `600` do not overlap.

`book-meeting` takes a `title`, a `day`, a `start`, an `end`, and the `attendees`, and books the meeting. It returns the `meeting`, with its `id` and, only when the service assigns one, a `room`. The service refuses a booking, with an error result, when an attendee is not free for the whole time. It knows of commitments that `get-availability` does not show.

Book these three meetings, in this order, all on `2026-03-02`:

| Title | Attendees | Length |
| --- | --- | --- |
| `Planning` | `ana`, `ben`, `chen` | 60 minutes |
| `Design review` | `ben`, `chen`, `dara` | 45 minutes |
| `Retro` | `ana`, `ben` | 30 minutes |

1. Read each of the four people's availability exactly once, with `day` `2026-03-02`, and do not read it again later. The reads may run concurrently.
2. Choose each meeting's time from what you read and from the meetings you have booked yourself. A candidate start is a multiple of 15 minutes. A candidate slot, from its start for the meeting's length, must lie inside every attendee's working hours and must not overlap any of their busy intervals or any meeting you booked earlier in this task that they attend.
3. Book each meeting at its earliest candidate slot. Book one meeting at a time, and book the next meeting only when the current one is booked.
4. If the service refuses a booking, take the earliest candidate slot whose start is later than the refused slot's start and try that. A meeting is tried at most 3 times. Do not retry a booking that succeeded, and do not book anything else.

Return a JSON object `{"meetings": [...]}` with one object for each meeting, in the order above, `{"title": TITLE, "id": ID, "start": START, "end": END, "room": ROOM, "refused": REFUSED}`, with `ROOM` the room the service assigned or `null`, and `REFUSED` the number of times the service refused that meeting before it was booked.
