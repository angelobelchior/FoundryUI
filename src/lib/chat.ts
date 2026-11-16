import { Channel, invoke } from "@tauri-apps/api/core";
import type { FoundryErrorPayload } from "./foundry";
import type { ResolvedParams } from "./params";

export const chatMessageRoles = ["user", "assistant", "divider"] as const;
export type ChatMessageRole = (typeof chatMessageRoles)[number];

export const chatMessageStatuses = [
  "streaming",
  "complete",
  "cancelled",
  "error",
  "interrupted",
] as const;
export type ChatMessageStatus = (typeof chatMessageStatuses)[number];

export type Conversation = {
  id: number;
  title: string;
  modelAlias: string;
  createdAt: number;
  updatedAt: number;
  favorite: boolean;
  archived: boolean;
};

export type TokenUsage = {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
};

export type ChatAttachment = {
  id: number;
  messageId: number;
  kind: string;
  mime: string;
  path: string;
  bytes: number;
  width: number | null;
  height: number | null;
};

export type ChatMessage = {
  id: number;
  conversationId: number;
  seq: number;
  role: ChatMessageRole;
  content: string;
  reasoning: string;
  modelAlias: string | null;
  params: ResolvedParams | null;
  usage: TokenUsage | null;
  error: FoundryErrorPayload | null;
  status: ChatMessageStatus;
  attachments: ChatAttachment[];
  createdAt: number;
  updatedAt: number;
};

export type MessagePage = {
  messages: ChatMessage[];
  hasMore: boolean;
};

export type ModelChange = {
  conversation: Conversation;
  divider: ChatMessage | null;
};

export type ChatParams = {
  temperature?: number | null | undefined;
  topP?: number | null | undefined;
  maxTokens?: number | null | undefined;
  presencePenalty?: number | null | undefined;
  frequencyPenalty?: number | null | undefined;
  seed?: number | null | undefined;
  stop?: string[] | null | undefined;
};

export type StreamEvent =
  | {
      type: "started";
      requestId: string;
      conversation: Conversation;
      userMessage: ChatMessage;
      assistantMessage: ChatMessage;
    }
  | {
      type: "delta";
      requestId: string;
      messageId: number;
      content: string;
      reasoning: string;
    }
  | {
      type: "usage";
      requestId: string;
      messageId: number;
      usage: TokenUsage;
    }
  | {
      type: "done";
      requestId: string;
      message: ChatMessage;
      conversation: Conversation;
      cancelled: boolean;
    }
  | {
      type: "error";
      requestId: string;
      message: ChatMessage;
      conversation: Conversation;
      error: FoundryErrorPayload;
    };

export type SendReceipt = {
  requestId: string;
  conversation: Conversation;
  userMessage: ChatMessage;
  assistantMessage: ChatMessage;
  removed: number[];
};

export type SearchHit = {
  messageId: number;
  conversationId: number;
  conversationTitle: string;
  role: "user" | "assistant";
  modelAlias: string | null;
  seq: number;
  snippet: string;
  createdAt: number;
};

export type ExportFormat = "markdown" | "json";

export type ExportReceipt = {
  path: string;
  bytes: number;
};

export type ExportLabels = {
  untitled: string;
  user: string;
  assistant: string;
  thinking: string;
  model: string;
  messages: string;
  modelChanged: string;
  tokens: string;
  imageAttachmentsOne: string;
  imageAttachmentsOther: string;
};

export type SendMessageInput = {
  conversationId: number;
  content: string;
  attachments?: string[];
  onEvent: (event: StreamEvent) => void;
};

export async function listConversations(archived = false): Promise<Conversation[]> {
  return invoke("chat_list_conversations", { archived });
}

export async function createConversation(modelAlias: string): Promise<Conversation> {
  return invoke("chat_create_conversation", { modelAlias });
}

export async function renameConversation(id: number, title: string): Promise<Conversation> {
  return invoke("chat_rename_conversation", { id, title });
}

