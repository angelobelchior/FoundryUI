import en from "./locales/en.json";
import es from "./locales/es.json";
import ptBR from "./locales/pt-BR.json";
import { executionEnglish, executionPortuguese, executionSpanish } from "./execution";
import { accessEnglish, accessPortuguese, accessSpanish } from "./access";
import { permissionsEnglish, permissionsPortuguese, permissionsSpanish } from "./permissions";
import { contextEnglish, contextPortuguese, contextSpanish } from "./context";
import { snapshotsEnglish, snapshotsPortuguese, snapshotsSpanish } from "./snapshots";
import { recoveryEnglish, recoveryPortuguese, recoverySpanish } from "./recovery";
import { agentEnglish, agentPortuguese, agentSpanish } from "./agent";
import { editorEnglish, editorPortuguese, editorSpanish } from "./editor";

export const resources = {
  en: { translation: { ...en, execution: executionEnglish, access: accessEnglish, permissions: permissionsEnglish, context: contextEnglish, snapshots: snapshotsEnglish, recovery: recoveryEnglish, agent: agentEnglish, editor: editorEnglish } },
  "pt-BR": { translation: { ...ptBR, execution: executionPortuguese, access: accessPortuguese, permissions: permissionsPortuguese, context: contextPortuguese, snapshots: snapshotsPortuguese, recovery: recoveryPortuguese, agent: agentPortuguese, editor: editorPortuguese } },
  es: { translation: { ...es, execution: executionSpanish, access: accessSpanish, permissions: permissionsSpanish, context: contextSpanish, snapshots: snapshotsSpanish, recovery: recoverySpanish, agent: agentSpanish, editor: editorSpanish } },
};

export type TranslationSchema = typeof resources.en.translation;

export const languages = ["en", "pt-BR", "es"] as const;
export type Language = (typeof languages)[number];
