The directory `{{root}}/manifests` holds service manifests, files named `NAME.yaml`. `legacy.yaml` uses YAML constructs that the editing tools here do not support: do not read it and do not change it.

For every other `*.yaml` file directly in the directory, in ascending order of file name, apply these rules to the file's text:

1. Read `metadata.version`, a version of the form `MAJOR.MINOR.PATCH` with three non-negative integers. If `metadata.owner` is `frozen`, skip the file: change nothing and write nothing.
2. Otherwise let the new version be `MAJOR.MINOR.(PATCH+1)`, with PATCH increased by one as an integer (`7.12.99` becomes `7.12.100`). Set `metadata.version` to it.
3. If `spec.image.tag` exists and equals the old version, set it to the new version too. Otherwise leave it.
4. If `spec.replicas` exists and is less than 2, set it to `2`.
5. Write the changed text back to the same file.

A value you change keeps the way it was written: a double-quoted value stays double-quoted, a single-quoted value stays single-quoted, and a bare value stays bare. Everything else in a file must stay byte for byte as it was, including comments, blank lines, line endings, key order, and indentation. A skipped file must be left untouched.

Return a JSON object with two fields:

- `updated`: for each file written, in ascending order of file name, `{"file": FILE, "version": NEW_VERSION}`, with `FILE` the complete file name, such as `api.yaml`
- `skipped`: the complete file names of the skipped files, in ascending order
