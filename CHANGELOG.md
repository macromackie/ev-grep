# Changelog

## v0.7.0

- Exit codes keep their meanings: 0 for a match, 1 for no match, 2 for an error, and 3 for uncertainty. A file over
  64 KiB no longer makes a search exit 2, because ev-grep now skips it; add `--strict` to exit 2 for it. A rejected
  credential, a rate limit that outlasts the request budget, and a selection over `--max-candidates` exit 2. The
  [output docs](https://ev-grep.com/docs/output#exit-codes) give the full rules.
- JSON error records gain `kind`, such as `timeout`, `connect`, `tls`, `rate_limited`, `moderation`, or `too_large`.
  The [output docs](https://ev-grep.com/docs/output) list every kind. Messages now include the underlying cause,
  such as `cannot inspect file: No such file or directory`.
- Name the cause of a transport error: `Jev transport failed (timeout)`, `(connect)`, `(tls)`, or `(transport)`,
  followed by the underlying cause, such as `Connection refused`. Text and JSON error records carry the same message.
- Send a request once when the response names a different model; the error names both models. TLS failures are also
  no longer retried. Each failure previously took three attempts.
- A 403 that names moderation reports its reason codes. ev-grep reads the body only for those codes and never prints
  the flagged text or the provider's message.
- Stop the search on HTTP 401, 402, or a 403 that names no moderation reason, with one `credential_rejected` error
  that names the provider and its key variable. No request starts after it: a rejected key over 40 files now sends 1
  request at `--jobs 1` and at most 4 at the default, instead of 40. The error keeps the provider's error code from
  the body, never its message. A 403 that names moderation still fails only its file.
- Fail a queued file at once with a `rate_limited` error when the provider's rate limit would end after its 15-second
  budget, and continue the search. A `Retry-After: 20` reply over 40 files at the default `--jobs 4` now ends in about
  40 ms after 4 requests, instead of 150 seconds and 32 requests that all still failed.
- Skip a file over the 64 KiB file limit instead of failing the search. JSON reports a `skipped` record with reason
  `too_large` and the file's `bytes`, text output names the file on stderr, and the summary counts it under
  `skipped`. Such a file, often a lockfile or a long changelog, was previously an error that made the search exit 2.
  Add `--strict` to exit 2 when any file is skipped as too large. Files are still never truncated.
- Add `--max-candidates N`, which fails with a `too_large` error and sends no request when a search selects more than
  N files, line ranges, or candidate records. The `begin` record gains `strict` and `max_candidates`.
- Send a model that the supported-model table does not list, such as a newer Jev release, with `--model` and
  `--unvalidated-model`. The `begin` record gains `model_validated`, which is `false` for such a model. Without the
  flag, an unlisted model still fails before any request. The
  [provider docs](https://ev-grep.com/docs/providers#models) list the supported models.
- Skip everything inside a directory that a glob excludes. `--glob '!vendor'` previously still selected and sent
  `vendor/lib/v.rs`, because the glob was matched only against file names.
- Report each symbolic link that a directory search finds as a `skipped` record with reason `symlink`, counted in the
  summary. Links are still not followed; they were previously dropped without a record.
- Add `--trace PATH`, or `EV_GREP_TRACE`, to write a JSON Lines trace of the search; `-` writes it to stderr. Its
  first record names the trace schema version, ev-grep's version, the Git revision it was built from, and the
  environment, which `--trace-environment` sets. Spans cover the search, each file, and each provider request, wait,
  and attempt, with sizes, outcomes, error kinds, and durations. A trace never holds the query, source text, API key,
  or provider messages. Tracing leaves results, exit codes, and requests unchanged, and a search never selects its own
  trace file.
- Rust API: `ev-grep-core` and `ev-grep-jev` return their own error enums instead of `anyhow::Error`. `Evaluator`
  declares an `Error` type that implements `Failure`, whose `kind` returns an `ErrorKind`, and converts from
  `ev_grep_core::Error`. `FileError` carries that kind. `SourceTooLarge` is now `ev_grep_core::Error::SourceTooLarge`.
  `scan_inputs` returns the first error whose kind is fatal, `ErrorKind::CredentialRejected`, and stops; its emit
  error type must also convert from the evaluator's error.
- Rust API: `ev-grep-jev` exposes `RequestError` with its `FailureKind`, HTTP status, `Retry-After` delay, provider
  error code, moderation reasons, and provider request identifiers. `FailureKind::retryable` is the transport's retry
  table. Request failures, including a timed-out 15-second budget, reach callers as `ev_grep_jev::Error::Request`.
- Rust API: `ScanEvent::Skipped` gains `reason`, a `SkipReason`, and `Discovery` gains `symlinks`, the links that
  directory searches did not follow. `SkipReason::TooLarge` carries the file's `bytes`; `scan_inputs` reports a file
  over `MAX_FILE_BYTES` with it instead of as a `FileError`. `Loaded::Binary` is now
  `Loaded::Skipped { target, reason }`.
- Rust API: `Provider::supported_models` is the supported-model table, default first. `Jev::with_unvalidated_model`
  connects to a model outside it.
- Rust API: `ev-grep-jev` records `tracing` spans: `provider.request` for each request, `provider.attempt` for each
  attempt, and `provider.wait` while a request waits for a concurrency slot, a shared rate limit, or a retry delay.
  Its `provider.attempt_finished` and `provider.request_finished` events carry the failure kind, HTTP status,
  `Retry-After` delay, request identifiers, attempt counts, and durations. They never carry the query, source, prompt,
  API key, or provider message. Jev installs no subscriber, so nothing is recorded unless the application installs
  one.
- Rust API: `scan_inputs` runs each input in an `ev_grep.file` `tracing` span with its path, size, outcome, and skip
  reason or error kind.
- Rust API migration: give each `Evaluator` implementation an `Error` type that implements `Failure` and
  `From<ev_grep_core::Error>`, and return it from `assess` and `assess_questions`; `ev_grep_jev::Jev` uses
  `ev_grep_jev::Error`. The `emit` closure passed to `scan_inputs` returns an error type that converts from
  `ev_grep_core::Error` and from the evaluator's error; `anyhow::Error` converts from both. `scan_inputs` now returns
  a rejected credential as its error instead of reporting a failed file for each input. Match `SkipReason` in
  `ScanEvent::Skipped` and `Loaded::Skipped`, and branch on `Failure::kind` or `FileError::kind` instead of message
  text.

## v0.6.3

- Trust the operating system's certificate store as well as the bundled Mozilla roots. Requests through a
  TLS-inspecting proxy no longer fail with `Jev transport failed: error sending request` when the proxy's certificate
  authority is installed. `SSL_CERT_FILE` and `SSL_CERT_DIR` replace the system store when set.

## v0.6.2

- Expose oversized source files as a typed Rust error, including their byte count. Callers can distinguish a source
  size limit from an unreadable file without parsing error text. File limits, scan outcomes, and exit codes are unchanged.

## v0.6.1

- Reject JSON objects containing reserved parser keys instead of silently changing their values. This protects state
  and candidate inputs, including nested objects, and the generic Rust assessment boundary. Ordinary JSON numbers
  retain their precision.

## v0.6.0

- Assess a supplied JSON value with `--state FILE` or `--state -`. State input is bounded to 64 KiB, preserves numeric
  precision, and never opens paths inside the value. Use `--dry-run --json` to inspect it without a model request.
- State JSON output retains the input, its content-derived identity, the raw model answer, and separate confidence
  routing. Terminal output includes negative answers. Source search behavior and JSONL schema version 2 are unchanged.
- Preserve bounded ast-grep `metaVariables` in candidate output as selection metadata without sending it to the model.
- Rust API: `Candidate` adds optional `meta_variables`; use `None` in existing struct literals. The provider adapter
  exposes `context_prompt_id()` for callers recording the generic question prompt's identity.

## v0.5.0

- Supply helper implementations with repeatable `--context PATH[:N-M]` or candidate JSON `supporting`. Supporting code
  explains the selected candidate; it does not expand the search target. Complete inputs share the existing bounds.
- Keep raw model assessments separate from confidence routing in JSON. Text uncertainty distinguishes a model's
  uncertain choice from a decisive choice below the configured threshold. The default remains 0.8.
- Record the query, executable version, and content-derived prompt identity for reproducible assessments.
- Rust API: assess `Candidate` directly and use `Assessment::route` for caller policy. Provider methods return raw
  answers consistently; `Jev::with_min_confidence` and stored routed assessment fields are removed.

## v0.4.4

- Remove `--sarif`. Use text output for terminals and `--json` for integrations.
- Retry a provider response that fails validation, such as probabilities that do not sum to one, within the existing
  three-attempt budget instead of failing the search.
- Rust API: remove `scan` and `scan_with_jobs`; call `scan_inputs` with file inputs instead.

## v0.4.3

- Recover transient provider failures within a bounded request budget. A slow Jev request may use one spare worker to race a duplicate; the first valid answer wins.
- Include request attempts, hedges, elapsed time, and available usage in JSON assessments.
- Support multiple named questions over shared context in the Rust adapter.

## v0.4.2

- Add `--sort score` to order results by match probability. Streaming remains the default; sorting makes no extra model requests.
- Refine Jev instructions to make decisive judgments from visible code while retaining uncertainty for missing definitions. Queries about declarations can match declarations.

## v0.4.1

- Retry temporary HTTP failures up to three attempts within the request timeout.
- Include upstream request IDs in HTTP errors when available, to help diagnose failed requests.

## v0.4.0

- Search raw text with `--stdin`, or pass ast-grep JSON Lines with `--candidates FILE` (`-` reads stdin).
- Keep exact candidate text and source ranges in JSONL v2. Optional full-file context is supplied explicitly;
  candidate paths are never opened. JSON includes all three model probabilities and raw confidence.
- Terminal matches show their match probability. SARIF preserves candidate columns, including Unicode positions.
- Clarify the search prompt's distinction between missing helper behavior and evidence of absence.

## v0.3.0

- Set `--min-confidence` to control which search results remain uncertain. JSON retains the original answer and score.
- Support structured context assessments in the Rust provider adapter for tools that compare changes or related files.

## v0.2.4

- Use `--endpoint` or `EV_GREP_ENDPOINT` to send requests through a trusted provider proxy.

## v0.2.3

- `--jobs N` controls concurrent requests, from 1 to 256. The default remains four. JSONL begin records include the chosen limit.
- The review guide shows how to retain uncertain candidates across two searches. The code-scanning example now treats interrupted commands as failures.
- Includes the line-range and SARIF support described below.

## v0.2.2

Search part of a file by adding `:N` or `:N-M` to a path, such as `ev-grep 'Writes to the database' src/users.py:40-72`. Lines count from 1 and include both ends. ev-grep still sends the whole file for context, and Jev judges only those lines. Terminal output prints the same `path:N-M` form, and JSON records add `start_line` and `end_line`. [Use with other tools](https://ev-grep.com/docs/pipelines) shows how to get paths and ranges from `rg`, `git diff`, and ast-grep.

`--sarif` writes one SARIF 2.1.0 log to stdout when the search ends. Matches become warnings and uncertain results become notes, so GitHub code scanning and other SARIF tools can show them. `--sarif` cannot be combined with `--json`.

Two changes may affect scripts:

- A path that ends in `:` and a number and does not exist is now read as a line range. An existing file with such a name is still searched whole. A malformed range such as `:0` or `:9-3`, or a range past the end of the file, is an error.
- The stderr summary starts with `N selected` instead of `N files selected`, because a selection can be a range. JSON output is unchanged apart from the new line fields.

Whole-file searches send the same request as 0.1.4. Versions 0.2.0 and 0.2.1 were not published; their changes are in this release.
