# Output

For source searches, terminal output lists matching and uncertain locations. For example:

```text
src/users.py:20-56    92% match
src/actions.py:4-12   uncertain (model chose uncertain)
src/client.py:20-45   uncertain (model chose no_match; confidence 0.5 < 0.8)
```

The percentage is the model's probability for `match`. It is not the probability that a review is correct.
Source nonmatches are omitted. State mode prints every answer, including `no_match`. Counts and errors go to stderr.

Use `--sort score` to put higher match probabilities first. Results wait until the search finishes; errors remain
immediate. Sorting preserves the model's scores and keeps uncertain results. It makes no extra model requests.

`--min-confidence` controls routing (default `0.8`). An answer below that confidence routes to uncertain, even if the
model chose `match` or `no_match`. An explicit uncertain choice stays uncertain at any threshold. Terminal output
shows which case occurred. JSON preserves the original answer and records the routing separately.

## JSON Lines

```sh
ev-grep 'Hides a database failure' src/ --json > results.jsonl
```

Every record has `schema_version: 2`, `type`, and `data`. A result has this shape (illustrative scores; request metadata omitted):

```json
{
  "schema_version": 2,
  "type": "result",
  "data": {
    "file": "src/users.py",
    "range": {
      "start": { "line": 19, "column": 0 },
      "end": { "line": 20, "column": 0 },
      "byteOffset": { "start": 380, "end": 394 }
    },
    "text": "    return []\n",
    "assessment": {
      "choice": "match",
      "confidence": 0.76,
      "probabilities": { "match": 0.92, "no_match": 0.03, "uncertain": 0.05 },
      "model": "typesafe/jev-1.13-20260917",
      "input_tokens": 280,
      "output_tokens": 12
    },
    "routing": { "outcome": "uncertain", "reason": "low_confidence" }
  }
}
```

Positions count from zero; columns count Unicode characters. The end is exclusive. Optional `byteOffset` counts
UTF-8 bytes. `file` is null for stdin. Optional `context` carries the complete original file used to assess a range.
`supporting` contains any supplied source entries, each with `file`, `range`, and `text`; it is omitted when empty.
Optional `metaVariables` retains ast-grep selection metadata; it is not part of the model's candidate search input.
Results contain source text; store them as carefully as the source itself.

`probabilities` answers one question with three alternatives. Low match probability alone cannot distinguish a clear
negative from missing context. `confidence` is the provider's separate confidence measure. The raw `assessment` is
unchanged by `--min-confidence`. Its optional `request` metadata includes the `prompt_id` used for the request.
The ID is a SHA-256 fingerprint of the prompt contents; the executable version is reported separately.

`routing.outcome` controls terminal inclusion, summary counts, and the exit code. An uncertain outcome has one of two
reasons: `model_uncertain` for an explicit uncertain choice, or `low_confidence` for a choice below the threshold.
Other outcomes omit `reason`. These reasons describe routing; the classifier does not return a free-text explanation.

| Type | Data |
| --- | --- |
| `begin` | `query`, executable `version`, `prompt_id`, provider, requested model, confidence threshold, dry-run flag, jobs, sort order |
| `selected` | Dry run: candidate fields, including optional `context`, `supporting`, and `metaVariables`, and combined source `bytes` |
| `result` | Candidate fields, raw `assessment`, and `routing` |
| `selected_state` | State dry run: `state`, content-derived `input_id`, and input `bytes` |
| `assessment` | State answer: `state`, `input_id`, `bytes`, raw `assessment`, and `routing` |
| `skipped` | File, optional requested lines, and binary reason |
| `error` | File or input identifier, optional requested lines, and message |
| `summary` | Selected, evaluated, match, no-match, uncertain, skipped, and error counts; dry-run flag |

Results arrive as requests finish, or in descending match probability with `--sort score`. All successful assessments appear, including nonmatches. Completed runs end with
one summary. Interrupted runs may not. Argument parsing errors print usage and exit 2 before starting the stream.
Sorted runs buffer results, so interruption may leave only configuration, errors, and skips.

## State assessments

With [--state](./cli.md#assess-json-state), the `begin` record adds `mode: "state"`. Its `query` and executable `version`
describe the run, and `prompt_id` identifies the context classifier prompt.

The `selected_state` and `assessment` records retain the parsed JSON value as `state`. Large integers and decimal
precision are preserved without floating-point rounding. `input_id` has the form `sha256:…` and hashes the serialized
parsed value; input whitespace does not affect it. `bytes` counts the original input bytes. These records have no
candidate file or range, and state fields that look like paths are never opened.

An `assessment` record includes the same raw answer, token usage, request metadata, and separate `routing` as a source
result. The label answers the supplied question; it does not establish a broader review conclusion. Completed state
runs end with a summary and use the same exit codes as source searches. A valid state dry run exits 0.

## Select by score

Keep candidates with at least 90% match probability:

```sh
jq -c 'select(.type == "result" and .data.assessment.probabilities.match >= 0.9) | .data' results.jsonl
```

For review, keep everything except confident negative answers:

```sh
jq -c 'select(.type == "result") | .data
  | select(.assessment.choice != "no_match" or .assessment.confidence < 0.9)' results.jsonl
```

These records can feed `--candidates`. Check the original exit status and summary before filtering: jq cannot recover
an error record you discarded. A score cutoff is a caller policy, not a completeness guarantee.

To keep the same candidates as terminal output at the run's threshold, filter on routing instead:

```sh
jq -c 'select(.type == "result" and .data.routing.outcome != "no_match") | .data' results.jsonl
```

## Exit codes

The first matching row wins:

| Code | Meaning |
| --- | --- |
| `2` | An execution or argument error; earlier results may still be available |
| `3` | At least one answer routes to uncertain |
| `0` | At least one match, with no errors or uncertainty |
| `1` | No matches, with no errors or uncertainty; includes empty selections |

Dry runs return 0 for eligible text or a valid state value, 1 for an empty candidate selection, and 2 for errors.
They need no key and make no requests.
