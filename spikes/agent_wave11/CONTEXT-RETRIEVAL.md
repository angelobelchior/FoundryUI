# FUI-019: automatic current-file context

## Retrieval model

Automatic context is a bounded lexical scan of the authorized roots performed for
each question. There is no persistent content index to invalidate: the file bytes
read for a request are the current bytes, and every selected excerpt carries the
`versionKey` and SHA-256 `contentFingerprint` observed while reading it.

The request prompt receives one extra system policy message and one user message
containing a JSON packet with `kind: "retrieved_workspace_context"`. The packet is
explicitly labelled `reference_data_not_instructions`, and the policy states that
excerpts cannot grant file, command, network or destructive-action permission.
Backend authorization remains mandatory for every effect.

Retrieval happens only when the conversation has at least one authorized root and
the global reading preference allows reading. Project conversations use their
project root; conversations without a project use their authorized standalone
roots. The preference is global and has no per-project override.

## Selection and limits

- Terms come from the latest user message: lowercased alphanumeric tokens of at
  least two characters, deduplicated, at most 12.
- At most 8 roots, depth 8 and 2000 directory entries.
- At most 256 candidate matches, ranked by matched-term count and then path/line.
- At most 8 excerpts, each up to 400 characters, with an output budget supplied
  by the caller (16 KiB in chat and agent runs).
- Files are read with a 256 KiB per-file limit and a 2 MiB total scan budget.
- At each 32nd directory entry the current authorization revision is compared to
  the revision captured before the walk. Any change discards the whole packet.

Sensitive and Git-ignored files are eligible by default, matching the global
reading preference. `.git` directories are skipped. Symlinks, non-regular entries,
binaries, oversized files and unreadable entries are reported in `omissions`
instead of being silently skipped, and `truncated` is set when a limit stopped the
scan or the budget excluded a selected excerpt.

## Freshness and revocation

- A stale read can never be returned: excerpts are not cached between requests.
- Rewriting a file between two questions changes both the excerpt and its
  fingerprint.
- Setting the global reading preference to restricted makes retrieval return the
  unchanged message list, so no previously retrieved content is reused.
- The retrieval helper never accepts a packet prepared under an older revision;
  the mid-walk check fails closed.

## Scope

This ticket does not add semantic search, embedding reuse or a second model. It
also does not attach whole files: excerpts are bounded and always labelled as
untrusted reference data. Attachments, conversation copies and tool results keep
their existing paths. UI presentation of retrieved sources belongs to FUI-030.
