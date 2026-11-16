import { languages, type Language } from "../i18n/resources";
import { readStored } from "./storage";

export const languageStorageKey = "foundryui.language";

export function normalizeLanguage(tag: string): Language {
  const normalized = tag.toLowerCase();

  if (normalized.startsWith("pt")) {
    return "pt-BR";
  }

  if (normalized.startsWith("es")) {
    return "es";
  }

  return "en";
}

export function detectLanguage(): Language {
  const stored = readStored(languageStorageKey);

  if (stored && (languages as readonly string[]).includes(stored)) {
    return stored as Language;
  }

  const preferred = navigator.languages?.[0] ?? navigator.language ?? "en";

  return normalizeLanguage(preferred);
}
