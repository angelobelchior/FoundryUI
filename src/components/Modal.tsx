import { useEffect, useId, useRef, useState, type ReactNode } from "react";
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
  size?: "xs" | "sm" | "md" | "lg";
  placement?: "center" | "top-right" | "top-left";
  pointerOffset?: number;
  pointerTargetId?: string;
  className?: string;
};

const focusableSelector =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function Modal({
  open,
  title,
  onClose,
  children,
  size = "sm",
  placement = "center",
  pointerOffset,
  pointerTargetId,
  className,
}: ModalProps) {
  const { t } = useTranslation();
  const titleId = useId();
  const dialogRef = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  const [autoPointerOffset, setAutoPointerOffset] = useState<number | null>(null);

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

  useEffect(() => {
    if (!open || placement === "center" || pointerOffset !== undefined || !pointerTargetId) {
      setAutoPointerOffset(null);
      return;
    }

    const targetId = pointerTargetId;

    const dialog = dialogRef.current;
    const target = document.getElementById(targetId);

    if (!dialog || !target) {
      setAutoPointerOffset(null);
      return;
    }

    function updatePointer() {
      const nextDialog = dialogRef.current;
      const nextTarget = document.getElementById(targetId);

      if (!nextDialog || !nextTarget) {
        setAutoPointerOffset(null);
        return;
      }

      const targetRect = nextTarget.getBoundingClientRect();
      const dialogRect = nextDialog.getBoundingClientRect();
      const raw = targetRect.top + targetRect.height / 2 - dialogRect.top - 6;
      const min = 14;
      const max = Math.max(min, dialogRect.height - 20);
      const clamped = Math.min(max, Math.max(min, raw));

      setAutoPointerOffset(Math.round(clamped));
    }

    updatePointer();

    const observer = new ResizeObserver(() => updatePointer());

    observer.observe(dialog);
    observer.observe(target);
    window.addEventListener("resize", updatePointer);

    return () => {
      observer.disconnect();
      window.removeEventListener("resize", updatePointer);
    };
  }, [open, placement, pointerOffset, pointerTargetId]);

  if (!open) {
    return null;
  }

  return createPortal(
    <div
      className={cx(
        "fade-in fixed inset-0 z-50 flex bg-[rgba(0,0,0,0.26)] p-3 sm:p-6",
        placement === "top-right"
          ? "items-start justify-center md:justify-end md:pt-10"
          : placement === "top-left"
            ? "items-start justify-center md:justify-start md:pt-10"
            : "items-center justify-center",
      )}
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
      data-modal-placement={placement}
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        className={cx(
          "pop-in relative flex max-h-[calc(100vh-24px)] w-full flex-col rounded-xl border border-line bg-elevated shadow-[var(--shadow-popover)] outline-none sm:max-h-[calc(100vh-48px)]",
          size === "lg"
            ? "max-w-[640px]"
            : size === "md"
              ? "max-w-[520px]"
              : size === "sm"
                ? "max-w-[420px]"
                : "max-w-[380px]",
          className,
        )}
      >
        {placement === "top-right" ? (
          <div
            aria-hidden
            className="onboarding-pointer onboarding-pointer-left pointer-events-none absolute -left-1.5 top-9 hidden h-3 w-3 border border-line bg-elevated md:block"
            style={{ top: `${pointerOffset ?? autoPointerOffset ?? 36}px` }}
          />
        ) : placement === "top-left" ? (
          <div
            aria-hidden
            className="onboarding-pointer onboarding-pointer-right pointer-events-none absolute -right-1.5 top-9 hidden h-3 w-3 border border-line bg-elevated md:block"
            style={{ top: `${pointerOffset ?? autoPointerOffset ?? 36}px` }}
          />
        ) : null}
        <header className="flex shrink-0 items-center justify-between border-b border-line py-2.5 pl-4 pr-2">
          <h2 id={titleId} className="truncate text-[13px] font-semibold text-primary">
            {title}
          </h2>
          <IconButton label={t("common.close")} onClick={onClose}>
            <CloseIcon size={16} />
          </IconButton>
        </header>
        <div
          className={cx(
            "min-h-0 overflow-y-auto",
            size === "xs" ? "px-3.5 py-3" : size === "sm" ? "px-4 py-3" : "px-5 py-4",
          )}
        >
          {children}
        </div>
      </div>
    </div>,
    document.body,
  );
}
