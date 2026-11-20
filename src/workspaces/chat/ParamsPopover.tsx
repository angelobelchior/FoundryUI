import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { SlidersIcon } from "../../components/Icon";
import { ParamsEditor } from "../../components/ParamsEditor";
import { Spinner } from "../../components/Spinner";
import { useParamsLayer } from "../../components/useParamsLayer";
import { cx } from "../../lib/cx";
import { layerFor, paramFields, type ParamField } from "../../lib/params";
import type { ChatParams } from "../../lib/chat";

type ParamsPopoverProps = {
  conversationId: number;
  modelAlias: string;
  supportedFields?: readonly ParamField[] | undefined;
  disabled?: boolean;
};

export function ParamsPopover({
  conversationId,
  modelAlias,
  supportedFields,
  disabled = false,
}: ParamsPopoverProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const { snapshot, busy, apply } = useParamsLayer({
    scope: "conversation",
    conversationId,
    modelAlias,
    enabled: true,
  });
  const layer = snapshot ? layerFor(snapshot, "conversation") : null;
  const visibleFields = supportedFields ?? paramFields;
  const hasVisibleOverride = Boolean(layer && visibleFields.some((field) => hasOverride(layer, field)));

  useEffect(() => {
    if (!open) {
      return;
    }

    function onPointerDown(event: PointerEvent) {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    }

    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setOpen(false);
      }
    }

    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);

    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        aria-label={t("params.title")}
        aria-expanded={open}
        aria-haspopup="dialog"
        title={t("params.title")}
        disabled={disabled}
        onClick={() => setOpen((current) => !current)}
        className={cx(
          "inline-flex h-7 max-w-[240px] items-center gap-1.5 rounded-[7px] border px-2 text-[11.5px] transition-colors",
          open || hasVisibleOverride
            ? "border-accent/45 bg-accent/10 text-primary hover:border-accent/70"
            : "border-line text-secondary hover:border-line-strong hover:text-primary",
          "disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-secondary",
        )}
      >
        <SlidersIcon size={14} />
        <span className="truncate font-medium">{t("params.title")}</span>
        {hasVisibleOverride ? <span className="h-1.5 w-1.5 rounded-full bg-accent" /> : null}
      </button>
      {open ? (
        <div
          role="dialog"
          aria-label={t("params.title")}
          className="pop-in absolute bottom-[calc(100%+4px)] right-0 z-30 w-[352px] rounded-lg border border-line bg-elevated p-3 shadow-[var(--shadow-popover)]"
        >
          <div className="mb-2 flex items-baseline justify-between gap-3">
            <p className="text-[12px] font-semibold text-primary">{t("params.title")}</p>
            <span className="truncate text-[10px] text-tertiary">{modelAlias}</span>
          </div>
          {snapshot ? (
            <ParamsEditor
              snapshot={snapshot}
              scope="conversation"
              fields={visibleFields}
              busy={busy}
              onSave={(params) => void apply(params)}
            />
          ) : (
            <div className="flex justify-center py-6">
              <Spinner size={13} className="text-tertiary" />
            </div>
          )}
        </div>
      ) : null}
    </div>
  );
}

function hasOverride(params: ChatParams, field: ParamField): boolean {
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
