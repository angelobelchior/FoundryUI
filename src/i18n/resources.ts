import en from "./locales/en.json";
import es from "./locales/es.json";
import ptBR from "./locales/pt-BR.json";

export const resources = {
  en: { translation: en },
  "pt-BR": { translation: ptBR },
  es: { translation: es },
};

export type TranslationSchema = typeof en;

export const languages = ["en", "pt-BR", "es"] as const;
export type Language = (typeof languages)[number];
