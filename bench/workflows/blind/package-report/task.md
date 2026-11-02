The directory `{{root}}/packages` holds one directory per JavaScript package. Each package directory normally holds a `package.json`. Write a CSV report of the packages to `{{root}}/report.csv`.

Reading the packages:

1. Look only at the directories directly inside `{{root}}/packages`; files there are ignored. A directory with no `package.json` file is `no_manifest`. A `package.json` that is not valid JSON is `invalid`, and so is an empty one. A valid manifest with `"private": true` is `private`. A package that is `no_manifest`, `invalid`, or `private` gets no row in the report.
2. For every other package, take these values from its manifest. A key whose value is `null` counts as absent.
   - `name`: the `name` string, or the name of the package's directory when `name` is absent.
   - `version`: the `version` string.
   - `license`: when `license` is a string, that string. When `license` is an object, its `type` string. Otherwise, the `type` string of the first entry of the `licenses` array. When none of these gives a value (including an empty `licenses` array), `UNLICENSED`.
   - `author`: when `author` is a string, the part before the first ` <` (a space and a less-than sign), or the whole string when it has none, without spaces at either end. When `author` is an object, its `name` string. When `author` is absent, `unknown`.
   - `deps` and `dev_deps`: the number of entries in the `dependencies` object and in the `devDependencies` object, `0` when absent.

The report:

3. The first line is `name,version,license,author,deps,dev_deps`. Then one line per package, ordered by `name`, comparing the names by Unicode code point (so every uppercase letter sorts before every lowercase letter), and two packages with the same `name` ordered by the name of their directory. Each line lists the six values in the header's order, separated by commas.
4. A text value that contains a comma or a double quote is written in double quotes, with every double quote inside it doubled. No other value is quoted. Text is written as UTF-8, and lines end with a line feed, the last one included. No spaces are added anywhere.

Return a JSON object with these fields:

- `rows`: the number of package lines in the report
- `private`, `invalid`, `no_manifest`: the directory names of the packages in that state, in ascending order
- `licenses`: one object `{"license": LICENSE, "count": N}` for each distinct license in the report rows, ordered by `count` from highest to lowest and then by license text in ascending code point order
