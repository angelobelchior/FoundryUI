import { invoke } from "@tauri-apps/api/core";
import type { Conversation } from "./chat";
import { detectLanguage } from "./language";

export type ProjectAvailability =
  | "available"
  | "missing"
  | "changed"
  | "denied"
  | "unsupported";

export type ChatProject = {
  id: number;
  name: string;
  folderPath: string;
  bindingRevision: number;
  availability: ProjectAvailability;
};

export type ProjectCommandErrorCode =
  | "ProjectInvalidInput"
  | "ProjectNotFound"
  | "ProjectConflict"
  | "ProjectStaleContext"
  | "ProjectBusy"
  | "ProjectStorage"
  | "ProjectFolderUnavailable"
  | "ProjectInvalidSelection"
  | "ProjectDialogUnavailable"
  | "ProjectNotReady";

export type ProjectCommandFailure = {
  code: ProjectCommandErrorCode;
  messageKey: string;
  message: string;
  detail: string | null;
};

export type ProjectConversation = {
  conversation: Conversation;
  context: {
    conversationId: number;
    projectId: number | null;
    revision: number;
    mode: "review" | "automatic";
  };
};

export type ContextCycleResult = {
  contexts: ProjectConversation["context"][];
  removedProjectId: number | null;
  deletedConversationIds: number[];
};

export const projectApi = {
  pickCreate: (name: string | null = null) =>
    invoke<ChatProject | null>("projects_pick_create", { name }),
  list: () => invoke<ChatProject[]>("projects_list"),
  get: (id: number) => invoke<ChatProject>("projects_get", { id }),
  rename: (id: number, name: string) =>
    invoke<ChatProject>("projects_rename", { id, name }),
  pickRelocate: (id: number) =>
    invoke<ChatProject | null>("projects_pick_relocate", { id }),
  createConversation: (projectId: number | null, modelAlias: string) =>
    invoke<ProjectConversation>("projects_create_conversation", {
      projectId,
      modelAlias,
    }),
  listConversations: (projectId: number | null, archived = false) =>
    invoke<ProjectConversation[]>("projects_list_conversations", {
      projectId,
      archived,
    }),
  moveConversation: (conversationId: number, destinationProjectId: number | null) =>
    invoke<ContextCycleResult | null>("projects_move_conversation", {
      conversationId,
      destinationProjectId,
      language: detectLanguage(),
    }),
  remove: (id: number) =>
    invoke<ContextCycleResult | null>("projects_remove", {
      id,
      language: detectLanguage(),
    }),
};
