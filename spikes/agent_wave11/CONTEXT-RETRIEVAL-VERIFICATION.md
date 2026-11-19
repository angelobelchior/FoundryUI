# FUI-019 verification

Tests use an in-memory database with a real temporary project root and real files.

Covered:

- a question finds relevant excerpts in the current authorized files without
  manual attachment, including hidden `.env` and Git-ignored files;
- the inserted policy message is separate from the data packet and both precede
  the original conversation;
- selected excerpts carry path, line, `versionKey`, 64-character SHA-256
  fingerprint and score;
- rewriting a file between two questions returns the new excerpt, drops the old
  excerpt and changes the fingerprint;
- restricting global reading returns the original message list and no content;
- binary files, symlinks pointing outside the root and oversized files appear as
  `binary`, `symlink` and `large` omissions, and outside content never appears in
  the prompt;
- a budget smaller than every excerpt yields a truncated packet with no sources.

Validation commands:

```text
cargo test
cargo fmt --check
cargo clippy --lib -- -D warnings
pnpm typecheck
```

Latest result: 426 library tests passed (5 added by this implementation), 24
agent-cycle integration tests passed, and the remaining integration suites
passed. `cargo fmt --check` passes. Strict Clippy still reports only the
pre-existing nested conditional in `chat.rs`; no new lint was introduced.

No project folder was modified, no application database was opened and no
dev server was started. No UI changed, so no capture was produced. The frontend
was checked only for type and build integrity.
