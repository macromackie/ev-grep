---
title: Execution outcomes
description: Keep semantic uncertainty distinct from failed execution.
---

# Execution outcomes

## Rules

- Return match, no_match, or uncertain for each successful assessment. Preserve the raw model choice and probabilities.
- Route low-confidence choices to uncertainty; never convert missing evidence or failed requests into no_match.
- Give execution errors precedence over uncertainty in exit status. Do not silently truncate source or queries.
- Preserve nonmatches in JSONL. Finish a completed run with a summary and report skipped binary files.

## Checks

Run `cargo test --workspace --locked`. Review `docs/cli.md` alongside changes to result or exit semantics.
