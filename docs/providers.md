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

| Provider     | Credential           | Supported models, default first |
| ------------ | -------------------- | ------------------------------- |
| `openrouter` | `OPENROUTER_API_KEY` | `typesafe/jev-1.13-20260917`    |
| `typesafe`   | `TYPESAFE_API_KEY`   | `jev-1.13.0`                    |

```sh
ev-grep --provider openrouter --model typesafe/jev-1.13-20260917 \
  'Performs database operations' src/
```

Set `EV_GREP_PROVIDER` and `EV_GREP_MODEL` for shell defaults. Flags override those variables. API keys do not select a provider implicitly. Unsupported models fail before requests.

The table lists the models that ev-grep's model evaluations have checked. To send a model outside it, such as a newer
Jev release, add `--unvalidated-model`:

```sh
ev-grep --provider typesafe --model jev-1.14.0 --unvalidated-model \
  'Performs database operations' src/
```

The `begin` record then sets `model_validated` to `false`. Responses must still name the requested model.

## Requests

Each request contains the query, candidate text and location, any supplied original file context, and explicit
supporting sources. Path searches supply the complete candidate file even when a range narrows the target.
Candidates are assessed independently; ev-grep does not retrieve related code. Requests may incur provider charges.

The adapter uses OpenRouter's Decisions endpoint or TypeSafe's System One endpoint. Retries stay within the
[request limits](./limits.md) and use the same provider and model. It does not follow redirects or fall back to
another provider. Custom endpoints and automatic `.env` loading are not supported.
