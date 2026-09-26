# Use with other tools

ev-grep takes a query and paths. Other tools can choose those paths, including [line ranges](./cli.md#search-part-of-a-file) such as `src/users.py:20-56`, and `xargs` hands them to ev-grep. Add `--dry-run` to the end of a pipeline to see what would be sent before any requests are made.

## Pick files with a text search

```sh
rg -l0 'except' src/ | xargs -0 -r ev-grep 'Hides a database failure'
```

`-l0` and `xargs -0` separate paths with NUL bytes, so names with spaces survive. Keep `-r`: with an empty list, some versions of xargs still run `ev-grep 'query'` once, and ev-grep then searches the current directory.

## Search the files a branch changed

```sh
git diff -z --name-only --diff-filter=d main... | xargs -0 -r ev-grep 'Writes to the database'
```

`main...` compares the current branch with the commit where it left `main`. `--diff-filter=d` leaves out deleted files, which no longer exist. ev-grep assesses the files as they are now, not whether the branch introduced what it finds.

## Search only the changed lines

```sh
git -c core.quotePath=false diff -U0 main... | awk '
  /^\+\+\+ / { file = ""; if (substr($0, 5, 2) == "b/") { file = substr($0, 7); sub(/\t$/, "", file) } }
  /^@@/ && file != "" { split($3, n, ","); start = substr(n[1], 2); count = (n[2] == "" ? 1 : n[2])
                        if (count > 0) print file ":" start "-" (start + count - 1) }' \
  | tr '\n' '\0' | xargs -0 -r ev-grep 'Writes to the database'
```

With `-U0`, each hunk header lists exactly the changed lines. The script turns a header such as `@@ -40,2 +40,5 @@` into `src/users.py:40-44`. Hunks that only delete lines have nothing to send, so they are skipped. Replace `-U0` with `-W` to send each changed function whole.

Run it from the repository root on a clean checkout of the branch, so the line numbers match the files on disk. git quotes file names that contain quotes, backslashes, or control characters; the script skips those files.

## Pick syntax with ast-grep

```sh
ast-grep scan --json=stream src/ --inline-rules '{id: catches, language: python,
  rule: {kind: function_definition, has: {kind: except_clause, stopBy: end}}}' \
  | jq -r '"\(.file):\(.range.start.line + 1)-\(.range.end.line + 1)"' \
  | tr '\n' '\0' | xargs -0 -r ev-grep 'Catches a database failure and returns an empty result'
```

The rule selects each Python function that contains an `except` clause. ev-grep then assesses each function, with the rest of its file as context. ast-grep's JSON counts lines from 0, hence the `+ 1`.

## Narrow a review in two passes

```sh
ev-grep 'Performs database operations or handles their failures' src/ \
  | cut -f1 | tr '\n' '\0' | xargs -0 -r ev-grep 'Returns an empty result instead of the error'
```

Terminal output lists each match or uncertain result as a location, a tab, and the outcome. `cut -f1` keeps the location, which ev-grep accepts again, ranges included. The first query keeps candidates, including uncertainty. The second looks for a specific violation. Inspect its matches and uncertain results before writing a review comment. Neither pass establishes whether a change introduced the behavior.

To pass on matches only, drop uncertain results with JSON output:

```sh
ev-grep 'Performs database operations or handles their failures' src/ --json \
  | jq -r 'select(.type == "result" and .data.assessment.outcome == "match") | .data
           | .path + (if .start_line then ":\(.start_line)-\(.end_line)" else "" end)'
```

## Upload results to GitHub code scanning

```yaml
name: ev-grep
on: pull_request
permissions:
  contents: read
  security-events: write
jobs:
  search:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
      - run: |
          curl -fsSL https://ev-grep.com/install.sh | bash
          echo "$HOME/.local/bin" >> "$GITHUB_PATH"
      - run: |
          status=0
          ev-grep 'Catches a database failure and returns an empty result' src/ --sarif > ev-grep.sarif || status=$?
          case "$status" in 0|1|3) ;; *) exit "$status" ;; esac
        env:
          OPENROUTER_API_KEY: ${{ secrets.OPENROUTER_API_KEY }}
      - uses: github/codeql-action/upload-sarif@v4
        with:
          sarif_file: ev-grep.sarif
          category: ev-grep-database-errors
```

[SARIF output](./output.md#sarif) turns matches into warnings and uncertain results into notes, which appear as code scanning alerts. The step accepts search outcomes (`0`, `1`, or `3`) and fails on execution errors or interruption. Give each query its own `category` so its results don't replace another query's. Store the API key as a repository secret. Code scanning is available for public repositories, and for private ones with GitHub Code Security.

## Pitfalls

- ev-grep reads stdin only with `-f -`, and only as the query. `cat file.py | ev-grep 'query'` does not search the piped text; with no paths, ev-grep searches the current directory.
- xargs may split a long list into several ev-grep runs, each with its own summary, JSON stream, and exit code. xargs exits `123` when any run exits with 1 to 125, which includes ev-grep's "no matches" (`1`) and "uncertain" (`3`). Read ev-grep's [summaries and exit codes](./output.md#exit-codes) rather than xargs's status.
