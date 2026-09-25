---
title: Rust and checks
description: Keep the Rust workspace readable and locally verifiable.
---

# Rust and checks

## Rules

- Format with rustfmt and pass Clippy with warnings denied. Prefer explicit control flow and ordinary library types.
- Use Result for recoverable errors. Keep unsafe code forbidden; avoid unwrap and expect in maintained code and tests.
- Test observable guarantees at their owning boundary. Keep paid model evaluations outside the default suite.
- Keep shared dependency versions and lints in the workspace manifest. Check in Cargo.lock.
- Define workspace and crate-local test, lint, typecheck, fmt, and check tasks using mise templates.

## Checks

Run `mise run check`; use `mise run check` inside a crate for a narrower change.
