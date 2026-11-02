The directory `{{root}}/logs` holds application logs. Summarize the errors in them and write the summary to `{{root}}/summary.txt`.

Read every file directly in `{{root}}/logs` whose name ends in `.log`. Ignore all other files and every subdirectory. A file's lines end with `\n` or with `\r\n`; a `\r` is not part of a line's content.

Split each line at single spaces into fields. A line is an error record when it has at least 4 fields and its second field is exactly `ERROR`. Its third field is the service and its fourth field is the error code. Every other line is not a record: lines at other levels, lines with fewer fields, blank lines, and indented continuation lines, even when the word `ERROR` appears somewhere in them.

Count the records per error code and per service over all the files. Order each count list by count from largest to smallest, and by name in ascending order among equal counts.

Write `{{root}}/summary.txt` as exactly these lines, each ending with `\n`, and no other text:

```
files: FILES
errors: ERRORS
by code:
CODE: COUNT
...
by service:
SERVICE: COUNT
...
```

`FILES` is the number of `.log` files read and `ERRORS` is the number of records. There is one `CODE: COUNT` line for each error code and one `SERVICE: COUNT` line for each service, in the order above.

Return a JSON object with these fields:

- `files`: the number of `.log` files read
- `errors`: the number of records
- `by_code`: `[{"code": CODE, "count": COUNT}, ...]` in the order above
- `by_service`: `[{"service": SERVICE, "count": COUNT}, ...]` in the order above
