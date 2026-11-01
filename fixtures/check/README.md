# Checker fixtures

Each fixture is a program in `NAME.sx` and the checker's full output in `NAME.expected`. An empty
`.expected` file means the program must check with no diagnostics.

The first line of every program is `; task: TASK-ID`. The checker reads the types, enums, cores,
and capabilities of that task in `bench/codemode/tasks/`.

The programs are model answers from the syntax pilot, copied verbatim. Each one is either a
construct the pilot made legal, or a mistake that stays an error, with the diagnostic the checker
must print for it.
