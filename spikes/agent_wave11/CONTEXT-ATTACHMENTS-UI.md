# FUI-030: context transparency and conversation attachments UI

## What changed

- The execution section now records and shows, per call, the context budget
  manifest produced by FUI-022: capacity origin (with the conservative warning
  when no trusted model capacity was available), included/summarized/omitted
  message counts, and the per-source and per-omission entries with identity,
  kind, mode (original vs summary), fingerprint and size. A new
  `context_budgets` command reads the persisted `context_budget` facts.
- A conversation-attachment panel lets the user search any listable
  conversation (including those in other projects), attach a read-only copy, and
  list/remove attached copies. Each copy shows its source title, capture date,
  a cross-project warning when the source belonged to another project, omitted
  parts, and an explicit note that the copy is independent and read-only.

## Safety

The backend still owns capture, cross-project confirmation and removal
(`snapshots_capture` / `snapshots_remove`), including the native
cross-project warning. The frontend only invokes those commands and re-reads
state after each action. Attached copies remain `historical_evidence`; the UI
does not present them as permissions or re-executable instructions.

## Not in scope

- Semantic search or richer excerpt inspection beyond the recorded manifests.
- Accessibility/localization verification is FUI-033.
- The visual diff/conflict/undo flow is FUI-031.
