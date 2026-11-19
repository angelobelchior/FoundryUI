# FUI-031 (file changes): applied changes, diff and selective undo UI

## What changed

The execution section now lists the file changes actually applied in the
conversation, grouped by run, with the operation (created/edited/deleted), the
path, the +/- line counts and the recorded diff. Each change that has real Git
protection shows an "undo" action; changes without a recovery record show
"no recovery is available".

Undo routes through the backend `recovery_undo` command:

- undoing a created file is destructive (it removes the file), so the UI asks
  for confirmation first and then re-invokes with `confirmedDestructive: true`;
- undo revalidates the current bytes; a later user change returns `conflict`
  without touching the file, which the UI reports verbatim;
- a moved/revoked/unavailable root, a missing protection record and other
  failures are reported with their specific message.

## Backend fixes

- `recovery_list` now takes an optional `run_id` and returns the diff, line
  counts, truncation flag and run id alongside the existing fields, so the UI
  can render the change and group by execution.
- `fact_by_key` no longer scopes to the current context revision: change and
  recovery facts are immutable historical records, so undo keeps working after
  the context revision advances (e.g. after another run or message). A test
  covers `fact_by_key` across a revision bump.

## Not in scope

- Review/proposal of command mutations remains blocked until FUI-026 is
  unblocked by a proven command-isolation mechanism.
- Accessibility/localization verification is FUI-033.
