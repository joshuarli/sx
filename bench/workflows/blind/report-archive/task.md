The directory `{{root}}/reports` collects report files, and `{{root}}/archive` stores old ones. Move the old reports into the archive.

1. Look at the files directly inside `{{root}}/reports` (a directory there is not touched). A report is a file whose name has the form `NAME-YYYY-MM-DD.EXT`: `NAME` and `EXT` are one or more lowercase letters, and the date is a real-looking date, with a four-digit year, a month from `01` to `12`, and a day from `01` to `31`, each written with two digits. Any other file is ignored: it stays where it is.
2. A report is old when its date is strictly before `2025-12-10`. Reports dated `2025-12-10` or later stay where they are.
3. Handle the old reports in ascending order of their date, and reports with the same date in ascending order of their name. For a report named `NAME-YYYY-MM-DD.EXT`, the archive directory is `{{root}}/archive/YYYY/MM`, which may not exist yet. Create it, with its parents, when it is missing; create no directory that is not needed. Then:
   - when the archive directory holds no file named like the report, move the report there under its own name;
   - when it holds a file of that name with exactly the same text as the report, the report is a duplicate: delete the report, and create nothing in the archive;
   - when it holds a file of that name with different text, move the report there under the name `NAME-YYYY-MM-DD.N.EXT`, with `N` the smallest number from 1 up for which no file of that name is in the archive directory.
4. `{{root}}/archive/INDEX.txt` already exists and lists the files archived by earlier runs. Keep its text as it is and add one line per old report, in the order of step 3, after the existing lines: `ORIGINAL -> YYYY/MM/FINAL` for a moved report, with `ORIGINAL` the name it had in `reports` and `FINAL` the name it was given in the archive, and `ORIGINAL (duplicate)` for a duplicate. Every line, the last included, ends with a line feed.

You may run `mkdir -p DIRECTORY`, `mv SOURCE DESTINATION`, and `rm FILE` (each with absolute paths, as the programs `/bin/mkdir`, `/bin/mv`, and `/bin/rm`) through the `process` tool. Give `{{root}}` as the working directory.

Return a JSON object with these fields:

- `archived`: one object `{"from": ORIGINAL, "to": "YYYY/MM/FINAL"}` for each moved report, in the order of step 3
- `duplicates`: the names of the duplicate reports, in the order of step 3
- `kept`: the number of reports that are not old
- `ignored`: the names of the ignored files, in ascending order
