# CLI

```text
ev-grep [OPTIONS] <QUERY> [PATHS]...
ev-grep [OPTIONS] --query-file <FILE> [PATHS]...
```

| Option | Meaning |
| --- | --- |
| `-f, --query-file FILE` | One complete multiline query; `-` reads stdin |
| `-g, --glob GLOB` | Repeatable gitignore-style filters; `!` excludes, later matches win |
| `--provider PROVIDER` | `openrouter` (default) or `typesafe` |
| `--model MODEL` | Supported pinned model for that provider |
| `--json` | Versioned JSON Lines on stdout |
| `--dry-run` | Inspect selected text files and sizes with no API requests or credentials |
| `-h, --help` / `-V, --version` | Usage / executable version |

Exactly one query source is used. With `--query-file`, every positional argument is a path, never another query.
Empty queries fail. An omitted search path means `.`. Input files are read from disk; stdin is reserved for `-f -`.

Directory traversal respects standard ignore files and skips hidden files. Explicit files bypass ignore files;
globs still narrow all candidates. Globs do not re-include ignored directory contents. Overlapping roots evaluate
each canonical path once. Directory symlinks are not followed; explicit symbolic links and special files are errors.
Paths must be valid UTF-8. NUL-containing files are skipped as binary; other invalid UTF-8 inputs are errors.

## Provider configuration

Precedence is CLI flag, then environment variable, then default. Environment values cannot override an explicit flag.

| Provider | Credential | Default and currently supported model |
| --- | --- | --- |
| `openrouter` | `OPENROUTER_API_KEY` | `typesafe/jev-1.13-20260917` |
| `typesafe` | `TYPESAFE_API_KEY` | `jev-1.13.0` |

`EV_GREP_PROVIDER` and `EV_GREP_MODEL` configure the two options. Keys never select a provider implicitly.
Unknown models fail before requests. The adapter uses OpenRouter's Decisions endpoint or TypeSafe's System One
endpoint. There is no fallback, retry, redirect following, arbitrary endpoint setting, or dotenv autoloading.

Each request contains the query, path, and complete source of one file. Source is treated as data, not instructions.
At most four requests are in flight. Connect timeout is 10 seconds; the full request timeout is 60 seconds.
Files are limited to 64 KiB, queries to 8 KiB, encoded requests to 96 KiB, and responses to 64 KiB.
Limits produce errors, never truncated assessments. Binary detection inspects up to the first 8 KiB before the size
check, then the rest of an eligible file. A large binary without an early NUL may therefore report a size error.

## Outcomes and exit status

`match` means the supplied source supports the query. `no_match` means it provides enough evidence to reject it.
`uncertain` means the model chose insufficient context, or confidence was below 0.8. This threshold is a conservative
initial policy, not a calibrated accuracy claim. Raw model choice, confidence, and probabilities remain available.
A failed request is an error; it never becomes uncertainty or a no-match.

| Exit code | Meaning, in precedence order |
| --- | --- |
| `2` | An execution/argument error occurred, possibly after partial results |
| `3` | Evaluation completed with at least one uncertain file |
| `0` | At least one match, with no errors or uncertainty |
| `1` | No matches, with no errors or uncertainty; includes empty selections |

Dry runs use 0 when at least one text file is selected, 1 when none are eligible, and 2 for errors.
They never claim that any file has been evaluated. Interruption may end output before its final summary.

## JSON Lines, version 1

Every record is one line with `schema_version: 1`, `type`, and `data`. Records arrive in completion order;
object key ordering and human-readable messages are not interfaces. New fields can be added within version 1.

| Type | Data |
| --- | --- |
| `begin` | `provider`, `requested_model`, `min_confidence`, `dry_run` |
| `selected` | Dry run only: `path`, `bytes` |
| `result` | `path`, `assessment` (below) |
| `skipped` | `path`, `reason: "binary"` |
| `error` | `path` (null for run-wide errors), `message` |
| `summary` | `selected`, `evaluated`, `matches`, `no_match`, `uncertain`, `skipped`, `errors`, `dry_run` |

An assessment contains `outcome`, raw `choice`, `confidence`, `probabilities` keyed by the three outcome names,
actual `model`, `input_tokens`, and `output_tokens`. Uncertain assessments also include `reason`, either
`insufficient_context` or `low_confidence`.

All successful assessments are emitted, including nonmatches. Execution errors have their own records. A completed
run ends with one summary. Consumers must check both the final summary and exit status; partial JSONL is not success.
Argument parsing failures before execution print usage to stderr and exit 2 without a JSONL stream.
Human progress/diagnostics never enter JSON stdout. Source contents and credentials are not echoed.

```sh
ev-grep 'Performs database operations' src/ --json > results.jsonl
# Inspect matching files. Preserve ev-grep's status separately when using shell pipelines.
jq -r 'select(.type == "result" and .data.assessment.outcome == "match") | .data.path' results.jsonl
```
