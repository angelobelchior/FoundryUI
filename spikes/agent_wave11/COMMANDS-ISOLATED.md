# FUI-016 + FUI-026: sandboxed commands and mutating-command review

## What changed

`run_command` is now implemented instead of being permanently blocked.

### FUI-016 — sandboxed execution (`command_execution.rs`)

- A deny-by-default `sandbox-exec` profile on macOS, matching the wave-1 spike:
  network denied, reads limited to system runtime paths, and file read/write
  confined to the working directory.
- Reduced environment (`PATH`, `HOME`, `TMPDIR`, `LANG`), process group
  isolation, a bounded timeout with `SIGKILL` to the process group plus a direct
  kill fallback, and bounded stdout/stderr capture.
- `capture_command_effects` copies the authorized root into a temporary staging
  directory (skipping `.git`/`.DS_Store`, bounded by size and file count), runs
  the command there, and diffs the staging directory against the real root to
  produce per-file `create`/`edit`/`delete` changes.

### FUI-026 — mutating commands reviewed before touching the real folder

- `run_command` is available only when the sandbox is present
  (`command_sandbox_available`); otherwise it stays `CapabilityBlocked`.
- `CommandAuthorizer` runs the command in the staging area during facts
  evaluation, records a `CommandChange` fact (per-file base/proposed
  fingerprints and content) and a `ChangeSet` fact, and classifies effects:
  `ReadOnly` (no file changes), `Alteration`, or `Destructive` (any deletion).
- In review mode a mutating command requires `AlterationReview`; a deletion
  requires destructive confirmation even in automatic mode. No policy is
  implemented in the frontend: the backend re-validates everything.
- `CommandExecutor` (after approval) re-applies each staged file to the real
  root via the resource layer, revalidating that the current bytes still match
  the staged base fingerprint; a later external change returns `conflict`
  without writing.

## Persistence

- New `ContextFact::CommandChange` (`command_change` kind in the schema) plus
  `CommandFileChange`, with bounded validation.

## Not proven yet (validation deferred)

- The lifecycle gap from the wave-1 spike (`setsid` descendants escaping group
  kill) is documented, not resolved; `cleanup_verified` is not claimed.
- End-to-end behaviour with a real model and the actual `sandbox-exec` binary is
  left for the user's validation pass.
