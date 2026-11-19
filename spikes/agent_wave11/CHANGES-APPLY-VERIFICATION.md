# FUI-023 verification

Backend tests cover proposal, approval and application:

- `create`/`edit`/`delete` apply and write the exact proposed bytes; `delete`
  removes the file.
- a mismatched `base_version` conflicts and leaves the file untouched;
- an external change between review and apply conflicts and preserves user work;
- creating over an existing file conflicts;
- an untracked target applies with the `unprotected_files` warning and
  `undoAvailable: false`; a tracked dirty target applies with a private
  recovery reference and still reports `undoAvailable: false`;
- the authorizer records a `change_set` fact with the exact diff, and the
  executor records `change_set` + `recovery_record` facts;
- review and destructive confirmation prompts are still required (policy),
  and an approved instruction file cannot waive destructive authorization.

`read_file` now also returns a `version` (SHA-256) so the model can supply a
correct `base_version`.

Validation commands:

```text
cargo test
cargo fmt --check
cargo clippy --lib -- -D warnings
pnpm typecheck
```

Latest result: 438 library tests passed (4 added; the change-set fixtures in the
database tests were updated to the richer shape), 25 agent-cycle integration
tests and all other integration suites pass. `cargo fmt --check` passes. Strict
Clippy reports only the pre-existing nested conditional in `chat.rs`; no new
lint was introduced. `pnpm typecheck` passes.

No UI changed, so no capture was produced. No dev server was started and no
project folder was modified.
