# CLI

```sh
ev-grep 'Performs database operations' src/
```

The first argument is one query. Remaining arguments are paths; without paths, ev-grep searches the current directory.

## Select files

```sh
ev-grep 'Performs database operations' src/ --glob '*.rs'
ev-grep 'Performs database operations' src/ --glob '!**/generated/**'
```

Directory searches respect ignore files and skip hidden files. Explicit file paths bypass ignore files, but glob filters still apply. Globs use gitignore syntax; later matches take precedence. They cannot re-include files inside an ignored directory. Overlapping paths are evaluated once.

## Search part of a file

```sh
ev-grep 'Catches a database failure and returns an empty result' src/users.py:20-56
ev-grep 'Writes to the database' src/users.py:42
```

Add `:N` or `:N-M` to a file path to assess only those lines. Lines count from 1, and the range includes both ends. Jev still receives the whole file as context, such as imports and helpers defined elsewhere in it, and judges what the selected lines do.

Ranges apply to regular files. A range that starts at 0, ends before it starts, or goes past the last line is an error; ev-grep does not shorten it. Each file and range is assessed once, and different ranges of one file are assessed separately. If a file's real name ends in something like `:12`, that file is searched whole.

Results print the range after the path, as in `src/users.py:20-56`. That is the same form ev-grep accepts, so results can feed another command.

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

## Search candidates

```sh
ast-grep run --kind function_declaration --lang ts --json=stream src/ \
  | ev-grep 'Retries a failed request' --candidates - --json
```

Each JSON line is one candidate. `--candidates candidates.jsonl` reads from a file instead. Records use ast-grep's
`file`, `range`, and `text` fields. Extra metadata is ignored. A JSON array is not accepted; use `--json=stream`.

Only the supplied text is assessed. ev-grep never opens paths from candidate records. To include file context, add
`context` containing the complete original file; its selected slice must equal `text`. Positions count lines and
Unicode characters from zero, with an exclusive end. Optional byte offsets count UTF-8 bytes.

Each record is assessed independently, including repeated ranges in the same file. Malformed records produce errors;
valid records still run. An oversized record or an unreadable stream stops input, retaining earlier results.
`--stdin`, `--candidates`, and path searches are separate input modes.

## Preview a search

```sh
ev-grep 'Performs database operations' src/ --dry-run
```

A dry run lists selected text files and line ranges with the size of each file. It makes no API requests and needs no key.

## Sort results

```sh
ev-grep 'Retries failed requests with a delay' src/ --sort score
```

`--sort score` waits for the search to finish and lists the highest match probabilities first. Ties sort by path,
then numeric source position. It works with text, JSONL, and SARIF. Scores, uncertainty, and exit codes stay the same.
The default, `--sort none`, emits results as requests finish.

Sorting buffers results in memory and makes no extra model requests. JSONL errors and skips are still reported as
they occur. Dry runs have no scores and keep their selection order.

## Limit concurrency

```sh
ev-grep 'Catches a database failure and returns an empty result' src/ --jobs 2
```

Up to four requests run at once by default. `--jobs` accepts 1 to 256. Lower it when sharing a provider account with other tools. More jobs do not reduce the number of requests.

## Options

```text
ev-grep [OPTIONS] <QUERY> [PATHS]...
ev-grep [OPTIONS] --query-file <FILE> [PATHS]...
```

| Option | Meaning |
| --- | --- |
| `-f, --query-file FILE` | Read one query from a file; `-` reads stdin |
| `--stdin` | Assess stdin as one text candidate |
| `--candidates FILE` | Read candidate JSON Lines; `-` reads stdin |
| `-g, --glob GLOB` | Filter paths; repeat to add filters, prefix with `!` to exclude |
| `--provider PROVIDER` | `openrouter` (default) or `typesafe` |
| `--model MODEL` | A supported pinned model for the selected provider |
| `-j, --jobs N` | Maximum concurrent requests (default 4) |
| `--min-confidence N` | Route answers below N to uncertain; 0 to 1 (default 0.8) |
| `--sort ORDER` | `none` streams results (default); `score` sorts by descending match probability |
| `--json` | Write versioned JSON Lines to stdout |
| `--sarif` | Write one SARIF 2.1.0 log to stdout for code scanning tools; cannot be combined with `--json` |
| `--dry-run` | List files without credentials or API requests |
| `-h, --help` | Show usage |
| `-V, --version` | Show the executable version |

Empty queries fail. Directory symlinks are not followed; explicit symlinks and special files are errors.
Paths and text must be valid UTF-8. Files containing NUL bytes are skipped as binary. Other invalid UTF-8 inputs are errors.

See [Output](./output.md) for exit codes, JSONL, and SARIF, and [Request limits](./limits.md) for size limits.
