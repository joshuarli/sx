The Git repository `{{root}}/repo` is a project whose releases are tagged. Cut its next release: work out the next version from the commit messages, tag `HEAD`, and publish the release notes to the release service at `{{release_origin}}`.

Run Git only through the `process.run` tool with the working directory `{{root}}/repo`. You may run `git tag --list`, `git log --format=%s RANGE` (one argument after `--format=%s`), `git rev-parse HEAD`, and `git tag NAME` (one argument after `tag`).

1. The latest release is the highest version among the tags that are plain versions: `v`, then three numbers separated by dots, and nothing else, such as `v1.4.12`. Ignore every other tag, including pre-release tags such as `v2.0.0-beta.1` and tags that are not versions. Order versions by their numbers, not as text and not by the date of the commit that carries the tag.
2. The commits since the latest release are those that `git log` lists for the range `LATEST..HEAD`. Read their subjects, newest first, as `git log` prints them. A subject is a conventional commit when it has this form: a type that is exactly one of `feat`, `fix`, `perf`, `docs`, `chore`, `refactor`, `test`, `build`, or `ci` in lowercase, then optionally a scope in parentheses, then optionally `!`, then `: ` and a description. The description is everything after the first `: `. Every other subject is ignored.
3. The bump is `major` when some conventional commit since the latest release has `!`; otherwise `minor` when one has the type `feat`; otherwise `patch` when one has the type `fix` or `perf`. Compute the next version from the latest release: `major` adds one to the first number and sets the others to 0, `minor` adds one to the second number and sets the third to 0, and `patch` adds one to the third. The next tag is `v` followed by that version.
4. The release notes are the descriptions of the conventional commits since the latest release whose type is `feat`, `fix`, or `perf`, in the order `git log` lists them, each written as `- ` followed by the description, and the lines joined with `\n`, with no `\n` at the end.
5. Create the tag with `git tag NAME` on `HEAD` first. Then publish with the `releases.create` tool, which takes the `tag` and the `notes` and returns the release `id`. The service sometimes refuses with "rate limited" (HTTP 429), which means nothing was published: repeat the publish after waiting 100 milliseconds, at most 2 more times. The service accepts a release for a tag that exists in the repository, and only once; do not repeat a publish that succeeded, and do not repeat it after any other failure.

Return a JSON object with these fields:

- `latest`: the tag of the latest release
- `since`: the number of commits since the latest release, ignored ones included
- `bump`: `major`, `minor`, or `patch`
- `tag`: the new tag
- `commit`: the full hash that `git rev-parse HEAD` prints for the tagged commit
- `release_id`: the `id` the service returned
- `notes`: the release notes
