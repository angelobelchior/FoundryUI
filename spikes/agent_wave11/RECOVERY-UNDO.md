# FUI-025: selective undo by execution

## What changed

- `change_tools` now reports `undoAvailable: true` exactly when Git protection
  was prepared (a private recovery ref was created). Untracked/ignored targets
  and folders without a usable repository still report `undoAvailable: false`.
- `git_protection` gained `read_recovery`, which resolves a private recovery ref
  and returns the protected pre-action content (or absence for a create),
  validating the reference shape and bounding the read.
- `agent_runtime::recovery` exposes `list_recoverable` (per-run, per-file) and
  `recover` (single-file restore), exposed to the frontend through
  `recovery_list` / `recovery_undo`.

## Recovery rules

Recovery restores the content that existed before the action, not the last
commit. For each change set joined to a recovery record:

- `create` undo removes the file, but only when the current bytes still match
  the applied fingerprint; it is destructive, so it requires confirmation.
- `edit` undo rewrites the file back to the protected base, only when the
  current bytes still match the applied fingerprint.
- `delete` undo recreates the file from the protected base, only while the path
  is still absent.

A later user change to the file, or a file that reappears after a delete, makes
recovery return `conflict` without touching the file. The current authorized
root is reopened and revalidated, so a moved/revoked/unavailable root reports
`root_unavailable` instead of false success. A change with no recovery record
reports `no_protection`.

## Non-goals

- No global reset, stash, clean or branch commit is ever performed; only the
  selected file is restored through the resource layer.
- Recovery of effects produced by commands or external processes is not
  promised.
- The visual recovery/conflict UI is FUI-031; the backend here provides the
  primitive and the conflict/confirmation contract.
