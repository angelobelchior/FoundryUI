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

export type ProjectSelection = ChatProject & { created: boolean };

export type ProjectFolderSelection = {
  selectionId: string;
  folderPath: string;
  defaultName: string;
  existingProject: ChatProject | null;
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
  pickFolder: () => invoke<ProjectFolderSelection | null>("projects_pick_folder"),
  confirmFolder: (selectionId: string, name: string) =>
    invoke<ProjectSelection>("projects_confirm_folder", { selectionId, name }),
  discardFolder: (selectionId: string) =>
    invoke<void>("projects_discard_folder", { selectionId }),
  pickCreate: (name: string | null = null) =>
    invoke<ProjectSelection | null>("projects_pick_create", { name }),
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
  conversationContext: (conversationId: number) =>
    invoke<ProjectConversation["context"]>("projects_conversation_context", {
      conversationId,
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
