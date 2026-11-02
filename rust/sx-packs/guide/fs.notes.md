### How the file tools behave

- Every path is absolute. A path outside the granted directories fails with `PermissionDenied`.
- A path that does not exist fails with `NotFound`, except `fs.stat`, which returns `none` for it.
- `fs.read-text` fails with `Invalid` for a directory or a file that is not UTF-8 text.
- `fs.list` returns the entries of one directory, sorted by name, each with `name` and `kind`
  (`File`, `Directory`, `Symlink`, or `Other`). It does not descend into subdirectories.
- `fs.write-text` creates or replaces the whole file atomically and keeps an existing file's mode. Its
  directory must already exist.
