The directory `{{root}}/manifests` holds the dependency manifests of several projects, files named `PROJECT.deps`. Find which dependencies are outdated by asking the package registry at `{{registry_origin}}`. Ignore files whose names do not end in `.deps`.

A manifest is text with lines ending in `\n`. A line that is blank or starts with `#` is ignored. Every other line is `PACKAGE==VERSION`, and spaces may surround `PACKAGE` and `VERSION`. A `VERSION` is `MAJOR.MINOR.PATCH`, three non-negative integers without leading zeros. The project is the file name without `.deps`.

Ask the registry about each distinct package name that appears in any manifest, exactly once, with the `registry.versions` tool. Run these requests concurrently, with at most 4 in flight at once. The tool returns the package's `versions`, in no particular order, or fails with "not found" when the registry does not know the package. The list may contain versions that are not exactly `MAJOR.MINOR.PATCH` of three integers, such as `2.0.0-beta.1`: ignore those. The latest version of a package is the greatest of the rest, by `MAJOR`, then `MINOR`, then `PATCH`, compared as numbers (`1.10.0` is greater than `1.9.0`).

A dependency is outdated when its latest version is greater than its version in the manifest. Its kind is `major` when the two differ in `MAJOR`, else `minor` when they differ in `MINOR`, else `patch`. A dependency whose version equals or exceeds the latest version is not outdated. A dependency whose package the registry does not know is missing.

Return a JSON object with these fields:

- `outdated`: one object `{"project": PROJECT, "package": PACKAGE, "current": VERSION, "latest": LATEST, "kind": KIND}` for each outdated dependency, in ascending order of project, then of package
- `missing`: one object `{"project": PROJECT, "package": PACKAGE}` for each missing dependency, in ascending order of project, then of package
- `counts`: an object `{"major": N, "minor": N, "patch": N}` with the number of outdated dependencies of each kind
