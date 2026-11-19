import { invoke } from "@tauri-apps/api/core";
import { detectLanguage } from "./language";

export type SnapshotOrigin = {
  conversationId: number;
  title: string;
  projectId: number | null;
  projectName: string | null;
};

export type SnapshotCandidate = {
  origin: SnapshotOrigin;
  archived: boolean;
  updatedAt: number;
};

export type SnapshotOmission =
  | { kind: "unsupported_attachment"; sourceSequence: number; mediaKind: string; mime: string }
  | { kind: "nested_conversation_copies"; count: number }
  | { kind: "private_context_facts"; count: number };

export type HistoricalContent =
  | { kind: "historical_text"; role: "user" | "assistant" | "divider"; content: string; reasoning: string }
  | { kind: "historical_tool_call"; sourceCallId: string; sourceTool: string; arguments: unknown }
  | { kind: "historical_tool_result"; sourceCallId: string; outcome: "succeeded" | "failed" | "cancelled"; summary: unknown }
  | { kind: "historical_state_event"; event: string; sourceActionId: number | null; detail: string | null };

export type ConversationSnapshot = {
  id: number;
  destinationConversationId: number;
  sourceConversationId: number;
  sourceTitle: string;
  capturedAt: number;
  readOnly: true;
  referenceOnly: true;
  contentAuthority: "historical_evidence";
  origin: SnapshotOrigin;
  crossedProjectBoundary: boolean;
  sourceMayBePartial: boolean;
  omittedAttachmentCount: number;
  omissions: SnapshotOmission[];
  entries: Array<{ sequence: number; sourceRunId: number | null; content: HistoricalContent }>;
};

export const snapshotApi = {
  candidates: (query: string | null = null, projectId: number | null = null) =>
    invoke<SnapshotCandidate[]>("snapshots_candidates", { query, projectId }),
  list: (destinationId: number) => invoke<ConversationSnapshot[]>("snapshots_list", { destinationId }),
  get: (destinationId: number, id: number) =>
    invoke<ConversationSnapshot>("snapshots_get", { destinationId, id }),
  capture: (sourceId: number, destinationId: number) =>
    invoke<ConversationSnapshot | null>("snapshots_capture", {
      sourceId,
      destinationId,
      language: detectLanguage(),
    }),
  remove: (destinationId: number, id: number) =>
    invoke<boolean>("snapshots_remove", { destinationId, id, language: detectLanguage() }),
};
