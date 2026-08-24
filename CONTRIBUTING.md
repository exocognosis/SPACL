# Contributing

## Development Setup

Install Rust 1.88 or later. Then run:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets
cargo doc --no-deps
```

## Pull Requests

Keep each pull request limited to one change. Add tests for security and state transitions. Update the security model when a change affects a trust boundary, token field, key, policy, audit record, or recovery path.

Do not commit private keys, robot credentials, production logs, or customer data.

Report security defects as described in [SECURITY.md](SECURITY.md).

