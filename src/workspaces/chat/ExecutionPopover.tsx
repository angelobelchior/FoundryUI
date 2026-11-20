import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ToolsIcon } from "../../components/Icon";
import { Spinner } from "../../components/Spinner";
import { cx } from "../../lib/cx";
import { AgentExecution } from "./AgentExecution";
import type { useAgentExecution } from "./useAgentExecution";

type ExecutionPopoverProps = {
  agent: ReturnType<typeof useAgentExecution>;
  compatible: boolean;
  hasAttachments: boolean;
};

export function ExecutionPopover({ agent, compatible, hasAttachments }: ExecutionPopoverProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const { status, busy, stopping } = agent;
  const pending = (status?.pendingRequests?.length ?? 0) > 0;
  const emphasized = open || pending || busy || stopping;

  useEffect(() => {
    if (!open) {
      return;
    }

    function onPointerDown(event: PointerEvent) {
      const target = event.target as Node | null;

      if (rootRef.current && !rootRef.current.contains(target)) {
        const insideModal =
          target instanceof Element && target.closest('[role="dialog"], [data-modal-placement]');
        if (!insideModal) {
          setOpen(false);
        }
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
        aria-label={t("execution.tools")}
        aria-expanded={open}
        aria-haspopup="dialog"
        title={t("execution.tools")}
        onClick={() => setOpen((current) => !current)}
        className={cx(
          "inline-flex h-7 items-center gap-1.5 rounded-[7px] border px-2 text-[11.5px] transition-colors",
          emphasized
            ? "border-accent/45 bg-accent/10 text-primary hover:border-accent/70"
            : "border-line text-secondary hover:border-line-strong hover:text-primary",
        )}
      >
        <ToolsIcon size={14} />
        <span className="truncate font-medium">{t("execution.tools")}</span>
        {busy || stopping ? <Spinner size={11} /> : pending ? <span className="h-1.5 w-1.5 rounded-full bg-accent" /> : null}
      </button>
      {open ? (
        <div className="pop-in absolute bottom-[calc(100%+4px)] right-0 z-30 flex h-[min(72vh,560px)] w-[min(94vw,640px)] flex-col overflow-hidden rounded-lg border border-line bg-elevated shadow-[var(--shadow-popover)]">
          <div className="flex shrink-0 items-center justify-between border-b border-line px-3 py-2">
            <p className="text-[12px] font-semibold text-primary">{t("execution.title")}</p>
          </div>
          <AgentExecution
            agent={agent}
            compatible={compatible}
            hasAttachments={hasAttachments}
          />
        </div>
      ) : null}
    </div>
  );
}
