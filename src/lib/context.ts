import { invoke } from "@tauri-apps/api/core";

export type ContextSource = {
  identity_key: string;
  kind: string;
  mode: "original" | "summary";
  content_fingerprint: string;
  bytes: number;
  tokens: number;
};

export type ContextOmission = {
  identity_key: string;
  kind: string;
  reason: string;
  bytes: number;
};

export type ContextManifest = {
  capacity_tokens: number;
  capacity_origin: "declared" | "conservative";
  warning: string | null;
  reserved_output_tokens: number;
  reserved_margin_tokens: number;
  allowance_tokens: number;
  allowance_bytes: number;
  estimated_input_tokens: number;
  estimated_input_bytes: number;
  original_messages: number;
  included_messages: number;
  summarized_messages: number;
  omitted_messages: number;
  truncated: boolean;
  compacted: boolean;
  sources: ContextSource[];
  omissions: ContextOmission[];
};

export type ContextBudget = {
  id: number;
  key: string;
  runId: number | null;
  createdAt: number;
  manifest: ContextManifest;
};

export const contextApi = {
  budgets: (conversationId: number) => invoke<ContextBudget[]>("context_budgets", { conversationId }),
};
