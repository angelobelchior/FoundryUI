# FUI-025 verification

Recovery tests run against disposable Git repositories:

- `listing_exposes_protected_changes_per_run` — a tracked edit is listed per run
  as `protected` and non-destructive;
- `editing_back_is_rejected_when_the_user_changed_the_file_after_apply` — a
  later user edit returns `conflict` and preserves the user bytes;
- `deleting_back_restores_the_previous_content` — undo of a delete recreates the
  exact previous content from the protected ref;
- `undoing_a_create_requires_confirmation_and_then_removes_the_file` — create
  undo demands confirmation first, then removes the file;
- `a_missing_recovery_record_reports_no_protection`.

Change-tool integration still passes, including the updated
`a_tracked_dirty_target_applies_and_reports_git_ready_with_undo`, which now
asserts `undoAvailable: true` with a private recovery reference, while the
untracked and no-repository cases keep `undoAvailable: false`.

Validation commands:

```text
cargo test
cargo fmt --check
cargo clippy --lib -- -D warnings
pnpm typecheck
```

Latest result: 443 library tests passed (5 added), 25 agent-cycle integration
tests and all other suites pass. `cargo fmt --check` passes. Strict Clippy still
reports only the pre-existing nested conditional in `chat.rs`. `pnpm typecheck`
passes.

No UI changed (the backend commands were added for FUI-031), so no capture was
produced. No dev server was started and no project folder was modified.
