import type { ReactNode } from "react";
import { dragWindowFromMouseDown } from "../lib/drag";

type DetailPaneProps = {
  title: string;
  subtitle?: string;
  actions?: ReactNode;
  children: ReactNode;
};

export function DetailPane({ title, subtitle, actions, children }: DetailPaneProps) {
  return (
    <section className="flex h-full min-h-0 min-w-0 flex-col bg-canvas">
      <header
        className="flex h-[var(--titlebar-inset)] shrink-0 items-center justify-between gap-3 border-b border-line bg-canvas px-[var(--pane-header-pad-x)]"
        data-tauri-drag-region
        onMouseDown={dragWindowFromMouseDown}
      >
        <div className="flex min-w-0 items-baseline gap-2 sm:gap-2.5" data-tauri-drag-region>
          <h1 className="truncate text-[15px] font-semibold tracking-[-0.015em] text-primary">{title}</h1>
          {subtitle ? <span className="truncate text-[11.5px] text-tertiary">{subtitle}</span> : null}
        </div>
        {actions ? <div className="flex min-w-0 items-center justify-end gap-1">{actions}</div> : null}
      </header>
      <div className="detail-enter min-h-0 flex-1 overflow-y-auto">{children}</div>
    </section>
  );
}
