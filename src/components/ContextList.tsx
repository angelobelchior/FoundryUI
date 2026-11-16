import { createContext, useContext, type ReactNode } from "react";
import { createPortal } from "react-dom";

export const ContextListTarget = createContext<HTMLElement | null>(null);

type ContextListProps = {
  title: string;
  action?: ReactNode;
  toolbar?: ReactNode;
  children: ReactNode;
};

export function ContextList({ title, action, toolbar, children }: ContextListProps) {
  const target = useContext(ContextListTarget);

  if (!target) {
    return null;
  }

  return createPortal(
    <section aria-label={title} className="flex h-full min-h-0 min-w-0 flex-col">
      <header className="flex shrink-0 flex-wrap items-center justify-between gap-2 border-b border-line px-[var(--sidebar-header-pad-x)] py-2.5">
        <h2 className="text-[14px] font-semibold text-primary">{title}</h2>
        {action}
      </header>
      {toolbar ? (
        <div className="shrink-0 border-b border-line px-[var(--sidebar-content-pad-x)] py-2.5">
          {toolbar}
        </div>
      ) : null}
      <div className="min-h-0 flex-1 overflow-y-auto px-[var(--sidebar-content-pad-x)] py-[var(--sidebar-content-pad-y)]">
        {children}
      </div>
    </section>,
    target,
  );
}
