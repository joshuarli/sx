The directory `{{root}}/project` holds a project with sloppy text files. Tidy them in place and report what you did.

1. Consider the files directly inside `{{root}}/project/src` and `{{root}}/project/docs`. Do not look in any subdirectory of those two directories. A file is considered when it is a regular file, not a symbolic link, and its name ends with `.md`, `.txt`, or `.py` in lowercase. Every other entry is left alone and not reported. A file's path is its path relative to `{{root}}/project`, such as `src/main.py`.
2. The file `{{root}}/project/.tidyignore` lists paths to leave alone. Its lines are trimmed of leading and trailing whitespace. A blank line, and a line starting with `#`, is not an entry; every other line is one path. A considered file whose path is an entry is `ignored`: do not read it or change it. An entry that names no considered file means nothing.
3. Read every other considered file as text. A file that cannot be read as text because it is not valid UTF-8 is `skipped`: leave it alone.
4. The tidy form of a text is made by these steps, in this order:
   - every `\r\n` becomes `\n`;
   - spaces and tabs at the end of each line are removed;
   - in each line, every tab at the very start, before any other character, becomes four spaces (tabs after another character are left as they are);
   - blank lines at the end of the text are removed, and the text ends with exactly one `\n`; a text with no characters left is the empty text, which has no `\n`.
5. Write a file back with the `fs.write-text` tool only when its tidy form differs from the text read, and write the tidy form. Never write a file whose text is already tidy, and never write an ignored or skipped file.

Return a JSON object with these fields:

- `changed`: the paths of the files you wrote, in ascending text order
- `unchanged`: the number of files that were read and already tidy
- `ignored`: the paths of the ignored files, in ascending text order
- `skipped`: the paths of the skipped files, in ascending text order
- `delta`: the total of the new length minus the old length, in characters, over the files you wrote (negative when the files shrank)
