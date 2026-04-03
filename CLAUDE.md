# Claude Code Guidelines

## Before Every Commit

Run all of the following and fix any issues before committing:

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

Or to auto-fix formatting:

```bash
cargo fmt
```

## Commit Message Rules

- Always ask the human to approve the commit message before committing.
- Do not include "Co-Authored-By: Claude" or any AI attribution lines.
- Do not include advertising phrases like "Generated with Claude Code".

## README.md

Check whether `README.md` needs updating before committing. Update it if the change affects any of:

- Features visible to end users
- Configuration options or `config.toml` fields
- Installation steps or runtime dependencies
- Docker / deployment instructions
