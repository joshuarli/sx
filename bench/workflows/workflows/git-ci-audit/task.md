The file `{{root}}/repos.txt` lists 100 repository names, one per line (`repo-000` to `repo-099`). Each repository should be a local Git checkout at `{{root}}/repos/NAME`. A few of the directories do not exist, and a few exist but are not working Git repositories: `git status` fails there, with a nonzero exit status. Call a repository unreadable if its directory is missing or if `git status --porcelain` exits with a nonzero status.

The host allows only these Git invocations, run with `/usr/bin/git` and the repository as the working directory:

- `git status --porcelain`
- `git rev-parse HEAD`
- `git branch --show-current`
- `git rev-list --count main..HEAD`

A readable repository is clean when `git status --porcelain` prints nothing at all (an untracked file makes it dirty). For each clean repository, find the full hash of its HEAD commit, its current branch name, and `ahead`, the number of commits that HEAD has and `main` does not.

A CI service answers `GET {{ci_origin}}/repos/NAME/commits/HASH/checks` with a JSON object whose `state` is `success`, `failure`, or `pending`. It answers with a "not found" error when it has no checks for that commit. Ask CI once for each clean repository's HEAD commit, and for no other commit. Run the CI requests concurrently, with at most 8 in flight at once. Do not modify any repository.

Return a JSON object with three fields:

- `failing`: one object `{"repo": NAME, "branch": BRANCH, "ahead": AHEAD}` for each clean repository whose HEAD commit's CI state is `failure`, sorted by `ahead` from largest to smallest, and by `repo` in ascending order among equal values
- `unreadable`: the names of the unreadable repositories, in ascending order
- `unknown`: how many clean repositories had no CI checks for their HEAD commit (CI answered "not found")
