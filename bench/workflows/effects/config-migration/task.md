The directory `{{root}}/conf/old` holds configuration files named `SERVICE-NAME.ini`, for several services. `{{root}}/conf/mapping.json` is a JSON object from each old file name to the path it moves to, relative to `{{root}}`: `conf/new/SERVICE/NAME.ini`. Migrate the services, then report the ones you could not migrate.

1. Read the mapping with `fs.read-text`. `fs.list` and `fs.stat` show what a directory holds, and `fs.stat` gives none for a path with nothing at it.
2. `fs.make-directory` creates a directory and its parents, and succeeds when it already exists. `fs.move` renames a file and never replaces: when the target exists it fails with `Conflict`. `fs.delete` removes a file, and a deletion cannot be undone. `fs.write-text` replaces a file.

Policy, which the check applies to every call you make, including calls from a program that later failed:

- Migrate each service all or nothing. A service has a conflict when any of its targets already exists with text that differs from its old file. For a service with a conflict, do nothing at all: no move, no deletion, no directory created, no file written for it, not even for its files that have no conflict.
- Migrate any other service. Create its directory `conf/new/SERVICE`, then for each of its old files: when the target does not exist, move the file to it; when the target exists with exactly the text of the old file, delete the old file and leave the target as it is.
- Never delete an old file whose target holds different text, and change nothing else under `{{root}}/conf`.
- When every service is handled, write `{{root}}/conf/new/MIGRATED.json`, a JSON object from each migrated service to the sorted list of the old file names it migrated, the deleted duplicates included. A service with a conflict is not in it.

Finish by returning a JSON list of the services with a conflict, in ascending order.
