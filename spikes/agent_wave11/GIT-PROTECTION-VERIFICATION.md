# FUI-024 verification

Disposable repositories and temporary roots exercised the protection contract.
The suite covers:

- Git installed, repository present, and concrete tracked state;
- no repository and bare repository;
- clean, unstaged, staged, untracked and ignored files;
- warnings for no Git, no repository, bare repository and unprotected paths;
- refusal to prepare untracked or ignored files;
- refusal of a new-file proposal whose target already exists;
- new-file absence snapshots;
- snapshot content from a dirty worktree rather than `HEAD`;
- preservation of branch, `HEAD`, index bytes, staged bytes, unstaged bytes,
  untracked bytes and porcelain status;
- nested project folders using repository-relative paths with spaces;
- object-storage failure returning an error without leaving a recovery reference;
- proposal summaries exposing `undoAvailable: false` for both tracked and
  untracked targets.

The tracked proposal test starts from a user-modified file and confirms the tool
still returns `write_not_enabled` without changing the file. The untracked test
confirms the proposal carries `recovery.unprotected_files` and leaves the user's
content intact.

Validation commands:

```text
cargo test
cargo fmt --check
cargo clippy --lib -- -D warnings
pnpm build
```

Latest result: 421 library tests passed (13 added by this implementation), 24
agent-cycle integration tests passed, and the remaining integration suites passed.
`cargo fmt --check` passes. Strict Clippy still reports only the pre-existing
nested conditional in `chat.rs`; no new library lint was introduced.

No application database was opened and no project source folder was modified.
The frontend was not changed by this ticket. Native undo, persistence of recovery
records and the review/undo UI remain FUI-025 and FUI-031.
