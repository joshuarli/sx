The Git repository at `{{root}}/repo` is about to get a new release. Write the release notes for the commits since the latest release.

1. Find the latest release. Release tags are the tags of the form `vMAJOR.MINOR.PATCH`, each part a decimal number (`v1.4.12`); any other tag, such as one with a suffix like `-rc.1`, is not a release. The latest release is the release tag with the highest version, comparing the three numbers as numbers, so `v1.10.0` is higher than `v1.9.0`.
2. List the commits after that tag up to `HEAD`, newest first. Run Git with the `process` tool in the directory `{{root}}/repo`. These are the commands you may use: `git tag --list`, and `git log` with exactly two arguments after `log`, the format `--format=%H %s` (full hash, a space, the subject) and a revision range such as `TAG..HEAD`.
3. A subject describes a change when it has the form `TYPE: DESCRIPTION`, `TYPE(SCOPE): DESCRIPTION`, `TYPE!: DESCRIPTION`, or `TYPE(SCOPE)!: DESCRIPTION`, where `TYPE` is one or more lowercase letters, `SCOPE` is any text without parentheses, and `DESCRIPTION` is the rest of the subject after the colon and one space. A `!` before the colon marks a breaking change. A commit whose subject has another form is not listed.
4. Which listed commits appear in the notes:
   - Every breaking change appears, whatever its type, under `Breaking changes`, and nowhere else.
   - A change that is not breaking appears only when its type is `feat` (under `Features`), `fix` (under `Fixes`), or `perf` (under `Performance`). Changes of any other type are left out.
5. Write the notes to `{{root}}/out/RELEASE_NOTES.md`. The directory already exists. The file is exactly:
   - the line `## Changes since TAG`, with the latest release tag in place of `TAG`;
   - then, for each section that has at least one entry, a blank line, the line `### TITLE`, and the entries, one per line. The sections come in this order: `Breaking changes`, `Features`, `Fixes`, `Performance`. A section with no entries is left out entirely;
   - each entry is `- **SCOPE:** DESCRIPTION (HASH)` when the subject has a scope, and `- DESCRIPTION (HASH)` when it has none, where `HASH` is the first 7 characters of the commit hash;
   - entries within a section are in the order of the log, newest first;
   - the file ends with exactly one newline, and has no trailing spaces.

Return a JSON object with these fields:

- `tag`: the latest release tag
- `commits`: the number of commits after the tag
- `listed`: the number of entries written to the file, over all sections
- `skipped`: the number of commits after the tag that are not in the notes
- `sections`: one object `{"title": TITLE, "entries": N}` for each section written, in file order
