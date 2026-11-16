import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Modal } from "../../components/Modal";
import { Spinner } from "../../components/Spinner";
import { cx } from "../../lib/cx";

type ConfirmDialogProps = {
  open: boolean;
  title: string;
  body: string;
  confirmLabel: string;
  tone?: "neutral" | "danger";
  busy?: boolean;
  confirmDisabled?: boolean;
  onConfirm: () => void;
  onClose: () => void;
  children?: ReactNode;
};

export function ConfirmDialog({
  open,
  title,
  body,
  confirmLabel,
  tone = "neutral",
  busy = false,
  confirmDisabled = false,
  onConfirm,
  onClose,
  children,
}: ConfirmDialogProps) {
  const { t } = useTranslation();

  return (
    <Modal open={open} title={title} onClose={onClose}>
      <p className="text-[12.5px] leading-relaxed text-secondary">{body}</p>
      {children}
      <div className="mt-5 flex flex-wrap justify-end gap-2">
        <button
          type="button"
          onClick={onClose}
          disabled={busy}
          className="inline-flex h-8 w-full items-center justify-center rounded-md border border-line px-3 text-[11.5px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary disabled:opacity-50 sm:w-auto"
        >
          {t("common.cancel")}
        </button>
        <button
          type="button"
          onClick={onConfirm}
          disabled={busy || confirmDisabled}
          className={cx(
            "inline-flex h-8 w-full items-center justify-center gap-1.5 rounded-md px-3 text-[11.5px] font-medium transition-colors disabled:opacity-60 sm:w-auto",
            tone === "danger"
              ? "bg-danger text-white hover:opacity-90"
              : "bg-accent text-accent-contrast hover:bg-accent-hover",
          )}
        >
          {busy ? <Spinner size={11} /> : null}
          {confirmLabel}
        </button>
      </div>
    </Modal>
  );
}
