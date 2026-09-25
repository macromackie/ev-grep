---
title: Crate boundaries
description: Keep the search engine independent of providers and the terminal.
---

# Crate boundaries

## Rules

- Keep selection, source loading, and normalized assessment types in ev-grep-core.
- Keep model IDs, credentials in HTTP headers, wire validation, and provider endpoints in ev-grep-jev.
- Keep argument parsing, environment lookup, and human/JSONL rendering in ev-grep.
- Use focused snake_case modules. Use a crate README for ownership; add another crate only for a current independent responsibility.
- Keep private context and publishing tools outside the exported workspace.

## Checks

Review dependency direction in the three Cargo manifests. Run the private exporter tests before exporting.
