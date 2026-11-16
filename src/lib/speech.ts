import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { readStored, writeStored } from "./storage";

export const maxAudioBytes = 25 * 1024 * 1024;
export const acceptedAudioExtensions = ["mp3", "wav", "flac", "ogg", "webm"] as const;
const speechModelPreferenceKey = "foundryui.speech.model";
const speechInputPreferenceKey = "foundryui.speech.input";

export type AudioInfo = {
  path: string;
  name: string;
  mime: string;
  bytes: number;
  durationMs: number;
};

export type Transcription = {
  id: number;
  modelAlias: string;
  sourcePath: string;
  language: string | null;
  params: {
    requestedLanguage: string | null;
    temperature: number;
  } | null;
  text: string;
  durationMs: number;
  audioExists: boolean;
  createdAt: number;
  favorite: boolean;
  archived: boolean;
};

export type TranscriptionResult = {
  transcription: Transcription;
  autoLoaded: boolean;
};

export type SpeechStorageSettings = {
  directory: string;
  defaultDirectory: string;
  custom: boolean;
};

export type TranscriptionDeleteMode = "all" | "transcription" | "audio";

export type TranscriptionDeleteResult = {
  removed: boolean;
  transcription: Transcription | null;
};

export type MicrophonePermission = "granted" | "denied" | "restricted" | "undetermined" | "unknown";

export type AudioInputDevice = {
  id: string;
  name: string;
  isDefault: boolean;
};

export const languageOptions = [
  "en",
  "pt",
  "es",
  "fr",
  "de",
  "it",
  "ja",
  "zh",
  "ru",
] as const;

export async function pickAudioFile(): Promise<string | null> {
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [
      {
        name: "Audio",
        extensions: [...acceptedAudioExtensions],
      },
    ],
  });

  return typeof selected === "string" ? selected : null;
}

export async function importAudio(path: string): Promise<AudioInfo> {
  return invoke("speech_import", { path });
}

export async function speechStorageSettings(): Promise<SpeechStorageSettings> {
  return invoke("speech_get_storage_settings");
}

export async function setSpeechStorageSettings(directory: string | null): Promise<SpeechStorageSettings> {
  return invoke("speech_set_storage_settings", { directory });
}

export async function openSpeechStorageFolder(): Promise<void> {
  return invoke("speech_open_storage_folder");
}

export async function listAudioInputDevices(): Promise<AudioInputDevice[]> {
  return invoke("speech_list_input_devices");
}

export async function startRecording(device?: string | null): Promise<number> {
  return invoke("speech_record_start", { device: device ?? null });
}

export async function recordingLevel(): Promise<number> {
  return invoke("speech_record_level");
}

export async function stopRecording(): Promise<AudioInfo> {
  return invoke("speech_record_stop");
}

export async function cancelRecording(): Promise<void> {
  return invoke("speech_record_cancel");
}

export async function discardAudio(path: string): Promise<void> {
  return invoke("speech_discard_audio", { path });
}

export async function audioData(path: string): Promise<string> {
  return invoke("speech_audio_data", { path });
}

export async function microphonePermission(): Promise<MicrophonePermission> {
  return invoke("speech_microphone_permission");
}

export async function openMicrophoneSettings(): Promise<void> {
  return invoke("speech_open_microphone_settings");
}

export async function transcribe(input: {
  path: string;
  modelAlias: string;
  language?: string | null;
  temperature?: number;
}): Promise<TranscriptionResult> {
  return invoke("speech_transcribe", {
    path: input.path,
    modelAlias: input.modelAlias,
    language: input.language ?? null,
    temperature: input.temperature ?? 0,
  });
}

export async function listTranscriptions(archived = false): Promise<Transcription[]> {
  return invoke("speech_list_transcriptions", { archived });
}

export async function setTranscriptionFavorite(
  id: number,
  favorite: boolean,
): Promise<Transcription> {
  return invoke("speech_set_transcription_favorite", { id, favorite });
}

export async function setTranscriptionArchived(
  id: number,
  archived: boolean,
): Promise<Transcription> {
  return invoke("speech_set_transcription_archived", { id, archived });
}

export async function updateTranscription(id: number, text: string): Promise<Transcription> {
  return invoke("speech_update_transcription", { id, text });
}

export async function deleteTranscription(
  id: number,
  mode: TranscriptionDeleteMode,
): Promise<TranscriptionDeleteResult> {
  return invoke("speech_delete_transcription", { id, mode });
}

export async function saveTranscriptionText(id: number): Promise<{ path: string; bytes: number }> {
  return invoke("speech_save_text", { id });
}

export async function sendToChat(input: {
  conversationId?: number | null;
  modelAlias?: string | null;
  text: string;
}): Promise<{ id: number; title: string; modelAlias: string; createdAt: number; updatedAt: number }> {
  return invoke("speech_send_to_chat", {
    conversationId: input.conversationId ?? null,
    modelAlias: input.modelAlias ?? null,
    text: input.text,
  });
}

export function fileName(path: string): string {
  const segments = path.split(/[\\/]/);

  return segments[segments.length - 1] || path;
}

export function audioRejection(name: string, bytes: number): "unsupported" | "tooLarge" | null {
  const extension = name.split(".").pop()?.toLowerCase() ?? "";

  if (!(acceptedAudioExtensions as readonly string[]).includes(extension)) {
    return "unsupported";
  }

  return bytes > maxAudioBytes ? "tooLarge" : null;
}

export function readSpeechModelPreference(): string | null {
  const value = readStored(speechModelPreferenceKey)?.trim();

  return value ? value : null;
}

export function writeSpeechModelPreference(alias: string | null) {
  const value = alias?.trim() ?? "";

  writeStored(speechModelPreferenceKey, value);
}

export function readSpeechInputPreference(): string | null {
  const value = readStored(speechInputPreferenceKey)?.trim();

  return value ? value : null;
}

export function writeSpeechInputPreference(id: string | null) {
  const value = id?.trim() ?? "";

  writeStored(speechInputPreferenceKey, value);
}
