The shift rosters for the last four weeks are CSV files in `{{root}}/rosters`, and the staff directory is a service at `{{hr_origin}}`. Work out who worked overtime and write one report file.

1. The rosters are the files named `week-N.csv` (a number for `N`); other files in the directory are not rosters. Each has a header line `staff,day,start,end,break_min` and one line per shift: a staff id, a weekday, start and end times as `HH:MM` on a 24-hour clock, and the unpaid break in minutes. A shift whose end is earlier than its start ends after midnight, on the next day. Some files use Windows line endings. Staff ids missing from the directory belong to contractors and are ignored.
2. `hr.staff` reads the directory one page at a time. A page has `items` and a `next` cursor (absent on the last page); pass `next` back as `cursor` to read the next page, starting with no cursor. A staff member has an `id`, a `name`, a `team`, and `contract_minutes`, the minutes of work per week their contract covers. Reading a page sometimes fails with 503; reading is safe to repeat.
3. A shift's worked minutes are its length in minutes minus its break. A person's overtime in one week is the worked minutes of that file beyond `contract_minutes`, or 0 if there are none. Each roster file is one week. A person's overtime is the sum over all the weeks.

Policy, which the check applies to every call you make, including calls from a program that later failed:

- Write the report to `{{root}}/out/overtime.csv` and write no other file. The directory `{{root}}/out` exists. A payroll job reads the report as soon as it appears, so write it exactly once, complete.
- The report starts with the line `name,team,overtime_min`, followed by one line `NAME,TEAM,MINUTES` for each staff member whose overtime is above 0, with the overtime in minutes. Order the lines by overtime, highest first, and by name, in plain character order, when overtimes are equal. Every line, the last included, ends with a line feed.
- Never write to the rosters.

Finish by returning the number of staff members in the report.
