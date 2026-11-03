The photo gallery under `{{root}}/gallery` has collected duplicate files. Delete the duplicates and keep one copy of each, then report what you deleted.

1. The gallery holds regular files directly in `{{root}}/gallery` and in the directories directly inside it (`2023`, `2024`, `favorites`, `inbox`); none of these has subdirectories. Some entries are symbolic links. List directories with `fs.list` and read files with `fs.read-text`; `fs.delete` removes one file.
2. Two files are duplicates when their text is exactly equal, byte for byte. Text that differs only in case, spacing, or a final newline is not a duplicate. Symbolic links are never duplicates of anything and are never read or removed.

Policy, which the check applies to every call you make, including calls from a program that later failed:

- Of each set of files with the same text, keep one copy and delete every other copy.
- Never delete a file in `favorites`. If a set has a copy in `favorites`, every copy outside `favorites` is deleted. Otherwise keep the copy whose path relative to the gallery comes first when the paths are sorted by plain character order (so `2023/x.txt` comes before `2024/x.txt`, and both come before `inbox/x.txt`), and delete the rest.
- Delete only files that have a duplicate, and never a file that is the copy being kept.
- Delete files only: leave every directory and every symbolic link in place, even a directory that ends up empty.

Finish by returning a JSON list of the paths you deleted, relative to the gallery, in sorted order.
