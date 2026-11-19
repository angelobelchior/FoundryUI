import { invoke } from "@tauri-apps/api/core";
import { detectLanguage } from "./language";

export type InstructionTarget = { rootId: string; path: string };
export type InstructionSource = {
  rootId: string;
  path: string;
  identityKey: string;
  versionKey: string;
  contentFingerprint: string;
  content: string;
  status: "pending" | "approved" | "denied";
  previewAvailable: boolean;
};
export type InstructionDiscovery = {
  conversationId: number;
  contextRevision: number;
  authorizationRevision: number;
  modeRevision: number;
  sources: InstructionSource[];
  omissions: { rootId: string; path: string | null; reason: "unavailable" | "blocked" | "too_large" | "unsupported_text" | "limit_reached" }[];
};

export const instructionApi = {
  discover: (conversationId: number, targets: InstructionTarget[] = []) => invoke<InstructionDiscovery>("instructions_discover", { conversationId, targets }),
  decide: (discovery: InstructionDiscovery, source: InstructionSource, approve: boolean) => invoke<InstructionSource | null>("instructions_decide", {
    input: {
      conversationId: discovery.conversationId,
      contextRevision: discovery.contextRevision,
      authorizationRevision: discovery.authorizationRevision,
      modeRevision: discovery.modeRevision,
      rootId: source.rootId,
      path: source.path,
      identityKey: source.identityKey,
      versionKey: source.versionKey,
      contentFingerprint: source.contentFingerprint,
      approve,
    },
    language: detectLanguage(),
  }),
};
