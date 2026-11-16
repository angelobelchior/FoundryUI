import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { ChatParams } from "../lib/chat";
import { cx } from "../lib/cx";
import {
  layerFor,
  paramFields,
  type ParamField,
  type ParamScope,
  type ParamsSnapshot,
} from "../lib/params";
import { RefreshIcon } from "./Icon";

type Draft = Record<ParamField, string>;

type ParamsEditorProps = {
  snapshot: ParamsSnapshot;
  scope: ParamScope;
  busy?: boolean;
  onSave: (params: ChatParams) => void;
};

export function ParamsEditor({ snapshot, scope, busy = false, onSave }: ParamsEditorProps) {
  const { t } = useTranslation();
  const layer = layerFor(snapshot, scope);
  const [draft, setDraft] = useState<Draft>(() => draftFrom(layer));
  const dirty = paramFields.some((field) => draft[field] !== draftFrom(layer)[field]);
  const invalid = paramFields.some((field) => isInvalid(field, draft[field]));

  useEffect(() => {
    setDraft(draftFrom(layer));
  }, [layer]);

  return (
    <div className="flex flex-col">
      {paramFields.map((field) => {
        const bounds = numericBounds(field);
        const overridden = isOverridden(layer, field);
        const origin = snapshot.resolved[field].origin;

        return (
          <div
            key={field}
            className="grid grid-cols-[1fr_148px_24px] items-center gap-2.5 border-b border-line py-2 last:border-b-0"
          >
            <div className="min-w-0">
              <label
                htmlFor={`params-${scope}-${field}`}
                className="block truncate text-[12px] font-medium text-primary"
              >
                {t(`params.fields.${field}`)}
              </label>
              <p className="mt-0.5 truncate text-[10.5px] text-tertiary">
                {t(`params.origin.${origin}`)}
              </p>
            </div>
            <input
              id={`params-${scope}-${field}`}
              type={bounds ? "number" : "text"}
              inputMode={bounds ? "decimal" : "text"}
              min={bounds?.min}
              max={bounds?.max}
              step={bounds?.step}
              value={draft[field]}
              placeholder={field === "stop" ? t("params.stopHint") : t("params.inherited")}
              aria-invalid={isInvalid(field, draft[field])}
              disabled={busy}
              onChange={(event) =>
                setDraft((current) => ({ ...current, [field]: event.target.value }))
              }
              className={cx(
                "h-7 w-full min-w-0 rounded-[7px] border bg-canvas px-2 text-[11.5px] text-primary outline-none transition-colors placeholder:text-tertiary focus:border-line-strong disabled:opacity-60",
                isInvalid(field, draft[field]) ? "border-danger" : "border-line",
              )}
            />
            <button
              type="button"
              onClick={() => onSave({ ...paramsFrom(draft, layer), [field]: undefined })}
              disabled={!overridden || busy}
              aria-label={t("params.reset")}
              title={t("params.reset")}
              className={cx(
                "inline-flex h-6 w-6 items-center justify-center rounded-md text-tertiary transition-colors",
                "hover:bg-hover hover:text-primary",
                "disabled:cursor-not-allowed disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-tertiary",
              )}
            >
              <RefreshIcon size={12} />
            </button>
          </div>
        );
      })}
      <div className="mt-2.5 flex items-center justify-between gap-3">
        <p className="max-w-[30ch] text-[10px] leading-snug text-tertiary">
          {t("params.cascade")}
        </p>
        <button
          type="button"
          onClick={() => onSave(paramsFrom(draft, layer))}
          disabled={busy || !dirty || invalid}
          className="inline-flex h-7 shrink-0 items-center rounded-[7px] bg-accent px-2.5 text-[11.5px] font-medium text-accent-contrast transition-opacity hover:bg-accent-hover disabled:opacity-40"
        >
          {busy ? t("params.applying") : t("params.apply")}
        </button>
      </div>
    </div>
  );
}

function draftFrom(params: ChatParams): Draft {
  return {
    temperature: params.temperature == null ? "" : String(params.temperature),
    topP: params.topP == null ? "" : String(params.topP),
    maxTokens: params.maxTokens == null ? "" : String(params.maxTokens),
    seed: params.seed == null ? "" : String(params.seed),
    stop: (params.stop ?? []).join(", "),
  };
}

function paramsFrom(draft: Draft, layer: ChatParams): ChatParams {
  return {
    ...layer,
    temperature: numberOrUndefined(draft.temperature),
    topP: numberOrUndefined(draft.topP),
    maxTokens: integerOrUndefined(draft.maxTokens),
    seed: integerOrUndefined(draft.seed),
    stop: stopList(draft.stop),
  };
}

function isInvalid(field: ParamField, value: string): boolean {
  const trimmed = value.trim();

  if (field === "stop" || trimmed === "") {
    return false;
  }

  const parsed = Number(trimmed);
  const bounds = numericBounds(field);

  if (Number.isNaN(parsed)) {
    return true;
  }

  return (
    (bounds?.min !== undefined && parsed < bounds.min) ||
    (bounds?.max !== undefined && parsed > bounds.max)
  );
}

function isOverridden(params: ChatParams, field: ParamField): boolean {
  return field === "stop" ? Boolean(params.stop?.length) : params[field] != null;
}

function numericBounds(
  field: ParamField,
): { min?: number; max?: number; step: number } | null {
  if (field === "temperature") {
    return { min: 0, max: 2, step: 0.1 };
  }

  if (field === "topP") {
    return { min: 0, max: 1, step: 0.05 };
  }

  if (field === "maxTokens") {
    return { min: 1, max: 1_000_000, step: 1 };
  }

  return field === "seed" ? { step: 1 } : null;
}

function numberOrUndefined(value: string): number | undefined {
  const trimmed = value.trim();
  const parsed = Number(trimmed);

  return trimmed === "" || Number.isNaN(parsed) ? undefined : parsed;
}

function integerOrUndefined(value: string): number | undefined {
  const parsed = numberOrUndefined(value);

  return parsed === undefined ? undefined : Math.trunc(parsed);
}

function stopList(value: string): string[] | undefined {
  const sequences = value
    .split(/[,\n]/)
    .map((entry) => entry.trim())
    .filter(Boolean);

  return sequences.length ? sequences : undefined;
}
