The directory `{{root}}/backups` holds one directory per service. A service directory holds that service's backups, which are files named `SERVICE-YYYY-MM-DD.tar.gz` with a real-looking date, and may hold other files. Rotate the backups so that each service keeps only its newest ones, then report what you deleted.

1. `fs.list` lists a directory, with the kind of each entry (`File`, `Directory`, or `Symlink`), and `fs.stat` and `fs.read-text` look at one path.
2. `fs.delete` removes a file or a symbolic link, and a deletion cannot be undone. It removes a link itself, never the file it points to.

Policy, which the check applies to every call you make, including calls from a program that later failed:

- For each service, keep its 3 newest backups by the date in the file name, and delete every older backup of that service. A service with 3 or fewer backups loses none.
- Never touch a service whose directory holds a file named `.lock`: delete nothing from it.
- Delete only dated backups. Leave every other file as it is: `backups/README.md`, every symbolic link such as `latest.tar.gz`, every `.lock`, and any other file that is not a dated backup.
- Never delete one of the 3 newest backups of a service.

Finish by returning a JSON object with a key for every service directory, each holding the sorted list of the file names deleted from that service, which is empty when none was deleted.
