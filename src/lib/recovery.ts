import { invoke } from "@tauri-apps/api/core";

export type RecoverableChange = {
  runId: number | null;
  actionId: number;
  operation: "create" | "edit" | "delete";
  path: string;
  protected: boolean;
  destructive: boolean;
  additions: number;
  deletions: number;
  diff: string;
  diffTruncated: boolean;
};

export type RecoveryOutcome = {
  actionId: number;
  operation: string;
  path: string;
  restored: boolean;
};

export type RecoveryCommandError = {
  code: string;
  message: string;
};

export const recoveryApi = {
  list: (conversationId: number, runId: number | null) =>
    invoke<RecoverableChange[]>("recovery_list", { conversationId, runId }),
  undo: (conversationId: number, actionId: number, confirmedDestructive: boolean) =>
    invoke<RecoveryOutcome>("recovery_undo", { conversationId, actionId, confirmedDestructive }),
};
