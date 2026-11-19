# FUI-022 verification

Deterministic planner tests cover:

- a 40-unit history under a 2,048-token capacity: the latest turn and policy stay,
  older groups are omitted, the notice is present, no omitted content leaks, and
  the serialized result stays inside the allowance;
- a single latest message larger than the allowance: text is shortened, marked
  as a summary, and the result stays inside the allowance;
- tool groups with two tool results: the group stays whole when it fits and is
  dropped whole when it does not, with no orphan `tool` message;
- conservative capacity reporting and the `context.capacity_conservative`
  warning when no metadata is declared;
- source fingerprints, modes and omission reasons in the manifest;
- a byte limit smaller than the token allowance tightening the result.

Agent-cycle integration covers:

- every turn persists a `context_budget` fact with a conservative origin and the
  warning key;
- the 24 existing cycle tests still pass, including duplicate call IDs, multiple
  tool calls per turn, resumed approvals and history correlation.

Database tests cover persistence and validation: a manifest object is stored as a
fact and invalid JSON is rejected as `InvalidInput`.

Validation commands:

```text
cargo test
cargo fmt --check
cargo clippy --lib -- -D warnings
pnpm typecheck
```

Latest result: 434 library tests passed (8 added by this implementation), 25
agent-cycle integration tests passed (1 added), and the remaining integration
suites passed. `cargo fmt --check` passes. Strict Clippy still reports only the
pre-existing nested conditional in `chat.rs`; no new lint was introduced.

No UI changed, so no capture was produced. No dev server was started and no
project folder was modified.
