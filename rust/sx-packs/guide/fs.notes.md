### How the file tools behave

- Every path is absolute. A path outside the granted directories fails with `PermissionDenied`.
- A path that does not exist fails with `NotFound`, except `fs.stat`, which returns `none` for it.
- `fs.read-text` fails with `Invalid` for a directory or a file that is not UTF-8 text.
- `fs.list` returns the entries of one directory, sorted by name, each with `name` and `kind`
  (`File`, `Directory`, `Symlink`, or `Other`). It does not descend into subdirectories.
- `fs.write-text` creates or replaces the whole file atomically and keeps an existing file's mode. Its
  directory must already exist.
- `fs.make-directory` creates the directory and its missing parents, and succeeds when it already
  exists, including a granted directory itself. A path that exists as a file or a symlink fails with
  `Invalid`.
- `fs.move` renames a file, symlink, or directory and never replaces: a destination that exists
  fails with `Conflict`. Its destination directory must already exist. It is not idempotent.
- `fs.delete` removes a file, a symlink (not what it points to), or an empty directory. A directory
  with anything in it fails with `Invalid`, and nothing is deleted. A path that is not there fails
  with `NotFound`.
- A path that already exists where a new one is wanted fails with `Conflict`.
