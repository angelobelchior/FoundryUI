import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export const foundryErrorCodes = [
  "CliMissing",
  "CliOutdated",
  "ServerUnreachable",
  "ModelNotCached",
  "ModelNotLoaded",
  "OutOfMemory",
  "Timeout",
  "Cancelled",
  "BadResponse",
  "CommandFailed",
  "Storage",
  "UnsupportedImage",
  "AttachmentRejected",
  "AudioRejected",
  "MicrophoneDenied",
  "RecordingFailed",
  "ExportFailed",
] as const;

export type FoundryErrorCode = (typeof foundryErrorCodes)[number];
export type FoundryErrorTranslationKey = `errors.${FoundryErrorCode}`;
export type ModelKind = "chat" | "multimodal" | "speech" | "embedding";
export type Origin = "live" | "curated" | "notDeclared";

export type Declared<T> = {
  value: T | null;
  origin: Origin;
  source: string | null;
};

export type ModelVariant = {
  id: string;
  displayName: string | null;
  device: string | null;
  executionProvider: string | null;
  fileSizeMb: number | null;
  cached: boolean;
};

export type ModelSummary = {
  alias: string;
  id: string;
  displayName: string | null;
  modelType: string | null;
  device: string | null;
  fileSizeMb: number | null;
  cached: boolean;
  loaded: boolean;
  source: string | null;
  license: string | null;
  publisher: string | null;
  supportsToolCalling: boolean;
  inputModalities: string[];
};

export type CliInfo = {
  path: string;
  version: string;
  minimumVersion: string;
};

export type ServerSnapshot = {
  endpoint: string | null;
  running: boolean;
  state: string | null;
  pid: number | null;
  startedByApp: boolean;
  logFile: string | null;
  port: number | null;
  uptime: string | null;
};

export type ServerPreferences = {
  port: number | null;
  keepRunning: boolean;
  connectTimeoutSecs: number;
  firstTokenTimeoutSecs: number;
  tokenIdleTimeoutSecs: number;
};

export type ConfigSetting = {
  key: string;
  value: unknown;
  userSet: boolean;
};

export type ConfigShow = {
  settings: ConfigSetting[];
};

export type JobStatus = "running" | "succeeded" | "failed" | "cancelled";

export type ModelJob = {
  alias: string;
  status: JobStatus;
  receivedBytes: number;
  totalBytes: number;
  bytesPerSecond: number;
  etaSeconds: number | null;
  startedAt: number;
  updatedAt: number;
  error: FoundryErrorPayload | null;
};

export type LoadedModel = {
  alias: string;
  id: string;
  displayName: string | null;
  modelType: string | null;
  device: string | null;
  fileSizeMb: number | null;
  loadedSince: number;
};

export type LoadedSummary = {
  models: LoadedModel[];
  totalBytes: number;
  availableMemoryBytes: number;
  totalMemoryBytes: number;
  serverRunning: boolean;
};

export type CacheInfo = {
  location: string;
  userSet: boolean;
  models: ModelSummary[];
  totalBytes: number;
};

export type FreedSpace = {
  alias: string | null;
  freedBytes: number;
};

export type RegisterModelInput = {
  path: string;
  modelId: string;
  task: string;
  displayName?: string | null;
  device?: string | null;
  executionProvider?: string | null;
  contextLength?: number | null;
  supportsToolCalling?: boolean | null;
  supportsReasoning?: boolean | null;
};

export const modelJobEvent = "foundry-model-job";

export type CatalogModel = {
  alias: string;
  id: string;
  displayName: string | null;
  family: string | null;
  kind: ModelKind;
  modelType: string | null;
  device: string | null;
  fileSizeMb: number | null;
  cached: boolean;
  loaded: boolean;
  supportsToolCalling: boolean;
  license: string | null;
  publisher: string | null;
  inputModalities: Declared<string[]>;
  outputModalities: Declared<string[]>;
  contextLength: Declared<number>;
  embeddingDimensions: Declared<number>;
  quantization: Declared<string>;
  overlaySource: string | null;
  variants: ModelVariant[];
};

export type FoundryErrorPayload = {
  code: string;
  messageKey: string;
  message: string;
  detail: string | null;
};

export class FoundryCommandError extends Error {
  readonly code: string;
  readonly detail: string | null;

