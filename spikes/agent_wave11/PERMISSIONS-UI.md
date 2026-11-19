# FUI-028: permissions, grants and instructions UI

## What changed

The agent execution section now surfaces the three authorization surfaces the
backend already enforces, plus the remember option:

- Each pending authorization request shows its requirements with the
  operation/target/recovery text produced by the backend, an "allow once"
  action and — only when every requirement is `rememberEligible` — an
  "approve and remember" action. External project access and destructive
  actions are never remember-eligible, so the button does not appear for them.
- A new `PermissionsPanel` lists this conversation's folder grants with a revoke
  action, and offers "grant folder access" only for non-project conversations
  (the backend rejects `authorization_pick_root` for project-bound ones).
- The same panel discovers `AGENTS.md`/`CLAUDE.md` instruction files, shows each
  source's status, version key and (bounded) content preview, and offers
  approve/deny for pending sources, alongside the explicit note that approved
  instructions never grant file, network, command or destructive permissions.

## Safety

No policy is implemented in the frontend. The buttons only invoke backend
commands (`authorization_decide`, `authorization_revoke`,
`authorization_pick_root`, `instructions_decide`), and the backend re-validates
status, preview availability, remember eligibility and stale context. Revocation
and errors re-read state instead of assuming success.

## Not in scope

- The visual diff/conflict/undo flow for applied changes is FUI-031.
- Accessibility/localization verification is FUI-033.
