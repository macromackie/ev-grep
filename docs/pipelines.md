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
  | ev-grep 'Catches a database failure and returns an empty result' --candidates - --json
```

The rule selects Python functions containing an `except` clause. Each candidate keeps its exact text and source range.
Only candidate text is sent unless you explicitly supply `context`. Use `set -o pipefail` in a script to detect upstream
failures too. An empty candidate stream never falls back to searching the current directory.

## Narrow a review in two passes

```sh
ev-grep 'Performs database operations or handles their failures' src/ \
  | cut -f1 | tr '\n' '\0' | xargs -0 -r ev-grep 'Returns an empty result instead of the error'
```

Terminal output lists each match or uncertain result as a location, a tab, and the outcome. `cut -f1` keeps the location, which ev-grep accepts again, ranges included. The first query keeps candidates, including uncertainty. The second looks for a specific violation. Inspect its matches and uncertain results before writing a review comment. Neither pass establishes whether a change introduced the behavior.

Use [JSON score filters](./output.md#select-by-score) to choose a different threshold without another model request.
To run a second question, write those candidate records to a file and pass `--candidates candidates.jsonl`.
Keep the first run's exit status and summary alongside the filtered records.

## Pitfalls

- Choose stdin explicitly: `--stdin` for text, `--candidates -` for JSONL, or `-f -` for the query.
- xargs may split a long list into several ev-grep runs, each with its own summary, JSON stream, and exit code. xargs exits `123` when any run exits with 1 to 125, which includes ev-grep's "no matches" (`1`) and "uncertain" (`3`). Read ev-grep's [summaries and exit codes](./output.md#exit-codes) rather than xargs's status.

## Agent skill

Download the [ev-grep skill](https://ev-grep.com/skill.md) into your agent's project skill directory. For agents that read `.agents/skills`:

```sh
mkdir -p .agents/skills/ev-grep
curl -fsSL https://ev-grep.com/skill.md -o .agents/skills/ev-grep/SKILL.md
```

Inspect the downloaded instructions before adopting them. The skill covers input selection, interpreting results,
and investigating findings. It also describes a focused curation loop for authorized code and contract changes.
For optional code, architecture, and contract curation skills, see [Tenet guidance](https://tenet-contracts.com/docs/guidance).
An agent can adopt selected guidance and review later updates while preserving project conventions.
