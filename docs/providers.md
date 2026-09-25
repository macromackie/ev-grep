# Providers

OpenRouter is the default:

```sh
export OPENROUTER_API_KEY='your-key'
ev-grep 'Performs database operations' src/
```

For direct TypeSafe access:

```sh
export TYPESAFE_API_KEY='your-key'
ev-grep --provider typesafe 'Performs database operations' src/
```

## Models

| Provider | Credential | Default and supported model |
| --- | --- | --- |
| `openrouter` | `OPENROUTER_API_KEY` | `typesafe/jev-1.13-20260917` |
| `typesafe` | `TYPESAFE_API_KEY` | `jev-1.13.0` |

```sh
ev-grep --provider openrouter --model typesafe/jev-1.13-20260917 \
  'Performs database operations' src/
```

Set `EV_GREP_PROVIDER` and `EV_GREP_MODEL` for shell defaults. Flags override those variables. API keys do not select a provider implicitly. Unsupported models fail before requests.

## Requests

Each request contains the query, path, and complete contents of one file. Files are assessed independently; ev-grep does not retrieve related code. Requests may incur provider charges.

The adapter uses OpenRouter's Decisions endpoint or TypeSafe's System One endpoint. It does not retry failures, follow redirects, or fall back to another provider. Custom endpoints and automatic `.env` loading are not supported.
