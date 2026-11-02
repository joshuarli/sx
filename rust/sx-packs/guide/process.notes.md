### How the program tool behaves

- Only the granted programs run, with argument lists the host allows exactly. Any other program or
  argument list fails with `PermissionDenied` before anything starts.
- `program` is an absolute path. `cwd` is an absolute directory inside the granted roots. A `cwd`
  that does not exist fails with `NotFound`, and one that is not a directory with `Invalid`.
- A nonzero exit status is not a failure: the result has `status`, `stdout`, and `stderr`.
- The program gets an empty environment and no input unless `stdin` is given. It has 60 seconds, and
  each output stream is capped at 1 MiB; going past either fails the call.
