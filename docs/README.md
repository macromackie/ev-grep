# ev-grep

ev-grep searches files with a natural-language query. It sends each selected file to Jev and reports `match`, `no_match`, or `uncertain`.

[Install ev-grep](./installation.md) and set your API key, then search a directory:

```sh
ev-grep 'Catches a database failure and returns an empty result' src/
```

Example stdout:

```text
src/users.py     match
src/actions.py   uncertain
```

Matching and uncertain files appear in the terminal. Use `--json` to include every assessment, including nonmatches.

## How it works

```text
query + paths
      │
      ▼
 select files ──▶ assess each file with Jev ──▶ results
                 query + full file             match
                                               no_match
                                               uncertain
```

Each file is assessed independently. ev-grep does not read related code or compare revisions. Model results can be wrong; use them to decide where to look.

Queries, paths, and selected file contents go to your chosen provider. Requests use your API key and may incur charges. Use `--dry-run` to inspect the selection without sending requests.

## Documentation

- [Installation](./installation.md): install a binary and set your API key.
- [Usage](./cli.md): queries, paths, and glob filters.
- [Providers](./providers.md): OpenRouter and TypeSafe configuration.
- [Output](./output.md): results, exit codes, and JSON Lines.
- [Limits](./limits.md): file sizes, concurrency, and timeouts.
