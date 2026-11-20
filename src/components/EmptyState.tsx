import type { ReactNode } from "react";
import type { IconComponent } from "./Icon";

type EmptyStateProps = {
  icon: IconComponent;
  title: string;
  body: string;
  action?: ReactNode;
};

export function EmptyState({ icon: Icon, title, body, action }: EmptyStateProps) {
  return (
    <div className="flex h-full flex-col items-center justify-center px-6 py-10 text-center sm:px-8 sm:py-12">
      <Icon size={30} className="text-tertiary" />
      <h3 className="mt-3.5 text-[15px] font-semibold tracking-[-0.01em] text-primary">{title}</h3>
      <p className="mt-1.5 max-w-[44ch] text-[12.5px] leading-relaxed text-secondary">{body}</p>
      {action ? <div className="mt-4">{action}</div> : null}
    </div>
  );
}

export function ListEmpty({ icon: Icon, title, body }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center px-4 pt-9 text-center">
      <Icon size={22} className="text-tertiary" />
      <p className="mt-3 text-[12px] font-medium text-secondary">{title}</p>
      <p className="mt-1 max-w-[28ch] text-[11px] leading-relaxed text-tertiary">{body}</p>
    </div>
  );
}
