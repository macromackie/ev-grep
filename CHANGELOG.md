# Changelog

## v0.2.4

- Use `--endpoint` or `EV_GREP_ENDPOINT` to send requests through a trusted provider proxy.

## v0.2.3

- `--jobs N` controls concurrent requests, from 1 to 256. The default remains four. JSONL begin records include the chosen limit.
- The review guide shows how to retain uncertain candidates across two searches. The code-scanning example now treats interrupted commands as failures.
- Includes the line-range and SARIF support described below.

## v0.2.2

Search part of a file by adding `:N` or `:N-M` to a path, such as `ev-grep 'Writes to the database' src/users.py:40-72`. Lines count from 1 and include both ends. ev-grep still sends the whole file for context, and Jev judges only those lines. Terminal output prints the same `path:N-M` form, and JSON records add `start_line` and `end_line`. [Use with other tools](https://ev-grep.com/docs/pipelines) shows how to get paths and ranges from `rg`, `git diff`, and ast-grep.

`--sarif` writes one SARIF 2.1.0 log to stdout when the search ends. Matches become warnings and uncertain results become notes, so GitHub code scanning and other SARIF tools can show them. [Output](https://ev-grep.com/docs/output#sarif) describes the mapping, and the pipelines guide has a [GitHub Actions workflow](https://ev-grep.com/docs/pipelines#upload-results-to-github-code-scanning). `--sarif` cannot be combined with `--json`.

Two changes may affect scripts:

- A path that ends in `:` and a number and does not exist is now read as a line range. An existing file with such a name is still searched whole. A malformed range such as `:0` or `:9-3`, or a range past the end of the file, is an error.
- The stderr summary starts with `N selected` instead of `N files selected`, because a selection can be a range. JSON output is unchanged apart from the new line fields.

Whole-file searches send the same request as 0.1.4. Versions 0.2.0 and 0.2.1 were not published; their changes are in this release.
