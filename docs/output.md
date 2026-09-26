# Output

Terminal output lists matching and uncertain files as `path`, a tab, then the outcome. A search of part of a file prints `path:N-M` instead, or `path:N` for one line. Counts and errors go to stderr. Nonmatches are omitted.

| Outcome | Meaning |
| --- | --- |
| `match` | The model judges that the file supports the query |
| `no_match` | The model judges that the file provides enough evidence to reject the query |
| `uncertain` | The model reports insufficient context, or its confidence is below `0.8` |

The confidence threshold is an initial policy, not a measured accuracy guarantee. A failed request is an execution error, never a semantic outcome.

## JSON Lines

```sh
ev-grep 'Performs database operations' src/ --json > results.jsonl
```

Each line has `schema_version`, `type`, and `data`. This summary from a dry run selecting one text file is expanded for readability. The actual record occupies one line:

```json
{
  "schema_version": 1,
  "type": "summary",
  "data": {
    "dry_run": true,
    "errors": 0,
    "evaluated": 0,
    "matches": 0,
    "no_match": 0,
    "selected": 1,
    "skipped": 0,
    "uncertain": 0
  }
}
```

Extract matching paths:

```sh
jq -r 'select(.type == "result" and .data.assessment.outcome == "match") | .data.path' results.jsonl
```

That filter does not check for errors or uncertainty. Check the command's exit status and final summary before treating results as complete.

## Exit codes

When several conditions apply, the first matching row wins:

| Code | Meaning |
| --- | --- |
| `2` | An execution or argument error occurred, possibly after partial results |
| `3` | Evaluation completed with at least one uncertain file |
| `0` | At least one match, with no errors or uncertainty |
| `1` | No matches, with no errors or uncertainty; includes empty selections |

Dry runs return `0` when at least one text file is selected, `1` when none are eligible, and `2` for errors. They do not evaluate files.

## Record types

JSONL schema version 1 includes:

| Type | Data |
| --- | --- |
| `begin` | `provider`, `requested_model`, `min_confidence`, `dry_run`, `jobs` |
| `selected` | Dry run only: `path`, `bytes` |
| `result` | `path`, `assessment` |
| `skipped` | `path`, `reason: "binary"` |
| `error` | `path` (null for run-wide errors), `message` |
| `summary` | `selected`, `evaluated`, `matches`, `no_match`, `uncertain`, `skipped`, `errors`, `dry_run` |

Records for part of a file add `start_line` and `end_line`, counted from 1. `path` stays the plain file path, so the range never has to be parsed back out of it. Errors about the argument itself, such as a malformed range or a range on a directory, report the argument as typed in `path`. A range past the end of a file reports the plain path with `start_line` and `end_line`. The summary counts each file and each range as one selection.

An assessment contains `outcome`, raw `choice`, `confidence`, `probabilities` keyed by the three outcome names, actual `model`, `input_tokens`, and `output_tokens`. Uncertain assessments also include `reason`: `insufficient_context` or `low_confidence`.

Records arrive in completion order. JSONL includes every successful assessment, including nonmatches. Completed runs end with one summary; interrupted runs may not. Argument parsing errors print usage to stderr and exit `2` without starting a JSONL stream.

Object key order and human-readable messages are not stable interfaces. Version 1 may gain fields. Source contents, credentials, and human diagnostics are never written to JSON stdout.

## SARIF

```sh
ev-grep 'Catches a database failure and returns an empty result' src/ --sarif > ev-grep.sarif
```

`--sarif` writes one [SARIF 2.1.0](https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html) log to stdout when the search ends, for code scanning tools such as GitHub's. It cannot be combined with `--json`. Counts and errors still go to stderr, and the exit codes above still apply.

| ev-grep | SARIF |
| --- | --- |
| The query | One rule, whose ID, such as `query/9884e336a13bc935`, is a hash of the query text; the same query keeps the same ID |
| `match` | A result with level `warning` |
| `uncertain` | A result with level `note` |
| `no_match` | Left out, as in terminal output |
| An error | A notification with level `error` in `invocations`, which then reports `executionSuccessful: false` |
| A skipped binary file | A notification with level `note` |
| A dry run | No results; the selected files are listed as `artifacts` |

A result's message starts with the first line of the query, followed by the outcome and confidence. Its `properties.assessment` holds the same assessment as JSON output. A line range becomes the result's `startLine` and `endLine`; a whole-file result points at line 1. Relative paths stay relative to `%SRCROOT%`, the directory ev-grep ran in, and absolute paths become `file://` URIs. Run ev-grep from the repository root so code scanning can find the files.

The log is written once, when the search ends, so an interrupted search writes none. Argument errors print usage and write no log.
