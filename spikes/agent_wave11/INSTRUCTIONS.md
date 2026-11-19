# FUI-020: version-bound folder guidance

## Recognized sources and scope

The engine recognizes `CLAUDE.md` and `AGENTS.md`. Discovery examines the authorized
root and the ancestor directories of explicitly supplied target file paths. It
never walks above the root, recursively searches unrelated directories, or follows
child symlinks. `InstructionTarget.path` names a relevant file; an empty target list
discovers only root-level sources.

A project conversation can discover sources only in its current project root.
A projectless conversation requires an explicit current standalone-root grant;
enrolling a root alone is insufficient. Action-specific external-project roots do
not become standing instruction scopes. Global read restrictions also apply to
instruction discovery and prompt preparation.

Directory guidance is ordered from root to the most specific relevant directory.
Within one directory, `CLAUDE.md` is lower priority than `AGENTS.md`. Sources from
different roots have no implied precedence. All guidance is scoped to its own root
and directory, and the current user's task takes precedence over it.

The prompt contract requires unresolved task conflicts and omitted/pending source
limitations to be explained to the user. This is not a semantic contradiction
detector or a claim that arbitrary models perfectly follow that contract. Effective
permissions remain independently enforced by backend policy.

## Decisions and freshness

`agent_instruction_decisions` stores decisions bound to:

- conversation and context revision;
- authorization revision and root identity;
- normalized relative path;
- the opened file's identity and filesystem version;
- SHA-256 of the complete source bytes.

The opened-file identity includes device/inode/generation and birth time. The
version also includes size, modification/change timestamps and mode. File bytes
and metadata are checked around the bounded read. Identical content in a replaced
file does not inherit approval; changing and restoring contents also requires a
current version decision.

Native previews are instance-bound, non-deserializable Rust values. The frontend
supplies expected metadata, never replacement source text. The backend rereads
the source before showing it and again before committing the decision. A changed
source, context, authorization revision or database instance rejects a delayed
decision. Mode changes invalidate an open preview but do not erase an already
approved unchanged instruction source or grant any operation.
The transaction also checks the previous decision state, so an older open preview
cannot silently overwrite a newer refusal. A new explicit preview/confirmation can
change the decision for the same unchanged source.

Approving guidance never creates an operation/root/network grant, satisfies a
destructive confirmation, approves a pending tool action, or authorizes remote
inference. Legacy `ContextFact::InstructionApproval` values and historical copies
are not the authoritative decision store. Decisions belong to their conversation
and are removed when that conversation is deleted; source files are preserved.

## Prompt integration

The ordinary chat path and the assembled tool cycle use the same source contract.
Root-level guidance is considered initially. The tool-cycle wrapper additionally
observes structured read/search/change `path` arguments and considers applicable
nested guidance for subsequent turns. It does not extract paths or commands from
free text.

Sources are freshly reread before every model turn. Only exact approved versions
contribute source text. Unapproved/denied sources contribute at most identifiers
and status, not instruction bytes. Approved source text is quoted task-context
data, never a new system message or a permission record. A fixed trust policy
explains precedence and the limits of document authority.

The wrapper does not retain injected source messages in the cycle's underlying
history. A source changed or revoked between turns therefore cannot reappear as
current guidance from a cached prompt. Already existing user/model messages remain
historical conversation content; this feature does not promise retroactive erasure.

## Bounds and native presentation

- Up to 16 target paths, each with at most 16 relevant ancestor directories.
  Exceeding the tracked tool-path limit is an explicit controlled failure, not a
  silent decision to ignore a newly relevant instruction scope.
- Up to 16 roots and 32 returned sources per discovery.
- Complete source reads up to 64 KiB, with a 256 KiB combined discovery-content cap.
- Binary/unsupported/control-decorated text, symlinks, unavailable sources and
  limit omissions are reported, not silently followed.
- A source that cannot fit the remaining model-input budget is omitted whole;
  its content is never truncated and presented as the approved full version.
  Lower-priority sources are omitted before more-specific sources.

The current native message confirmation accepts only short, printable previews:
at most 2 KiB, eight lines, 80 characters per line, and a 256-character path. The
complete body and fingerprint are passed to the native confirmation without a
truncation operation. Sources outside this presentation envelope remain
unapprovable through this command until a validated full-content viewer is added.

FUI-028 owns the dedicated instruction-review list and keyboard workflow. The
backend commands `instructions_discover`/`instructions_decide` and the typed
`src/lib/instructions.ts` client are available for that integration. Positive
decisions require native confirmation; dismissing it makes no decision. Denial
does not grant authority.

## Persistence boundary

This adds a table to `agent-schema.sql`. The repository's existing schema-stamp
policy recreates incompatible conversation databases; no migration is introduced.
The real application/database was not opened during implementation. Filesystem
operations in tests are limited to disposable fixtures; no project source folder
is removed by this feature.
