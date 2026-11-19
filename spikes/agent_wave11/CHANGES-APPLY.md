# FUI-023: propose, review and apply file changes

## Flow

`propose_file_change` now goes from proposal to actual application. The same
plan is built by the authorizer and the executor so the reviewed diff and the
applied change are always the same content.

- `create`, `edit` and `delete` are supported.
- `base_version` is optional corroboration. When supplied it must equal the
  SHA-256 of the file read at proposal time; otherwise the proposal conflicts.
- The authoritative base is the bytes read at proposal time, recorded in the
  change set as a fingerprint. Application revalidates against that fingerprint,
  so an external change after review invalidates the approval instead of being
  silently overwritten.

## Proposal and approval

The authorizer reads the current file, builds a bounded diff and records a
`change_set` fact keyed `change-<action_id>`. The fact carries:

- `operation`, `path`, `base_fingerprint`, `proposed_fingerprint`,
  `proposal_fingerprint`, `additions`, `deletions`, `diff`, `diff_truncated`
  and `resource_keys`.

The approval is bound to the exact diff through
`concrete_change_id = change-<proposal_fingerprint>`, which is part of the
authorization fingerprint. If the file changes and the diff is recomputed, the
record conflicts and the stale approval is invalidated.

Review is the persisted default per conversation. A rejected proposal never
modifies files. Automatic mode still requires destructive confirmation for
deletion and never skips external-folder or network restrictions, because those
checks live in the policy layer, not the executor.

## Application

The executor, after the approval is dispatched:

1. rebuilds the plan and refuses when `concrete_change_id` no longer matches;
2. records (idempotently) the same `change_set` fact;
3. revalidates the base, prepares Git protection and records a
   `recovery_record` fact pointing at the private recovery ref when the target
   is eligible;
4. revalidates again and applies through the resource layer (`create_new`,
   `rewrite`, `remove`).

The result is reported per file with `status: applied`, the operation, the
path, base/applied fingerprints, the diff, the recovery inspection and
`undoAvailable: false` (recovery stays unavailable until FUI-025). Duplicate
events do not reapply: the cycle replays the stored result, and creation uses
`O_EXCL` so a repeat cannot overwrite.

## Resource primitives

`RootHandle` gained `rewrite` (truncate-and-write after strict regular-file and
hard-link checks) and `remove` (validated `unlinkat` beneath the root). Both
follow the existing `open_beneath` containment rules, refuse symlinks and
non-regular targets, and never escape the enrolled root.

## Not in scope

- Multi-file atomicity is not promised; a change set is one file per call.
- Semantic recovery (selective undo) is FUI-025.
- Command-mutation review is FUI-026.
- The visual diff/conflict UI is FUI-031.
