# CLI

```sh
ev-grep 'Performs database operations' src/
```

The first argument is one query. In path mode, remaining arguments are paths; without paths, ev-grep searches the
current directory. Use `--stdin`, `--candidates`, or `--state` to select another input mode explicitly.

## Select files

```sh
ev-grep 'Performs database operations' src/ --glob '*.rs'
ev-grep 'Performs database operations' src/ --glob '!**/generated/**'
```

Directory searches respect ignore files and skip hidden files. Explicit file paths bypass ignore files, but glob filters still apply. Globs use gitignore syntax; later matches take precedence. A glob that excludes a directory, such as `--glob '!vendor'`, skips everything inside it. Globs cannot re-include files inside a directory that a glob or an ignore file excludes. Overlapping paths are evaluated once.

## Skip large files

```sh
ev-grep 'Performs database operations' . --strict
```

ev-grep reads at most 64 KiB (65,536 bytes) from each file and never truncates one. It skips a larger file, such as a
lockfile or a long changelog, and sends no request for it. JSON reports it as a `skipped` record with reason
`too_large` and the file's `bytes`; text output names it on stderr. The summary counts it under `skipped`.

A skipped file does not change the exit code. Add `--strict` to exit 2 when any file is skipped as too large, so a
script notices source that was not searched. Use `--glob` to leave such files out. A
[line range](#search-part-of-a-file) does not help, because it still needs its whole file within 64 KiB.

## Limit the selection

```sh
ev-grep 'Performs database operations' src/ --max-candidates 200
```

Each selected file, line range, or candidate record gets its own assessment, which may incur charges.
`--max-candidates N` counts the selection before the first request. When more than N are selected, ev-grep reports a
`too_large` error, sends no request, and exits 2. A selection of N or fewer runs unchanged. Skipped files count toward
N; symbolic links and path errors do not. With `--candidates`, ev-grep reads up to N + 1 records before it starts.
Combine it with `--dry-run` to check a selection without a key.

## Search part of a file

```sh
ev-grep 'Catches a database failure and returns an empty result' src/users.py:20-56
ev-grep 'Writes to the database' src/users.py:42
```

Add `:N` or `:N-M` to a file path to assess only those lines. Lines count from 1, and the range includes both ends. Jev still receives the whole file as context, such as imports and helpers defined elsewhere in it, and judges what the selected lines do.

Ranges apply to regular files. A range that starts at 0, ends before it starts, or goes past the last line is an error; ev-grep does not shorten it. Each file and range is assessed once, and different ranges of one file are assessed separately. If a file's real name ends in something like `:12`, that file is searched whole.

Results print the range after the path, as in `src/users.py:20-56`. That is the same form ev-grep accepts, so results can feed another command.

## Supply supporting sources

```sh
ev-grep 'Retries failed requests' src/client.ts:20-45 \
  --context src/transport.ts --context src/retry.ts:10-38
```

Repeat `--context PATH[:N-M]` to attach supporting files or line ranges to every candidate. Use it when the selected
code calls a helper or dependency defined elsewhere. The model judges the selected code; unrelated behavior in a
supporting source cannot make it match. ev-grep does not follow imports or find dependencies automatically.

The option works with paths, `--stdin`, and `--candidates`. Each context path must name a regular text file, and ranges
use the same rules as candidate paths. The candidate's original source and all supporting text share a 64 KiB limit.
Even a supporting range requires its full source file to fit within 64 KiB. See [Request limits](./limits.md).

## Read a query from a file

```sh
ev-grep -f query.txt src/
ev-grep -f - src/ < query.txt
```

The whole query file is one query, even if it spans several lines. With `-f`, all positional arguments are paths. Do not combine `-f -` with another stdin input.

## Search text from stdin

```sh
cat src/users.py | ev-grep 'Hides a database failure' --stdin
```

The complete stream is one candidate. Use a query argument or a query file on disk. Stdin inputs cannot be combined
with paths or globs. Empty text is still one candidate; empty candidate streams select nothing.

## Assess JSON state

```sh
ev-grep 'Is the job marked failed?' --state state.json
ev-grep --query-file question.txt --state state.json
printf '%s\n' '{"job":{"status":"failed","attempts":3}}' \
  | ev-grep 'Is the job marked failed?' --state - --json
```

`--state FILE` asks one question about a supplied JSON value; `-` reads it from stdin. The input must contain exactly
one value and fit within 64 KiB. Objects, arrays, strings, numbers, booleans, and `null` are all valid.
Large integers and decimal precision are preserved without floating-point rounding. Large exponents are also accepted.

Disk inputs must be regular files. Symbolic links, directories, and FIFOs are rejected; use `--state -` for a pipe.

This mode sends the query and state to the provider. It does not select source candidates or open paths mentioned
inside the state. It uses the same context classifier as the Rust `Evaluator::assess_context` API: `match` is an
affirmative answer, `no_match` is a negative answer, and `uncertain` retains uncertainty. The answer concerns only
the supplied question and state.

State input cannot be combined with paths, `--stdin`, `--candidates`, `--context`, `--strict`, `--max-candidates`, or
an explicit `--sort` option.
The query and state cannot both read stdin. Add `--dry-run --json` to inspect the parsed state and its content-derived
`input_id` without credentials or requests.

Text output prints the answer, including `no_match`. JSON emits `selected_state` for a dry run or `assessment` for a
model answer; both retain the state and its identifier. See [Output](./output.md#state-assessments) for record fields.
Exit codes remain 0 for match, 1 for no-match, 2 for input or execution errors, and 3 for uncertainty.

## Search candidates

```sh
ast-grep run --kind function_declaration --lang ts --json=stream src/ \
  | ev-grep 'Retries a failed request' --candidates - --json
```

Each JSON line is one candidate. `--candidates candidates.jsonl` reads from a file instead. Records use ast-grep's
`file`, `range`, and `text` fields. Optional `metaVariables` is retained in JSON output to trace the syntax selection.
Its serialized value is limited to 64 KiB and is excluded from the model's candidate search input. Other extra
metadata is ignored. A JSON array is not accepted; use `--json=stream`.

ev-grep never opens paths from candidate records. To include file context, add
`context` containing the complete original file; its selected slice must equal `text`. Positions count lines and
Unicode characters from zero, with an exclusive end. Optional byte offsets count UTF-8 bytes.

Add `supporting` to supply other source text directly. Each entry has a `file` label, `range`, and `text`; its range
uses the same coordinates as the candidate. For example:

```json
{
  "file": "src/client.py",
  "range": { "start": { "line": 19, "column": 0 }, "end": { "line": 20, "column": 0 } },
  "text": "    return fetch()\n",
  "supporting": [
    {
      "file": "src/db.py",
      "range": { "start": { "line": 9, "column": 0 }, "end": { "line": 10, "column": 0 } },
      "text": "def fetch(): return []\n"
    }
  ]
}
```

`supporting` defaults to an empty list and is omitted from output when empty. Its file labels are never opened.
`--context` adds the same supporting sources to each input record, alongside any entries already in that record.

Each record is assessed independently, including repeated ranges in the same file. Malformed records produce errors;
valid records still run. An oversized record or an unreadable stream stops input, retaining earlier results.
`--stdin`, `--candidates`, `--state`, and path searches are separate input modes.

## JSON input limits

State values and candidate JSON records allow at most 128 nesting levels. They reject the object keys
`$serde_json::private::Number` and `$serde_json::private::RawValue` at every depth, including JSON-escaped spellings.
String values containing those names remain valid. These checks apply in addition to the existing byte limits.

## Preview a search

```sh
ev-grep 'Performs database operations' src/ --dry-run
```

A dry run lists selected text files and line ranges with their combined source sizes. Add `--json` to inspect the
exact candidate and supporting text. It makes no API requests and needs no key. It reports files over 64 KiB as
skipped, and `--strict` and `--max-candidates` apply as they do to a search.

## Sort results

```sh
ev-grep 'Retries failed requests with a delay' src/ --sort score
```

`--sort score` waits for the search to finish and lists the highest match probabilities first. Ties sort by path,
then numeric source position. It works with text and JSONL. Scores, uncertainty, and exit codes stay the same.
The default, `--sort none`, emits results as requests finish.

Sorting buffers results in memory and makes no extra model requests. JSONL errors and skips are still reported as
they occur. Dry runs have no scores and keep their selection order.

## Limit concurrency

```sh
ev-grep 'Catches a database failure and returns an empty result' src/ --jobs 2
```

Up to four requests run at once by default. `--jobs` accepts 1 to 256. Lower it when sharing a provider account with other tools. More jobs do not reduce the number of requests.

## Trace a search

```sh
ev-grep 'Retries failed requests with a delay' src/ --trace trace.jsonl
EV_GREP_TRACE=- ev-grep 'Retries failed requests with a delay' src/
```

`--trace PATH` writes a JSON Lines trace of the search to PATH, replacing the file; `-` writes it to stderr.
`EV_GREP_TRACE` sets the same path. The trace times each file, request, wait, and attempt, and records sizes, HTTP
statuses, error kinds, retry delays, and provider request identifiers. It never records the query, source text, the
API key, request headers, or provider messages.

Tracing does not change results, exit codes, or requests. The search never selects its own trace file. A trace that
cannot be created is an error before any request; a failed write later does not stop the search.
See [Output](./output.md#traces) for the records.

## Options

```text
ev-grep [OPTIONS] <QUERY> [PATHS]...
ev-grep [OPTIONS] --query-file <FILE> [PATHS]...
```

| Option                    | Meaning                                                                              |
| ------------------------- | ------------------------------------------------------------------------------------ |
| `-f, --query-file FILE`   | Read one query from a file; `-` reads stdin                                          |
| `--stdin`                 | Assess stdin as one text candidate                                                   |
| `--candidates FILE`       | Read candidate JSON Lines; `-` reads stdin                                           |
| `--state FILE`            | Assess one JSON value; `-` reads stdin                                               |
| `--context PATH[:N-M]`    | Attach a supporting file or line range to every candidate; repeat to add sources     |
| `-g, --glob GLOB`         | Filter paths; repeat to add filters, prefix with `!` to exclude                      |
| `--provider PROVIDER`     | `openrouter` (default) or `typesafe`                                                 |
| `--model MODEL`           | A supported pinned model for the selected provider                                   |
| `--unvalidated-model`     | Send `--model` even when the [supported-model table](./providers.md#models) omits it |
| `-j, --jobs N`            | Maximum concurrent requests (default 4)                                              |
| `--min-confidence N`      | Route answers below N to uncertain; 0 to 1 (default 0.8)                             |
| `--sort ORDER`            | `none` streams results (default); `score` sorts by descending match probability      |
| `--json`                  | Write versioned JSON Lines to stdout                                                 |
| `--dry-run`               | Inspect selected input without credentials or API requests                           |
| `--strict`                | Exit 2 when a file is skipped for exceeding 64 KiB                                   |
| `--max-candidates N`      | Fail before any request when more than N inputs are selected                         |
| `--trace PATH`            | Write a JSON Lines trace to PATH; `-` writes stderr                                  |
| `--trace-environment ENV` | `local`, `ci`, or `release` in the trace's first record                              |
| `-h, --help`              | Show usage                                                                           |
| `-V, --version`           | Show the executable version                                                          |

Empty queries fail. Directory searches do not follow symbolic links; they report each one as a `symlink` skip. Explicit symlinks and special files are errors.
Paths and text must be valid UTF-8. Files containing NUL bytes are skipped as binary, and files over 64 KiB as
`too_large`. Other invalid UTF-8 inputs are errors.

See [Output](./output.md) for exit codes and JSONL, and [Request limits](./limits.md) for size limits.
