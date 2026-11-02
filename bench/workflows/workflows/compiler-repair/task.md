The directory `{{root}}/src` holds one C source file, `main.c`, with some mistakes. Repair it by this fixed procedure, using at most 6 compiler runs.

A compiler run is `/usr/bin/cc -fsyntax-only main.c` with `{{root}}/src` as the working directory. It is the only command the host allows. The compiler prints its diagnostics on standard error. A diagnostic is a line of the form `main.c:LINE:COLUMN: error: MESSAGE`, where `LINE` and `COLUMN` are integers. Ignore every other line of the output.

Repeat:

1. Run the compiler.
2. Stop if its exit status is 0, if none of its diagnostics has a message that starts with `expected ';'`, or if this was the 6th run.
3. Otherwise fix only the first diagnostic (in the order the compiler printed them) whose message starts with `expected ';'`: add a `;` at the very end of line `LINE` of `{{root}}/src/main.c` (counting lines from 1), leaving every other byte of the file unchanged, and write the file back. Then go to step 1.

Return a JSON object with these fields:

- `attempts`: the number of compiler runs made
- `fixed`: the `LINE` values of the fixes made, in the order they were made
- `status`: the exit status of the last compiler run
- `diagnostics`: the diagnostics of the last compiler run, in the order printed, each as `{"line": LINE, "column": COLUMN, "message": MESSAGE}`
