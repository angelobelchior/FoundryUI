# Persisted approval continuation

## Behavior

Production cycles retain their continuation in memory while waiting for a
persisted authorization decision. No tool arguments or model-message continuation
are restored from redacted historical records. Approval re-enters the existing
dispatch gate with fresh facts; denial becomes a tool result. Remaining calls in
the same turn are preserved. Waiting consumes the same bounded cycle duration.

The interface calls the existing `authorization_decide` command. Positive decisions
still require the native dialog and a complete preview. A missing preview disables
approval without blocking denial. Cancelling native confirmation leaves the
request pending. Changing mode, revoking authority or cancelling a run cannot
approve an old request. After reopening, no execution is automatically replayed.

## Evidence

- Agent-cycle integration tests cover approve-and-continue across two pending
  calls, denial without execution, cancellation with a late decision, mode
  invalidation, the approval-wait deadline, and fresh-facts revalidation.
- Ten Playwright tests cover execution controls and approval IPC, including an
  incomplete preview and cancelled native confirmation.
- `incomplete-preview-1440.png` uses synthetic fixture data and mocked IPC. It was
  inspected for control visibility, wrapping and explicit disabled-approval copy.
- Browser verification serves build assets through route interception, without
  starting a preview/development server or contacting external destinations.

## Limits

Native dialog interaction and the complete path against a real loaded model have
not been exercised here. Existing capability gates still apply: opaque network
or change previews remain unapprovable; process execution and actual file writes
are not enabled by this change. An interrupted or uncertain writer is not released
merely because its UI has reconnected or a pending dialog was dismissed.
