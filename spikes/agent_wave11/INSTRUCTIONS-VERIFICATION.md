# FUI-020 verification

Tests use disposable roots and SQLite databases, plus scripted model providers.
They cover root/ancestor scope and precedence, no parent traversal, approval and
denial, content changes, identical-content file replacement, same-object content
ABA, conversation isolation, context invalidation, root-grant revocation, global
read restriction, legacy-fact forgery, cross-database previews, binary/large or
symlink sources, invisible controls, source preservation on conversation deletion,
and persisted decisions after reopening.

Native-command boundary tests verify complete source/fingerprint presentation in
English, Portuguese and Spanish, ignored frontend replacement text, source changes
during a simulated native confirmation, and database replacement while a preview
is open. They do not operate actual OS dialogs.

Prompt tests verify that unapproved bytes are absent, approved text is not promoted
to a system role, command capability gates remain closed, changed source versions
are excluded on the next turn, structured tool paths select nested sources, read
restrictions remove current guidance, whole-source budget omissions are explicit,
and the ordinary chat helper uses the same approval contract.

Validation commands:

```text
cargo test
cargo fmt --check
cargo clippy --lib -- -D warnings
pnpm build
```

The full Rust suite and frontend build pass. Strict Clippy remains blocked by the
pre-existing nested conditional in `chat.rs` (its line moved after this integration);
no new library lint was reported. Real-model/native end-to-end verification is
still pending, and no server was started for these tests. No frontend interface
surface was changed by this ticket; the dedicated review UI remains FUI-028 work.

Latest result: 408 library tests passed (33 added by this implementation), eight
existing live-environment tests ignored, 24 agent-cycle integration tests passed,
and all other integration suites passed.
