# Installation

Prebuilt binaries support macOS and Linux on ARM64 and x86-64.

```sh
curl -fsSL https://ev-grep.com/install.sh | bash
ev-grep --version
```

You can [read the installer](https://ev-grep.com/install.sh) before running it. If your shell cannot find `ev-grep`, open a new terminal or add the installation directory to `PATH`.

Or install with [mise](https://mise.jdx.dev/):

```sh
mise use -g github:macromackie/ev-grep
```

## First search

Set an OpenRouter API key, then search a directory:

```sh
export OPENROUTER_API_KEY='your-key'
ev-grep 'Catches a database failure and returns an empty result' src/
```

Use `--dry-run` first to list the files that would be sent. It needs no credentials.

```sh
ev-grep 'Catches a database failure and returns an empty result' src/ --dry-run
```

See [Providers](./providers.md) for direct TypeSafe access and model selection.

## Versions

Rerun the installer to update. For a pinned version, use its installer URL or specify a version in mise:

```sh
curl -fsSL https://ev-grep.com/v0.1.2/install.sh | bash
mise use -g github:macromackie/ev-grep@0.1.2
```

[GitHub releases](https://github.com/macromackie/ev-grep/releases) include archives and checksums. To build from source:

```sh
git clone https://github.com/macromackie/ev-grep.git
cd ev-grep
cargo build --release --locked
```
