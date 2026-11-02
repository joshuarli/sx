### How the database tools behave

- Each call runs one declared statement in its own transaction. A failing call changes nothing.
- A constraint violation, such as a duplicate primary key, fails with `Conflict`. A busy database
  fails with `Unavailable` after 5 seconds.
- A row's column that is `NULL` reads as `none` in an `Option` field.
