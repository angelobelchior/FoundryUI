import { invoke } from "@tauri-apps/api/core";
import type { ChatParams } from "./chat";

export const paramScopes = ["global", "model", "conversation"] as const;
export type ParamScope = (typeof paramScopes)[number];

export const paramFields = ["temperature", "topP", "maxTokens", "seed", "stop"] as const;
export type ParamField = (typeof paramFields)[number];

export const paramOrigins = ["conversation", "model", "global", "default"] as const;
export type ParamOrigin = (typeof paramOrigins)[number];

export type Resolved<T> = {
  value: T | null;
  origin: ParamOrigin;
};

export type ResolvedParams = {
  temperature: Resolved<number>;
  topP: Resolved<number>;
  maxTokens: Resolved<number>;
  seed: Resolved<number>;
  stop: Resolved<string[]>;
};

export type ParamsSnapshot = {
  global: ChatParams;
  model: ChatParams;
  conversation: ChatParams;
  resolved: ResolvedParams;
};

export function layerFor(snapshot: ParamsSnapshot, scope: ParamScope): ChatParams {
  if (scope === "global") {
    return snapshot.global;
  }

  return scope === "model" ? snapshot.model : snapshot.conversation;
}

export async function paramsSnapshot(
  conversationId: number | null,
  modelAlias: string | null,
): Promise<ParamsSnapshot> {
  return invoke("params_snapshot", { conversationId, modelAlias });
}

export async function paramsSetLayer(input: {
  scope: ParamScope;
  params: ChatParams;
  conversationId?: number | null;
  modelAlias?: string | null;
}): Promise<ParamsSnapshot> {
  return invoke("params_set_layer", {
    scope: input.scope,
    params: input.params,
    conversationId: input.conversationId ?? null,
    modelAlias: input.modelAlias ?? null,
  });
}
