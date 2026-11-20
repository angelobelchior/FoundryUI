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
import { InfoIcon, RefreshIcon } from "./Icon";

type Draft = Record<ParamField, string>;

const reasoningEffortOptions = ["low", "medium", "high"] as const;
const responseVerbosityOptions = ["short", "balanced", "detailed"] as const;

type ParamsEditorProps = {
  snapshot: ParamsSnapshot;
  scope: ParamScope;
  fields?: readonly ParamField[];
  busy?: boolean;
  onSave: (params: ChatParams) => void;
};

export function ParamsEditor({
  snapshot,
  scope,
  fields,
  busy = false,
  onSave,
}: ParamsEditorProps) {
  const { t } = useTranslation();
  const layer = layerFor(snapshot, scope);
  const visibleFields = fields ?? paramFields;
  const [draft, setDraft] = useState<Draft>(() => draftFrom(layer));
  const [openInfoField, setOpenInfoField] = useState<ParamField | null>(null);
  const baseline = draftFrom(layer);
  const dirty = visibleFields.some((field) => draft[field] !== baseline[field]);
  const invalid = visibleFields.some((field) => isInvalid(field, draft[field]));

  useEffect(() => {
    setDraft(draftFrom(layer));
  }, [layer]);

  useEffect(() => {
    if (!openInfoField) {
      return;
    }

    function onPointerDown(event: PointerEvent) {
      const target = event.target;

      if (target instanceof Element && target.closest("[data-param-info-root='true']")) {
        return;
      }

      setOpenInfoField(null);
    }

    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setOpenInfoField(null);
      }
    }

    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);

    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [openInfoField]);

  return (
    <div className="flex flex-col">
      {visibleFields.length === 0 ? (
        <p className="py-3 text-[11px] text-secondary">{t("params.notSupportedForModel")}</p>
      ) : null}
      {visibleFields.map((field) => {
        const bounds = numericBounds(field);
        const overridden = isOverridden(layer, field);
        const origin = snapshot.resolved[field].origin;

        return (
          <div
            key={field}
            className="grid grid-cols-[1fr_148px_24px] items-center gap-2.5 border-b border-line py-2 last:border-b-0"
          >
            <div className="min-w-0">
              <div data-param-info-root="true" className="relative flex items-center gap-1">
                <label
                  htmlFor={`params-${scope}-${field}`}
                  className="block min-w-0 flex-1 truncate text-[12px] font-medium text-primary"
                >
                  {t(`params.fields.${field}`)}
                </label>
                <button
                  type="button"
                  aria-label={t("params.fieldInfo", { field: t(`params.fields.${field}`) })}
                  title={t("params.fieldInfo", { field: t(`params.fields.${field}`) })}
                  onClick={() =>
                    setOpenInfoField((current) =>
                      current === field ? null : field,
                    )
                  }
                  className={cx(
                    "inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-md text-tertiary transition-colors",
                    "hover:bg-hover hover:text-primary",
                    openInfoField === field && "bg-active text-primary",
                  )}
                >
                  <InfoIcon size={12} />
                </button>
                {openInfoField === field ? (
                  <div
                    role="dialog"
                    aria-label={t("params.fieldInfo", { field: t(`params.fields.${field}`) })}
                    className="pop-in absolute left-0 top-[calc(100%+4px)] z-20 w-[min(300px,calc(100vw-2.5rem))] rounded-lg border border-line bg-elevated p-2.5 shadow-[var(--shadow-popover)]"
                  >
                    <p className="mb-1 text-[10px] font-semibold uppercase tracking-[0.05em] text-tertiary">
                      {t("params.infoTitle")}
                    </p>
                    <p className="text-[11px] leading-relaxed text-secondary">{t(`params.help.${field}`)}</p>
                  </div>
                ) : null}
              </div>
              <p className="mt-0.5 truncate text-[10.5px] text-tertiary">
                {t(`params.origin.${origin}`)}
              </p>
            </div>
            {isEnumField(field) ? (
              <select
                id={`params-${scope}-${field}`}
                value={draft[field]}
                disabled={busy}
                onChange={(event) =>
                  setDraft((current) => ({ ...current, [field]: event.target.value }))
                }
                className={cx(
                  "h-7 w-full min-w-0 rounded-[7px] border bg-canvas px-2 text-[11.5px] text-primary outline-none transition-colors focus:border-line-strong disabled:opacity-60",
                  isInvalid(field, draft[field]) ? "border-danger" : "border-line",
                )}
              >
                <option value="">{t("params.inherited")}</option>
                {enumOptions(field).map((value) => (
                  <option key={value} value={value}>
                    {t(`params.options.${field}.${value}` as never)}
                  </option>
                ))}
              </select>
            ) : (
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
            )}
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
      {visibleFields.length > 0 ? (
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
      ) : null}
    </div>
  );
}

function draftFrom(params: ChatParams): Draft {
  return {
    reasoningEffort: params.reasoningEffort ?? "",
    responseVerbosity: params.responseVerbosity ?? "",
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
    reasoningEffort: enumOrUndefined(draft.reasoningEffort, reasoningEffortOptions),
    responseVerbosity: enumOrUndefined(draft.responseVerbosity, responseVerbosityOptions),
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

  if (field === "reasoningEffort") {
    return !reasoningEffortOptions.includes(trimmed as (typeof reasoningEffortOptions)[number]);
  }

  if (field === "responseVerbosity") {
    return !responseVerbosityOptions.includes(
      trimmed as (typeof responseVerbosityOptions)[number],
    );
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
  if (field === "stop") {
    return Boolean(params.stop?.length);
  }

  if (field === "reasoningEffort") {
    return params.reasoningEffort != null;
  }

  if (field === "responseVerbosity") {
    return params.responseVerbosity != null;
  }

  return params[field] != null;
}

function numericBounds(
  field: ParamField,
): { min?: number; max?: number; step: number } | null {
  if (field === "reasoningEffort" || field === "responseVerbosity" || field === "stop") {
    return null;
  }

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

function enumOrUndefined<T extends readonly string[]>(
  value: string,
  options: T,
): T[number] | undefined {
  const normalized = value.trim();

  if (!normalized) {
    return undefined;
  }

  return options.includes(normalized as T[number]) ? (normalized as T[number]) : undefined;
}

function isEnumField(field: ParamField): field is "reasoningEffort" | "responseVerbosity" {
  return field === "reasoningEffort" || field === "responseVerbosity";
}

function enumOptions(field: "reasoningEffort" | "responseVerbosity") {
  return field === "reasoningEffort" ? reasoningEffortOptions : responseVerbosityOptions;
}
