import { invoke } from "@tauri-apps/api/core";
import { detectLanguage } from "./language";

export type AuthorizationStatus = "ready" | "pending" | "denied" | "claimed" | "invalidated";
export type RequirementKind =
  | "operation"
  | "standalone_root"
  | "external_resource"
  | "destructive_confirmation"
  | "alteration_review"
  | "network_destination";

export type AuthorizationRequirement = {
  kind: RequirementKind;
  target: string;
  constraintsFingerprint: string;
  rememberEligible: boolean;
  recoveryGuaranteed: boolean;
  previewAvailable: boolean;
};

export type AuthorizationRevision = {
  conversationId: number;
  contextRevision: number;
  authorizationRevision: number;
  modeRevision: number;
  mode: "review" | "automatic";
  projectBound: boolean;
  globalReadAllowed: boolean;
};

export type AuthorizationRequest = {
  id: number;
  conversationId: number;
  actionId: number;
  runId: number;
  contextRevision: number;
  authorizationRevision: number;
  modeRevision: number;
  status: AuthorizationStatus;
  requirements: AuthorizationRequirement[];
  remembered: boolean;
};

export type AuthorizationGrant = {
  id: number;
  conversationId: number;
  contextRevision: number;
  requirement: AuthorizationRequirement;
};

export type AuthorizationDecision = { decision: "approve"; remember: boolean } | { decision: "deny" };
export type DecisionReceipt = {
  requestId: number;
  status: AuthorizationStatus;
  grantId: number | null;
  dispatchStarted: false;
};

export type RevocationReceipt = {
  authorizationRevision: number;
  startedOrUncertainActionIds: number[];
  automaticallyCancelled: false;
};

export const authorizationApi = {
  revision: (conversationId: number) =>
    invoke<AuthorizationRevision>("authorization_revision", { conversationId }),
  requests: (conversationId: number) =>
    invoke<AuthorizationRequest[]>("authorization_requests", { conversationId }),
  grants: (conversationId: number) =>
    invoke<AuthorizationGrant[]>("authorization_grants", { conversationId }),
  revoke: (conversationId: number, grantId: number) =>
    invoke<RevocationReceipt>("authorization_revoke", { conversationId, grantId }),
  pickRoot: (conversationId: number) =>
    invoke<AuthorizationGrant | null>("authorization_pick_root", {
      conversationId,
      language: detectLanguage(),
    }),
  decide: (conversationId: number, requestId: number, decision: AuthorizationDecision) =>
    invoke<DecisionReceipt | null>("authorization_decide", {
      conversationId,
      requestId,
      decision,
      language: detectLanguage(),
    }),
};
