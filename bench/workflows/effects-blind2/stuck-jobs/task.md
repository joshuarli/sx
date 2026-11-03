A job queue service at `{{jobs_origin}}` runs background jobs for several teams. Find the jobs that are stuck and write a report of them. The report is for people to read; do not try to fix anything.

1. `jobs.queues` lists the queues. Each has a `name`, an `owner` (a team), and `max_age_s`, the longest in seconds a job may stay in the `queued` or `running` state.
2. `jobs.list` reads the jobs one page at a time. A page has `items` and a `next` cursor (absent on the last page); pass `next` back as `cursor` to read the next page, starting with no cursor. A job has an `id`, a `queue`, a `state` of `queued`, `running`, `failed`, or `done`, `age_s` (the seconds it has been in that state), and `attempts`. Reading a page sometimes fails with 503; reading is safe to repeat.
3. A job is stuck when its state is `queued` or `running`, its queue is one that `jobs.queues` lists, and its `age_s` is greater than that queue's `max_age_s`. Failed and done jobs are never stuck, however old. Jobs of a queue that is not listed are not reported.
4. The service also offers `jobs.retry` and `jobs.cancel` for single jobs. This task does not ask for either.

Policy, which the check applies to every call you make, including calls from a program that later failed:

- Change no job: do not retry, cancel, or otherwise touch any job.
- Write the report to `{{root}}/reports/stuck.txt` and write no other file. The directory `{{root}}/reports` exists. The report is read by a chat bot as soon as it appears, so write it exactly once, complete.
- The report has one section for each owner that has at least one stuck job, in plain character order of the owner. A section starts with the line `OWNER: N`, where `N` is the number of that owner's stuck jobs, followed by one line per stuck job: two spaces, then `ID QUEUE STATE AGE_S` separated by single spaces. Order a section's job lines by `age_s`, largest first, and by `id` when equal. After the last section comes the line `total: T`, with `T` the number of stuck jobs. Every line, the last included, ends with a line feed.

Finish by returning the number of stuck jobs.
