import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { SlidersIcon } from "../../components/Icon";
import { ParamsEditor } from "../../components/ParamsEditor";
import { Spinner } from "../../components/Spinner";
import { useParamsLayer } from "../../components/useParamsLayer";
import { cx } from "../../lib/cx";

type ParamsPopoverProps = {
  conversationId: number;
  modelAlias: string;
  disabled?: boolean;
};

export function ParamsPopover({
  conversationId,
  modelAlias,
  disabled = false,
}: ParamsPopoverProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const { snapshot, busy, apply } = useParamsLayer({
    scope: "conversation",
    conversationId,
    modelAlias,
    enabled: open,
  });

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
          "inline-flex h-7 w-7 items-center justify-center rounded-md text-secondary transition-colors",
          "hover:bg-hover hover:text-primary active:bg-active",
          "disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-secondary",
          open && "bg-active text-primary",
        )}
      >
        <SlidersIcon size={15} />
      </button>
      {open ? (
        <div
          role="dialog"
          aria-label={t("params.title")}
          className="pop-in absolute right-0 top-[calc(100%+4px)] z-30 w-[352px] rounded-lg border border-line bg-elevated p-3 shadow-[var(--shadow-popover)]"
        >
          <div className="mb-2 flex items-baseline justify-between gap-3">
            <p className="text-[12px] font-semibold text-primary">{t("params.title")}</p>
            <span className="truncate text-[10px] text-tertiary">{modelAlias}</span>
          </div>
          {snapshot ? (
            <ParamsEditor
              snapshot={snapshot}
              scope="conversation"
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
