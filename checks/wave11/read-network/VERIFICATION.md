# FUI-032: reading preference and conversation network history

## Delivered behavior

The chat's existing agent panel has a collapsed Reading and network section.
It exposes the single persisted global `read_allowed` preference, initially true,
and a paginated network history scoped to the selected conversation. There is no
project-level override. The current restriction is deliberately coarse: disabling
the preference blocks all new file reads, not just files considered sensitive.

Copy explains that authorized roots include sensitive, hidden and Git-ignored
files by default, that binary/large/unsupported files remain limited or omitted,
and that restricting new reads does not erase existing messages or attached copies.
English, Brazilian Portuguese and Spanish are included.

Network rows distinguish requested, blocked/denied, attempted, redirected,
completed, failed and cancelled. A request or attempt is not presented as a
completed access. Entries include run, tool/action origin when known, time and
HTTP status when available. They are historical records, not authorization grants.

## Backend corrections required by this surface

- Network audit phases now use separate idempotent fact keys instead of competing
  for one fact per action. Requested and attempted states precede the terminal
  result; a failed attempted audit prevents opening a connection.
- Redirects are logged without following the new destination. Dropping an active
  request records cancellation on a best-effort basis; an unrecorded terminal
  result remains an attempt, never an invented success.
- Denial records blocked rather than attempted/completed.
- Destinations are sanitized before persistence and again when projected to IPC.
  The client also reconstructs a sanitized destination for display and clipboard;
  it never renders a raw URL as a link or tooltip.
- Read and search executors reject stale authority. Search rechecks between files
  and discards results if authority changes before return. No index/cache grants
  access after a restriction. Scoped search also keeps its directory prefix.

## Verification

- `pnpm build` passes, including TypeScript checks.
- All 13 tests in `playwright.execution.config.ts` pass. The three new tests cover
  global persistence across conversations/reload, scoped log pagination and safe
  copying with synthetic credentials, and a failed preference write without a
  false success state.
- `cargo test`: 375 library tests, 24 agent-cycle integration tests, and all other
  integration suites pass. Eight existing live-environment tests remain ignored.
- Tests cover independent audit phases, denial without network attempts, no
  connection after audit failure, refused redirects, cancellation records,
  conversation-owned retention, hidden/ignored file eligibility and stale reads.
- `cargo fmt --check` passes. Strict library Clippy remains blocked only by the
  pre-existing conditional in `chat.rs:888`.
- The UI detector reports no findings. Synthetic captures were inspected at
  1440 and 960 pixels; scrolling is intentional inside the expanded bounded panel.

Browser tests intercept static build assets and mock IPC without starting a
development/preview server. Network executor tests use disposable local listeners.
No external destinations are contacted. Screenshots use only synthetic data.

## Remaining boundaries

This does not validate native confirmation dialogs against a real loaded model,
enable process execution, or enable file writes. Foundry was installed but its
server was stopped when inspected; it was not started for this change. FUI-029's
native end-to-end verification and FUI-020/FUI-024 remain separate pending work.
