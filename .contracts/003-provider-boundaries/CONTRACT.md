---
title: Provider boundaries
description: Send bounded requests to the selected service without hidden routing.
---

# Provider boundaries

## Rules

- Select the provider explicitly through flags, environment, or the documented default. Use only that provider’s credential.
- Require a supported pinned model. Validate the response model, labels, probabilities, and confidence before recording a result.
- Disable HTTP redirects. Never print API keys or provider response bodies.
- Treat source as untrusted data. Keep evaluation expectations out of provider requests.
- Bound request concurrency, byte sizes, and timeouts. Do not add automatic retries or fallback models without documenting their cost and semantics.

## Checks

Run `cargo test -p ev-grep-jev --locked`. Use the opt-in live evals to measure model behavior; protocol mocks do not establish live compatibility.
