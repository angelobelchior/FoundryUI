import { invoke } from "@tauri-apps/api/core";

export const maxImagesPerMessage = 4;
export const maxImageBytes = 5 * 1024 * 1024;
export const acceptedImageTypes = ["image/png", "image/jpeg", "image/gif", "image/webp"] as const;

export type StagedAttachment = {
  token: string;
  mime: string;
  bytes: number;
  width: number | null;
  height: number | null;
};

export type StagedImage = StagedAttachment & {
  previewUrl: string;
};

export async function stageAttachment(dataUrl: string): Promise<StagedAttachment> {
  return invoke("attachment_store", { dataUrl });
}

export async function discardAttachments(tokens: string[]): Promise<void> {
  await invoke("attachment_discard", { tokens });
}

export async function attachmentData(id: number): Promise<string> {
  return invoke("attachment_data", { id });
}

export function isAcceptedImage(file: File): boolean {
  return (acceptedImageTypes as readonly string[]).includes(file.type);
}

export type ImageRejection = "unsupported" | "tooLarge";

export function imageRejection(file: File): ImageRejection | null {
  if (!isAcceptedImage(file)) {
    return "unsupported";
  }

  return file.size > maxImageBytes ? "tooLarge" : null;
}

export function fileToDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();

    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error ?? new Error("could not read the image"));
    reader.readAsDataURL(file);
  });
}
