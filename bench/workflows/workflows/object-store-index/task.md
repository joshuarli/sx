The directory `{{root}}/docs` holds documentation. Build an index of its Markdown files and publish it to the object store.

An indexed file is a file whose name ends in `.md`, located directly in `{{root}}/docs` or directly in one of its subdirectories (not deeper). Other files are not indexed. For each indexed file make an entry `{"path": PATH, "bytes": BYTES, "title": TITLE}`:

- `PATH` is the file's path relative to `{{root}}/docs`, with `/` between directory and file name, such as `guides/setup.md`
- `BYTES` is the file's size in bytes on disk (not its number of characters)
- `TITLE` is the text after the leading `# ` (a number sign and a space) of the first line of the file that starts with `# `, or the empty string if no line does

Order the entries by `PATH`, ascending. Store the index as the object `docs-index.json` with the `store.put` tool, passing `count` (the number of entries) and `files` (the entries). The object store requires a credential. The host adds it to every request for you: your program never has it, and must not set an `authorization` header itself.

Return a JSON object `{"count": COUNT, "etag": ETAG}`, with `ETAG` the `etag` the object store returned for the stored object.
