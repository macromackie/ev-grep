# Changelog

## v0.4.4

- Remove `--sarif`. Use text output for terminals and `--json` for integrations.
- Retry a provider response that fails validation, such as probabilities that do not sum to one, within the existing
  three-attempt budget instead of failing the search.
- Rust API: remove `scan` and `scan_with_jobs`; call `scan_inputs` with file inputs instead.

## v0.4.3

- Recover transient provider failures within a bounded request budget. A slow Jev request may use one spare worker to race a duplicate; the first valid answer wins.
- Include request attempts, hedges, elapsed time, and available usage in JSON assessments.
- Support multiple named questions over shared context in the Rust adapter.

## v0.4.2

- Add `--sort score` to order results by match probability. Streaming remains the default; sorting makes no extra model requests.
- Refine Jev instructions to make decisive judgments from visible code while retaining uncertainty for missing definitions. Queries about declarations can match declarations.

## v0.4.1

- Retry temporary HTTP failures up to three attempts within the request timeout.
- Include upstream request IDs in HTTP errors when available, to help diagnose failed requests.

## v0.4.0

- Search raw text with `--stdin`, or pass ast-grep JSON Lines with `--candidates FILE` (`-` reads stdin).
- Keep exact candidate text and source ranges in JSONL v2. Optional full-file context is supplied explicitly;
  candidate paths are never opened. JSON includes all three model probabilities and raw confidence.
- Terminal matches show their match probability. SARIF preserves candidate columns, including Unicode positions.
- Clarify the search prompt's distinction between missing helper behavior and evidence of absence.

## v0.3.0

- Set `--min-confidence` to control which search results remain uncertain. JSON retains the original answer and score.
- Support structured context assessments in the Rust provider adapter for tools that compare changes or related files.

## v0.2.4

- Use `--endpoint` or `EV_GREP_ENDPOINT` to send requests through a trusted provider proxy.

## v0.2.3

- `--jobs N` controls concurrent requests, from 1 to 256. The default remains four. JSONL begin records include the chosen limit.
- The review guide shows how to retain uncertain candidates across two searches. The code-scanning example now treats interrupted commands as failures.
- Includes the line-range and SARIF support described below.

## v0.2.2

Search part of a file by adding `:N` or `:N-M` to a path, such as `ev-grep 'Writes to the database' src/users.py:40-72`. Lines count from 1 and include both ends. ev-grep still sends the whole file for context, and Jev judges only those lines. Terminal output prints the same `path:N-M` form, and JSON records add `start_line` and `end_line`. [Use with other tools](https://ev-grep.com/docs/pipelines) shows how to get paths and ranges from `rg`, `git diff`, and ast-grep.

`--sarif` writes one SARIF 2.1.0 log to stdout when the search ends. Matches become warnings and uncertain results become notes, so GitHub code scanning and other SARIF tools can show them. `--sarif` cannot be combined with `--json`.

Two changes may affect scripts:

- A path that ends in `:` and a number and does not exist is now read as a line range. An existing file with such a name is still searched whole. A malformed range such as `:0` or `:9-3`, or a range past the end of the file, is an error.
- The stderr summary starts with `N selected` instead of `N files selected`, because a selection can be a range. JSON output is unchanged apart from the new line fields.

Whole-file searches send the same request as 0.1.4. Versions 0.2.0 and 0.2.1 were not published; their changes are in this release.
