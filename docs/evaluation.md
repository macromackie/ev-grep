# Evaluation

Offline software tests establish file-selection, result, and protocol behavior. They do not measure Jev's accuracy.
Run them with `mise run test`; run the complete offline checks with `mise run check`.

The small [live fixture suite](../crates/ev-grep-jev/tests/fixtures/evals.json) evaluates both applicability and
violation questions. Each case contains a query, synthetic source, source path, and expected outcome.
The harness sends only the query/path/source; the expected outcome and case name stay outside the request.
It uses the production provider adapter, makes fresh paid requests, and compares the normalized result.

```sh
EV_GREP_PROVIDER=typesafe cargo test -p ev-grep-jev --locked --test live -- --ignored --nocapture
EV_GREP_PROVIDER=openrouter cargo test -p ev-grep-jev --locked --test live -- --ignored --nocapture
```

Configure the corresponding standard API key first. `EV_GREP_MODEL` optionally selects a supported pin.
No live eval runs in default tests or CI. The harness prints expected/actual outcomes, confidence, actual model,
mismatch count, and elapsed time. API failures fail the run instead of becoming fixture mismatches.

These fixtures are a smoke baseline, not evidence that the threshold is calibrated or that a contract is proven.
Before routing automatic approvals, add representative clean and faulty cases and inspect false negatives, false
positives, and uncertainty separately. In a two-pass workflow, test that applicability does not discard violations.
