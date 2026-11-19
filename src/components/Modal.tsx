import { useEffect, useId, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { CloseIcon } from "./Icon";
import { IconButton } from "./IconButton";
import { cx } from "../lib/cx";

type ModalProps = {
  open: boolean;
  title: string;
  onClose: () => void;
  children: ReactNode;
  size?: "sm" | "md" | "lg";
  className?: string;
};

const focusableSelector =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function Modal({ open, title, onClose, children, size = "sm", className }: ModalProps) {
  const { t } = useTranslation();
  const titleId = useId();
  const dialogRef = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);

  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    if (!open) {
      return;
    }

    const previouslyFocused = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialogRef.current?.focus();

    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        onCloseRef.current();
        return;
      }

      if (event.key !== "Tab") {
        return;
      }

      const node = dialogRef.current;

      if (!node) {
        return;
      }

      const focusable = Array.from(node.querySelectorAll<HTMLElement>(focusableSelector));
      const first = focusable.at(0);
      const last = focusable.at(-1);

      if (!first || !last) {
        return;
      }

      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }

    document.addEventListener("keydown", onKeyDown);

    return () => {
      document.removeEventListener("keydown", onKeyDown);
      previouslyFocused?.focus();
    };
  }, [open]);

  if (!open) {
    return null;
  }

  return createPortal(
    <div
      className="fade-in fixed inset-0 z-50 flex items-center justify-center bg-[rgba(0,0,0,0.26)] p-3 sm:p-6"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        className={cx(
          "pop-in flex max-h-[calc(100vh-24px)] w-full flex-col rounded-xl border border-line bg-elevated shadow-[var(--shadow-popover)] outline-none sm:max-h-[calc(100vh-48px)]",
          size === "lg" ? "max-w-[640px]" : size === "md" ? "max-w-[520px]" : "max-w-[420px]",
          className,
        )}
      >
        <header className="flex shrink-0 items-center justify-between border-b border-line py-2.5 pl-4 pr-2">
          <h2 id={titleId} className="truncate text-[13px] font-semibold text-primary">
            {title}
          </h2>
          <IconButton label={t("common.close")} onClick={onClose}>
            <CloseIcon size={16} />
          </IconButton>
        </header>
        <div className={cx("min-h-0 overflow-y-auto", size === "sm" ? "px-4 py-3" : "px-5 py-4")}>
          {children}
        </div>
      </div>
    </div>,
    document.body,
  );
}
