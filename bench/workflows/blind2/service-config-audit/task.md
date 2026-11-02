The directory `{{root}}/services` holds the configuration of a company's services. Audit it and write the findings to `{{root}}/audit.tsv`.

1. Find the services. A directory is a service when it directly contains a file named `service.json`. The service's path is the directory's path relative to `{{root}}/services`, with `/` between names, such as `billing` or `platform/auth`. Start at `{{root}}/services` itself: it is never a service, but its subdirectories are checked, and so on. Search only down to 3 levels below `{{root}}/services`. Do not look inside a service's subdirectories: a service's own directory is the end of the search along that branch. Skip every directory whose name starts with `_` or `.`, wherever it is. Ignore every file that is not a `service.json` of a service.
2. Read each service's `service.json`. When its text is not valid JSON, or is empty, the service's status is `invalid` and it has no other findings.
3. Otherwise check these rules. A broken rule is a finding, named as shown:
   - `name`: the field `name` is not a JSON string equal to the last name of the service's path.
   - `version`: the field `version` is not a JSON string of three non-negative numbers separated by dots, such as `2.3.1`, with nothing else.
   - `port`: the field `port` is not a JSON number that is an integer from 1024 to 65535. A string, `null`, or a missing field breaks the rule.
   - `port-conflict`: another valid service has the same `port` as this service, and both ports obey the previous rule. Every service that shares such a port gets this finding.
   - `repository`: the field `repository` is neither a JSON string starting with `https://` nor a JSON object whose `url` is a JSON string starting with `https://`.
   - `owner`: the field `owner` is not a non-empty JSON string.
   A valid service has status `ok` when it has no findings and `fail` otherwise.
4. Write `{{root}}/audit.tsv` with one line for each service, ordered by path in ascending text order. Each line is the path, a tab, the status, a tab, and the findings, in the order of the list above, joined with `,` and with no spaces. The findings are empty for `ok` and `invalid`. Every line ends with `\n`, and there is no other text.

Return a JSON object with these fields:

- `services`: the number of services
- `ok`, `fail`, `invalid`: the number of services with each status
- `invalid_paths`: the paths of the invalid services, in ascending text order
- `findings`: `[{"finding": NAME, "count": COUNT}, ...]` for every finding that occurred, with `COUNT` the number of services that have it, ordered by count from largest to smallest and by name in ascending text order among equal counts