  constructor(payload: FoundryErrorPayload) {
    super(payload.message);
    this.name = "FoundryCommandError";
    this.code = payload.code;
    this.detail = payload.detail;
  }
}

export async function catalogList(): Promise<CatalogModel[]> {
  return invoke("catalog_list");
}

export async function cliDetect(): Promise<CliInfo> {
  return invoke("cli_detect");
}

export async function startServer(): Promise<ServerSnapshot> {
  return invoke("server_start");
}

export async function serverStatus(): Promise<ServerSnapshot> {
  return invoke("server_status");
}

export async function stopServer(): Promise<ServerSnapshot> {
  return invoke("server_stop");
}

export async function restartServer(): Promise<ServerSnapshot> {
  return invoke("server_restart");
}

export async function serverLogs(lines = 200): Promise<string> {
  return invoke("server_logs", { lines });
}

export async function cliStatus(): Promise<unknown> {
  return invoke("cli_status");
}

export async function serverGetPreferences(): Promise<ServerPreferences> {
  return invoke("server_get_preferences");
}

export async function serverSetPreferences(preferences: ServerPreferences): Promise<ServerPreferences> {
  return invoke("server_set_preferences", { preferences });
}

export async function configShow(): Promise<ConfigShow> {
  return invoke("config_show");
}

export async function configSet(key: string, value: string): Promise<ConfigShow> {
  return invoke("config_set", { key, value });
}

export async function diagnosticsReport(): Promise<string> {
  return invoke("diagnostics_report");
}

export async function catalogModel(alias: string): Promise<CatalogModel> {
  return invoke("catalog_model", { alias });
}

export async function downloadModel(alias: string): Promise<ModelJob> {
  return invoke("model_download", { alias });
}

export async function cancelModelDownload(alias: string): Promise<boolean> {
  return invoke("model_download_cancel", { alias });
}

export async function listModelJobs(): Promise<ModelJob[]> {
  return invoke("model_jobs");
}

export async function loadedModels(): Promise<LoadedSummary> {
  return invoke("loaded_models");
}

export async function loadModel(alias: string): Promise<LoadedSummary> {
  return invoke("model_load", { alias });
}

export async function unloadModel(alias: string): Promise<LoadedSummary> {
  return invoke("model_unload", { alias });
}

export async function removeModel(alias: string): Promise<FreedSpace> {
  return invoke("model_remove", { alias });
}

export async function cacheInfo(): Promise<CacheInfo> {
  return invoke("cache_info");
}

export async function clearCache(): Promise<FreedSpace> {
  return invoke("cache_clear_all");
}

export async function setCacheDirectory(path: string): Promise<CacheInfo> {
  return invoke("cache_set_directory", { path });
}

export async function localModels(): Promise<ModelSummary[]> {
  return invoke("local_models");
}

export async function registerModel(input: RegisterModelInput): Promise<ModelSummary[]> {
  return invoke("model_register", { input });
}

export async function unregisterModel(model: string): Promise<ModelSummary[]> {
  return invoke("model_unregister", { model });
}

export function subscribeModelJobs(handler: (job: ModelJob) => void): Promise<UnlistenFn> {
  return listen<ModelJob>(modelJobEvent, (event) => handler(event.payload));
}

export function toFoundryCommandError(error: unknown): FoundryCommandError {
  if (error instanceof FoundryCommandError) {
    return error;
  }

  if (isFoundryErrorPayload(error)) {
    return new FoundryCommandError(error);
  }

  if (error instanceof Error) {
    return new FoundryCommandError({
      code: "CommandFailed",
      messageKey: "errors.CommandFailed",
      message: error.message,
      detail: null,
    });
  }

  return new FoundryCommandError({
    code: "CommandFailed",
    messageKey: "errors.CommandFailed",
    message: String(error),
    detail: null,
  });
}

export function isFoundryErrorCode(code: string): code is FoundryErrorCode {
  return (foundryErrorCodes as readonly string[]).includes(code);
}

function isFoundryErrorPayload(value: unknown): value is FoundryErrorPayload {
  if (typeof value !== "object" || value === null) {
    return false;
  }

  const payload = value as Partial<FoundryErrorPayload>;

  return typeof payload.code === "string" && typeof payload.message === "string";
}
