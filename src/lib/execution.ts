import { invoke } from "@tauri-apps/api/core";
import type { SendReceipt } from "./chat";
import type { AuthorizationRequest } from "./authorization";

export type RunState = "prepared" | "running" | "complete" | "failed" | "cancelled" | "interrupted";
export type ExecutionStatus = {
  conversationId: number;
  mode: "review" | "automatic";
  contextRevision: number;
  authorizationRevision: number;
  modeRevision: number;
  execution: {
    run: { id: number; conversation_id: number; state: RunState; request_key: string };
    writerRemaining: boolean;
    events: { sequence: number; event: string; actionId: number | null }[];
  } | null;
  blocker: { run_id: number; conversation_id: number; state: RunState } | null;
  blockerTitle: string | null;
  pendingRequests: AuthorizationRequest[];
  continuationAvailable: boolean;
};

export type AgentRunEvent =
  | { type: "cycle"; event: { conversationId: number; runId: number; sequence: number | null } }
  | { type: "completed"; conversationId: number; outcome: { state: string } }
  | { type: "error"; conversationId: number; message: string };

export const executionApi = {
  status: (conversationId: number) => invoke<ExecutionStatus>("agent_execution_status", { conversationId }),
  mode: (status: ExecutionStatus, mode: ExecutionStatus["mode"]) => invoke<void>("agent_execution_mode", {
    conversationId: status.conversationId,
    contextRevision: status.contextRevision,
    authorizationRevision: status.authorizationRevision,
    modeRevision: status.modeRevision,
    mode,
  }),
  start: (conversationId: number, content: string) => invoke<SendReceipt>("agent_run_start", { conversationId, content }),
  cancel: (conversationId: number, runId: number) => invoke<{ writer_remaining: boolean }>("agent_run_cancel", { conversationId, runId }),
};

export function executionBusy(status: ExecutionStatus | null): boolean {
  const execution = status?.execution;
  return Boolean(execution && (execution.writerRemaining || ["prepared", "running"].includes(execution.run.state)));
}

export function executionStage(status: ExecutionStatus): "idle" | "generating" | "tool" | "pending" | "complete" | "failed" | "cancelled" | "interrupted" | "uncertain" {
  const execution = status.execution;
  if (!execution) return "idle";
  if (execution.writerRemaining && ["cancelled", "interrupted", "failed"].includes(execution.run.state)) return "uncertain";
  const last = execution.events.at(-1)?.event;
  if (execution.run.state !== "prepared" && execution.run.state !== "running") return execution.run.state;
  if (last === "authorization_pending") return "pending";
  if (last === "started") return "tool";
  return "generating";
}
