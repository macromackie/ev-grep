# CLI reference

Use a query to describe the files you want to find. Normal searches need a provider API key; see
[providers](#providers). A dry run makes no API requests.

## Examples

Search a directory, keeping only Rust files:

```sh
ev-grep 'Performs database operations' src/ --glob '*.rs'
```

Read a longer query from `query.txt`, or pass it through stdin:

```sh
ev-grep -f query.txt src/
ev-grep -f - src/ < query.txt
```

The whole query file is one question, even if it spans several lines. With `-f`, all positional arguments are paths.

Preview which files would be sent:

```sh
ev-grep 'Performs database operations' src/ --dry-run
```

Use JSON Lines for scripts:

```sh
ev-grep 'Performs database operations' src/ --json > results.jsonl
```

Check the command's [exit status](#exit-status) and final summary before treating the output as complete.
To extract matching paths from a saved result, use:

```sh
jq -r 'select(.type == "result" and .data.assessment.outcome == "match") | .data.path' results.jsonl
```

This filter prints only matches. It does not check for errors or uncertain results.

## Arguments

```text
ev-grep [OPTIONS] <QUERY> [PATHS]...
ev-grep [OPTIONS] --query-file <FILE> [PATHS]...
```

| Option | Meaning |
| --- | --- |
| `-f, --query-file FILE` | Read one query from a file; `-` reads stdin |
| `-g, --glob GLOB` | Filter paths; repeat to add filters, prefix with `!` to exclude |
| `--provider PROVIDER` | `openrouter` (default) or `typesafe` |
| `--model MODEL` | A supported pinned model for the selected provider |
| `--json` | Write versioned JSON Lines to stdout |
| `--dry-run` | List selected text files and sizes without credentials or API requests |
| `-h, --help` | Show usage |
| `-V, --version` | Show the executable version |

Without `-f`, the first positional argument is the query. Empty queries fail. Search paths default to the current
directory. Input files are read from disk; stdin is reserved for `-f -`.

## File selection

Directory searches respect standard ignore files and skip hidden files. An explicit file path bypasses ignore files,
but glob filters still apply. Globs use gitignore syntax; later matches take precedence. They cannot re-include files
inside an ignored directory. Overlapping roots evaluate each canonical path once.

Directory symlinks are not followed. Explicit symlinks and special files are errors. Paths and text must be valid
UTF-8. Files containing NUL bytes are skipped as binary; other invalid UTF-8 inputs are errors.

## Providers

OpenRouter is the default. For direct TypeSafe access:

```sh
export TYPESAFE_API_KEY='your-key'
ev-grep --provider typesafe 'Performs database operations' src/
```

| Provider | Credential | Default and currently supported model |
| --- | --- | --- |
| `openrouter` | `OPENROUTER_API_KEY` | `typesafe/jev-1.13-20260917` |
| `typesafe` | `TYPESAFE_API_KEY` | `jev-1.13.0` |

Set `EV_GREP_PROVIDER` and `EV_GREP_MODEL` to change the defaults for your shell. CLI flags override those environment
variables. API keys never select a provider implicitly. Unknown models fail before requests.

Each request contains the query, path, and complete contents of one file. The prompt treats source as data, not
instructions. Files are assessed independently. A search does not retrieve related code or establish that a change
introduced a bug. Requests may incur provider charges.

The adapter uses OpenRouter's Decisions endpoint or TypeSafe's System One endpoint. It does not retry failed
requests, follow redirects, or fall back to another provider. Custom endpoints and automatic `.env` loading are
not supported.

## Results

| Outcome | Meaning |
| --- | --- |
| `match` | The model judges that the file supports the query |
| `no_match` | The model judges that the file provides enough evidence to reject the query |
| `uncertain` | The model reports insufficient context, or its confidence is below `0.8` |

The confidence threshold is an initial policy, not a measured accuracy guarantee. JSONL retains the raw choice,
confidence, and probabilities. A failed request is an execution error; it never becomes a semantic outcome.

Terminal output prints a path and label for matches and uncertain files, separated by a tab. It omits nonmatches
and sends the count summary and errors to stderr. JSONL includes nonmatches and skipped-file records as well.

## Exit status

When several conditions apply, the first matching row wins:

| Code | Meaning |
| --- | --- |
| `2` | An execution or argument error occurred, possibly after partial results |
| `3` | Evaluation completed with at least one uncertain file |
| `0` | At least one match, with no errors or uncertainty |
| `1` | No matches, with no errors or uncertainty; includes empty selections |

Dry runs return `0` when at least one text file is selected, `1` when none are eligible, and `2` for errors.
They do not evaluate files. An interrupted run may end before the final summary.

## Limits

| Resource | Limit |
| --- | --- |
| Concurrent requests | 4 |
| Connection timeout | 10 seconds |
| Full request timeout | 60 seconds |
| File | 64 KiB |
| Query | 8 KiB |
| Encoded request | 96 KiB |
| Response | 64 KiB |

Exceeding a limit produces an error. Content is never silently truncated.
Binary detection checks the first 8 KiB before enforcing the file-size limit, then checks the rest of an eligible file.
A large binary without an early NUL byte may therefore produce a size error.

## JSON Lines, version 1

Each line is an object with `schema_version: 1`, `type`, and `data`. Records arrive in completion order.
Object key ordering and human-readable messages are not stable interfaces. New fields may be added within version 1.

| Type | Data |
| --- | --- |
| `begin` | `provider`, `requested_model`, `min_confidence`, `dry_run` |
| `selected` | Dry run only: `path`, `bytes` |
| `result` | `path`, `assessment` (below) |
| `skipped` | `path`, `reason: "binary"` |
| `error` | `path` (null for run-wide errors), `message` |
| `summary` | `selected`, `evaluated`, `matches`, `no_match`, `uncertain`, `skipped`, `errors`, `dry_run` |

An assessment contains `outcome`, raw `choice`, `confidence`, `probabilities` keyed by the three outcome names,
actual `model`, `input_tokens`, and `output_tokens`. Uncertain assessments also include `reason`: either
`insufficient_context` or `low_confidence`.

All successful assessments are emitted, including nonmatches. Errors have their own records. A completed run ends
with one summary. Check both that summary and the exit status; partial JSONL does not establish success.
Argument parsing failures print usage to stderr and exit `2` without starting a JSONL stream.
Human diagnostics never enter JSON stdout. Source contents and credentials are not echoed.
