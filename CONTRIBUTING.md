# Contributing to TTED

Keep changes incremental and preserve conventional keyboard, mouse, Unicode,
terminal-cleanup, and zero-configuration behavior. Avoid turning TTED into a
multiplexer, Vim clone, or AI-dependent application.

Before submitting a change, run:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

Add focused tests for core behavior, update user-facing documentation, and keep
`ROADMAP.md` and `PROJECT_STATUS.md` accurate. Do not commit generated
`target/` output.
