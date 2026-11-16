import type { ReactNode } from "react";
import { cx } from "../lib/cx";
import { Tooltip } from "./Tooltip";

type IconButtonProps = {
  label: string;
  onClick?: () => void;
  disabled?: boolean;
  children: ReactNode;
  className?: string;
};

export function IconButton({ label, onClick, disabled = false, children, className }: IconButtonProps) {
  return (
    <Tooltip label={label}>
      <button
        type="button"
        aria-label={label}
        onClick={onClick}
        disabled={disabled}
        className={cx(
          "inline-flex h-[var(--control-size)] w-[var(--control-size)] items-center justify-center rounded-[var(--item-radius)] border border-transparent text-secondary transition-colors",
          "hover:border-line hover:bg-hover hover:text-primary active:border-line-strong active:bg-active",
          "disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-secondary",
          className,
        )}
      >
        {children}
      </button>
    </Tooltip>
  );
}
