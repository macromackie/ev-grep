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

## Read a query from a file

```sh
ev-grep -f query.txt src/
ev-grep -f - src/ < query.txt
```

The whole query file is one query, even if it spans several lines. With `-f`, all positional arguments are paths. Stdin is reserved for the query; source files are read from disk.

## Preview a search

```sh
ev-grep 'Performs database operations' src/ --dry-run
```

A dry run lists selected text files and sizes. It makes no API requests and needs no key.

## Options

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
| `--dry-run` | List files without credentials or API requests |
| `-h, --help` | Show usage |
| `-V, --version` | Show the executable version |

Empty queries fail. Directory symlinks are not followed; explicit symlinks and special files are errors.
Paths and text must be valid UTF-8. Files containing NUL bytes are skipped as binary. Other invalid UTF-8 inputs are errors.

See [Output](./output.md) for exit codes and JSONL, and [Request limits](./limits.md) for size limits.
