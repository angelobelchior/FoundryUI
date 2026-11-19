# FUI-036 — Delivery gate consolidation (working report)

This report confronts the E1–E7 gates and the T01–T37 matrix with the evidence
currently in the repository. It is a consolidation, not a release: it does not
publish, and it does not mark unexecuted tests as passed. FUI-036 remains open
until FUI-033 (accessibility), FUI-034 (multi-platform) and FUI-035
(end-to-end with a real local model) are resolved.

## Scope decision

macOS only. Windows and Linux are explicitly deferred (FUI-003, FUI-004). See
`../../FoundryUI-Specs/09-decisoes-confirmadas.md`.

## Static verification (latest run, branch `main`)

- `cargo test` — 444 library tests pass (8 ignored, macOS-gated), plus
  integration suites: `agent_cycle` 25, `project_creation` 12,
  `local_inference` 13, `project_selection` 5, `lifecycle_confirmation` 5,
  `project_navigation` 2, `context_lifecycle` 1, `local_context_transport` 1,
  `permission_commands` 1, `snapshot_commands` 1. No failures.
- `cargo fmt --check` — clean.
- `cargo clippy --lib -- -D warnings` — clean except one pre-existing
  `collapsible_if` in `chat.rs` (a user-owned file, not part of these waves).
- `pnpm typecheck` and `pnpm build` — pass.

## Gates

| Gate | Status |
|---|---|
| E1 — technical proof | Files/sockets contained; command lifecycle escaped (`setsid`) → commands stay `CapabilityBlocked`. Network is mediated + logged. |
| E2 — persistence/org | Implemented and tested (projects, runs, actions, revisions, snapshots, interrupted-recovery). |
| E3 — runtime/policy | Implemented and tested (cycle, approvals, grants, revocation, concurrency, cancellation, sanitized network logs). |
| E4 — context/references | Implemented and tested (retrieval, approved instructions, context budget, independent attachments). |
| E5 — changes/Git | Implemented and tested for files (propose/apply, protection, selective undo). Mutating commands remain blocked (FUI-026). |
| E6 — integrated experience | Backend + UI panels implemented; keyboard/accessibility verification (FUI-033) not executed. |
| E7 — product validation | Automated suites pass; real-model E2E and UI captures not executed (no local model loaded; no dev server started). |

## Capability limitations (explicit)

- **Commands** (`run_command`): `CapabilityBlocked`. FUI-002's macOS candidate
  failed its negative lifecycle test; there is no approved process isolation.
  Authorizing a command therefore stays unavailable rather than falling back to
  an unrestricted shell.
- **Mutating-command review** (FUI-026): blocked by the same gap. T35 has no
  passing result; it is recorded as a blocker, not a success.
- **Windows/Linux**: deferred; no platform support claimed.
- **Native end-to-end** (loaded model) and **UI captures**: not produced this
  session; no dev server or inference process was started.

## Scenario matrix

| Scenario | Result | Primary evidence |
|---|---|---|
| T01 folder identity dedup | automated | `project_creation`/`project_selection` root-identity tests |
| T02 move conversation | automated | context-cycle/project tests |
| T03 folder unavailable | automated | project tests (`missing_or_replaced_roots…`) |
| T04 remove project, keep conversations | automated | project tests |
| T05 remove project + conversations | automated | project/retention tests |
| T06 permission per conversation | automated | authorization tests |
| T07 specific operation | automated (files/network); commands blocked | authorization tests |
| T08 external never remembered | automated | authorization tests |
| T09 standalone folder grant | automated | authorization tests |
| T10 destructive confirmation | automated | policy + change-tools tests |
| T11 path/symlink escape | macOS files/sockets contained; Windows/Linux deferred | spike `agent_wave1` + resource tests |
| T12 descendant escape | **blocked** | FUI-002 `setsid` negative result |
| T13 command network | **blocked** (commands unavailable) | `CapabilityBlocked` |
| T14 redirect re-evaluated | automated | network-tool tests |
| T15 credential sanitization | automated | network/db tests |
| T16 opaque network blocked | automated | network-authorizer tests |
| T17 inference origin | automated | `local_inference` tests |
| T18 instruction does not grant | automated | instruction tests |
| T19 instruction version | automated | instruction tests |
| T20 external file change | automated | change-tools conflict test |
| T21 revoked scope | automated | retrieval/read tests (no persistent index) |
| T22 independent copy | automated | snapshot tests |
| T23 no recursion/reexecution | automated | snapshot tests |
| T24 budget/summary provenance | automated | context-budget tests + `ContextPanel` |
| T25 incompatible model/call | automated | agent-cycle/local tests |
| T26 one executor per project | automated | concurrency tests |
| T27 overlapping folders | automated | concurrency tests |
| T28 approve after cancel | automated | authorization-resume tests |
| T29 cancel descendants | **blocked** (command process tree) | FUI-002 |
| T30 crash mid-effect | automated | interrupted-recovery tests |
| T31 no Git warning | automated | change-tools untracked test |
| T32 preserve existing Git work | automated | recovery tests |
| T33 per-file protection | automated | git-protection + change-tools tests |
| T34 undo after later edit | automated | recovery conflict test |
| T35 mutating command reviewed | **blocked** | FUI-026 |
| T36 duplicate/obsolete decision | automated | cycle `duplicate_call_ids` + stale-auth tests |
| T37 keyboard/screen reader | **not executed** | FUI-033 |

## Outstanding for completion

- FUI-033 — accessibility/localization validation (needs UI review + axe/AT).
- FUI-034 — multi-platform regression (Windows/Linux, currently deferred).
- FUI-035 — integration/resilience with a real compatible local model, plus the
  UI captures at supported sizes for the interface flows.
- FUI-026 — unblocking mutating-command review requires a proven command
  isolation mechanism (revisit FUI-002).
