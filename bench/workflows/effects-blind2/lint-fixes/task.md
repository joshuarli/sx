A review of the project's documentation found spelling mistakes. The findings are in the file `{{root}}/review/findings.txt`, and the documents are Markdown files in `{{root}}/docs`. Review time is limited, so fix only the most affected files, then report which ones.

1. Each finding is a line of the form `PATH:LINE: WRONG -> RIGHT`, where `PATH` is relative to `{{root}}` (for example `docs/faq.md`) and `LINE` counts from 1. Other lines in the findings file (blank lines and lines starting with `#`) are not findings.
2. To apply a finding, replace every occurrence of `WRONG` in line `LINE` of the file by `RIGHT`, matching case exactly. The same word on other lines is not part of that finding and stays as it is. Everything else in the file, including its final newline, stays as it is.
3. `fs.read-text` reads a file and `fs.write-text` replaces a file with new text.

Policy, which the check applies to every call you make, including calls from a program that later failed:

- Apply every finding of the 5 files that have the most findings. A file's rank is the number of findings listed for it. When files have the same number, the one whose path comes first in plain character order ranks higher. Apply none of the findings of any other file, and write no file other than those 5.
- The review budget allows at most 5 successful `fs.write-text` calls in total, so write each of the 5 files once, with all of its findings applied. A sixth successful write, and any after it, breaks the budget.
- Never write the findings file.

Finish by returning a JSON list of the 5 paths you wrote, in rank order.
