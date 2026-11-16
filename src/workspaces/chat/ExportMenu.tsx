import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { DownloadIcon } from "../../components/Icon";
import { Spinner } from "../../components/Spinner";
import type { ExportFormat } from "../../lib/chat";
import { cx } from "../../lib/cx";

type ExportMenuProps = {
  disabled?: boolean;
  busy?: boolean;
  onExport: (format: ExportFormat) => void;
};

const formats: ExportFormat[] = ["markdown", "json"];

export function ExportMenu({ disabled = false, busy = false, onExport }: ExportMenuProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);

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
        event.preventDefault();
        setOpen(false);
        triggerRef.current?.focus();
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
        ref={triggerRef}
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={t("chat.export.label")}
        title={t("chat.export.label")}
        disabled={disabled || busy}
        onClick={() => setOpen((current) => !current)}
        className={cx(
          "inline-flex h-7 w-7 items-center justify-center rounded-[7px] text-secondary transition-colors",
          "hover:bg-hover hover:text-primary",
          "disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-transparent disabled:hover:text-secondary",
        )}
      >
        {busy ? <Spinner size={12} /> : <DownloadIcon size={15} />}
      </button>
      {open ? (
        <div
          role="menu"
          aria-label={t("chat.export.label")}
          className="pop-in absolute right-0 top-[calc(100%+4px)] z-30 min-w-[180px] overflow-y-auto rounded-lg border border-line bg-elevated p-1 shadow-[var(--shadow-popover)]"
        >
          {formats.map((format) => (
            <button
              key={format}
              type="button"
              role="menuitem"
              onClick={() => {
                setOpen(false);
                onExport(format);
              }}
              className="flex h-8 w-full items-center rounded-md px-2 text-left text-[12px] text-secondary transition-colors hover:bg-hover hover:text-primary"
            >
              {t(`chat.export.${format}`)}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}
