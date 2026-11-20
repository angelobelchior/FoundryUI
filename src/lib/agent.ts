import { invoke } from "@tauri-apps/api/core";

export type AgentToolOutcome = "succeeded" | "failed" | "cancelled";

export type AgentToolCard = {
  callId: string;
  tool: string;
  arguments: Record<string, unknown> | null;
  outcome: AgentToolOutcome | null;
  summary: Record<string, unknown> | null;
  diff: string | null;
};

export type AgentTurn = {
  messageId: number;
  tools: AgentToolCard[];
};

export const agentApi = {
  turns: (conversationId: number) => invoke<AgentTurn[]>("agent_turns", { conversationId }),
};
