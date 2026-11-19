# FUI-022: context budget and per-call manifest

## Budget model

Every model call is planned from the messages that would otherwise be sent. The
planner is deterministic and local: it never calls a second model and never
changes permissions or tool availability.

- Capacity uses declared model metadata when the caller has it. Without trusted
  metadata the planner uses 8,192 tokens, marks the origin as `conservative` and
  attaches the warning key `context.capacity_conservative`.
- Response capacity is reserved from the requested output tokens, capped at half
  of the capacity; otherwise a conservative quarter of the capacity is reserved.
- A margin of 10% of the capacity is reserved for protocol overhead and new
  actions.
- Token estimates use bytes divided by three and are rounded up. This is a
  conservative estimate, not tokenizer output.
- A byte limit can tighten the allowance further; the smaller limit wins.

The resulting allowance is the input budget. Capacity never exceeds it: the
planner stops adding sources when the next unit does not fit.

## Selection and summaries

Messages are grouped so an assistant message with tool calls stays with all of
its tool results. A group is included or omitted as a unit, which keeps tool
correlation valid.

- Leading system policies and the newest turn are mandatory.
- Older groups are admitted newest-first while they fit.
- When the newest turn alone exceeds the allowance, its text is shortened and
  marked as a summary. Non-text parts become an explicit text note.
- When the first older group does not fit but space remains, that message is
  shortened and marked as a summary; older groups are omitted.
- Otherwise the group and every older group are omitted.
- A system notice tells the model that earlier context was reduced. The notice is
  counted against the budget instead of being added on top.

Original and summary content are never conflated: every source entry carries
`mode: "original" | "summary"`, a content fingerprint and an estimated size.

## Manifest

The planner returns a manifest with:

- capacity, origin, warning, reserved output, reserved margin, allowance and
  estimated input size;
- counts of original, included, summarized and omitted messages;
- `sources` with identity (`history.<index>.<role>`), kind, mode, SHA-256
  fingerprint, bytes and estimated tokens;
- `omissions` with identity, kind, reason (`budget` or `older`) and bytes;
- a `truncated` flag.

Kinds distinguish `policy`, `instructions`, `retrieved_context`, `user`,
`assistant` and `tool_result`. This is the data FUI-030 needs to render which
sources were used and why others were left out.

Manifests are persisted as `context_budget` facts in `agent_context_facts`:

- chat calls use `context_budget.message.<assistant message id>`, with no run;
- agent turns use `context_budget.<run id>.<step>` bound to the run revision.

Keys are unique per conversation revision. A persistence failure does not change
what is sent; the local budget still applies. The manifest JSON is validated as
an object and capped at 128 KiB before storage.

## Limits

- Conservative capacity is 8,192 tokens until trusted model metadata is wired
  through the caller.
- The planner does not summarize semantically; it selects and truncates while
  labelling the result as a summary.
- Attachments are counted through their message payload. Non-text parts can be
  omitted when a message is shortened; the manifest records that decision.
- Tool definitions are outside the manifest; the agent runtime already counts
  them in its byte limit.

## Follow-up

FUI-030 exposes the manifest. FUI-034 and FUI-035 exercise large budgets, real
histories and failure transitions. Declared model capacity can be passed by a
caller once the runtime exposes trustworthy metadata; no other planner change is
needed.
