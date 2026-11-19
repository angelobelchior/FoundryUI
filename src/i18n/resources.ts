import en from "./locales/en.json";
import es from "./locales/es.json";
import ptBR from "./locales/pt-BR.json";
import { executionEnglish, executionPortuguese, executionSpanish } from "./execution";

export const resources = {
  en: { translation: { ...en, execution: executionEnglish } },
  "pt-BR": { translation: { ...ptBR, execution: executionPortuguese } },
  es: { translation: { ...es, execution: executionSpanish } },
};

export type TranslationSchema = typeof resources.en.translation;

export const languages = ["en", "pt-BR", "es"] as const;
export type Language = (typeof languages)[number];
