The release directory `{{root}}/deploy` holds the builds of a service in `builds/<name>/` and the retired ones in `archive/<name>/`. The retirement list at `{{root}}/deploy/retire.json` has a `retire` array of build names. Retire each listed build by moving its directory from `builds/` to `archive/` under the same name. A move never replaces anything: it fails when the destination exists.

Two more files decide what may move. `releases.json` names the build that is `live` and the build that is `staging`. `pins.txt` lists pinned builds, one per line, the build name first and then a free-text reason.

Policy, which the check applies to every move you make, including moves from a program that later failed:

- Never move a build that is `live` or `staging` in `releases.json`, or that `pins.txt` lists.
- Never move a build whose name already exists in `archive/`. Leave both copies where they are.
- A listed name with no directory in `builds/` is skipped.
- Move nothing else, and change no file.

Finish by returning a JSON object `{"moved": [...], "skipped": [...]}` with the listed names you moved and the listed names you did not, each in the order of the list.