export async function deleteConversation(id: number): Promise<void> {
  return invoke("chat_delete_conversation", { id });
}

export async function setConversationArchived(
  id: number,
  archived: boolean,
): Promise<Conversation> {
  return invoke("chat_set_conversation_archived", { id, archived });
}

export async function setConversationFavorite(
  id: number,
  favorite: boolean,
): Promise<Conversation> {
  return invoke("chat_set_conversation_favorite", { id, favorite });
}

export async function setConversationModel(id: number, modelAlias: string): Promise<ModelChange> {
  return invoke("chat_set_model", { id, modelAlias });
}

export async function listMessages(
  conversationId: number,
  beforeSeq?: number | null,
  limit?: number,
): Promise<MessagePage> {
  return invoke("chat_list_messages", {
    conversationId,
    beforeSeq: beforeSeq ?? null,
    limit: limit ?? null,
  });
}

export async function conversationUsage(conversationId: number): Promise<TokenUsage> {
  return invoke("chat_conversation_usage", { conversationId });
}

export async function sendChatMessage(input: SendMessageInput): Promise<SendReceipt> {
  const channel = new Channel<StreamEvent>((event) => input.onEvent(event));

  return invoke("chat_send_message", {
    conversationId: input.conversationId,
    content: input.content,
    attachments: input.attachments ?? [],
    channel,
  });
}

export async function cancelGeneration(requestId: string): Promise<boolean> {
  return invoke("chat_cancel_generation", { requestId });
}

export async function searchMessages(input: {
  query: string;
  conversationId?: number | null;
  limit?: number;
}): Promise<SearchHit[]> {
  return invoke("chat_search", {
    query: input.query,
    conversationId: input.conversationId ?? null,
    limit: input.limit ?? null,
  });
}

export async function regenerateMessage(
  conversationId: number,
  onEvent: (event: StreamEvent) => void,
): Promise<SendReceipt> {
  const channel = new Channel<StreamEvent>((event) => onEvent(event));

  return invoke("chat_regenerate", { conversationId, channel });
}

export async function editChatMessage(
  messageId: number,
  content: string,
  onEvent: (event: StreamEvent) => void,
): Promise<SendReceipt> {
  const channel = new Channel<StreamEvent>((event) => onEvent(event));

  return invoke("chat_edit_message", { messageId, content, channel });
}

export async function exportConversation(
  conversationId: number,
  format: ExportFormat,
  labels: ExportLabels,
): Promise<ExportReceipt> {
  return invoke("chat_export", { conversationId, format, labels });
}

export function mergeConversation(
  current: Conversation[],
  conversation: Conversation,
): Conversation[] {
  const next = [conversation, ...current.filter((entry) => entry.id !== conversation.id)];

  return next.sort(
    (left, right) =>
      Number(right.favorite) - Number(left.favorite) ||
      right.updatedAt - left.updatedAt ||
      right.id - left.id,
  );
}

export function mergeMessages(current: ChatMessage[], incoming: ChatMessage[]): ChatMessage[] {
  const byId = new Map(current.map((message) => [message.id, message]));

  for (const message of incoming) {
    const existing = byId.get(message.id);

    byId.set(message.id, existing ? coalesceMessage(existing, message) : message);
  }

  return Array.from(byId.values()).sort((left, right) => left.seq - right.seq);
}

function coalesceMessage(current: ChatMessage, incoming: ChatMessage): ChatMessage {
  const terminal = incoming.status === "streaming" && current.status !== "streaming";

  return {
    ...incoming,
    content: incoming.content.length >= current.content.length ? incoming.content : current.content,
    reasoning:
      incoming.reasoning.length >= current.reasoning.length ? incoming.reasoning : current.reasoning,
    usage: incoming.usage ?? current.usage,
    error: incoming.error ?? current.error,
    status: terminal ? current.status : incoming.status,
  };
}
