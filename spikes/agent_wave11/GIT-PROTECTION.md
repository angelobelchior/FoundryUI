# FUI-024: real Git protection preparation

## Detection contract

`git_protection` resolves a local Git executable from fixed macOS locations, runs
it with a cleared environment, and never invokes a shell. It distinguishes:

- Git not installed or unusable;
- a folder outside any repository;
- a bare repository without a work tree;
- an invalid repository/work-tree relationship;
- a valid work tree with a tracked, untracked, ignored, clean, staged, unstaged or
  staged-and-unstaged target.

`ls-files` decides whether a concrete path is tracked. `check-ignore` decides
whether an untracked path is ignored. `status --porcelain=v1 -z` supplies the
working-tree state. A `.git` directory alone never proves file protection.

The inspection result always carries `undoAvailable: false`. Protection is only
advertised after a snapshot reference has actually been created; inspection alone
never claims that undo exists.

## Warning contract

Warnings are attached to the change proposal returned by `propose_file_change`,
so they are available before approval in both review and automatic modes:

- no Git installation: `recovery.no_git`;
- no repository: `recovery.no_repository`;
- bare repository: `recovery.bare_repository`;
- invalid repository: `recovery.repository_invalid`;
- concrete unprotected files: `recovery.unprotected_files`, with their paths.

The warning states that the application cannot undo those changes. It does not
block an operation by itself. The existing write gate remains in place in this
wave, so no alteration is applied while the proposal is rejected with
`write_not_enabled`.

## Private snapshot preparation

`prepare` stores the previous content using Git object storage without touching
the user's branch, index or working tree:

1. read the base bytes through `RootHandle` with a 8 MiB limit;
2. write an exact, unfiltered blob with `hash-object -w --no-filters`;
3. build a tree in a disposable index selected by `GIT_INDEX_FILE`, never the
   repository index;
4. create a commit object with fixed private metadata;
5. create a reference under `refs/foundryui/recovery/`;
6. verify that the reference resolves to the created commit.

A new-file preparation records an empty tree, proving the target was absent. It
does not store an invented previous content.

If any step fails, preparation returns an error and no reference is left behind.
The caller therefore cannot announce a recoverable result on a failed preparation.

Only tracked files are snapshotted. Untracked and ignored files are reported as
not recoverable rather than silently copied into the object database; this avoids
turning ignored secrets into implicit backups. Ignored new files are also refused.
New, non-ignored files inside a repository can record absence and remain eligible
for the selective removal flow in FUI-025.

## Preserved state

Tests prove, against disposable repositories, that preparation preserves:

- the branch and `HEAD`;
- the repository index byte-for-byte;
- staged content;
- unstaged worktree content;
- untracked work;
- the porcelain status before and after the snapshot.

The snapshot of a dirty tracked file contains the current worktree bytes, not the
last commit. This is the content selective undo must restore in FUI-025.

## Limits and follow-up

- macOS only, consistent with the current delivery scope.
- Git is found through fixed executable paths; the application PATH is not used.
- Git commands are bounded to 64 KiB of output, have prompts disabled, do not run
  hooks, and do not modify user refs.
- Base content is limited to 8 MiB per file.
- Actual file application is still gated by the existing capability boundary.
  FUI-025 must prepare a fresh snapshot immediately before applying and persist a
  `RecoveryRecord`; FUI-031 owns the review/undo interface.
- Command-produced changes remain unavailable while FUI-016/FUI-026 isolation is
  rejected.
