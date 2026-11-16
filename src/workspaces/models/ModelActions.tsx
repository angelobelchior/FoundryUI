import { useTranslation } from "react-i18next";
import { AlertIcon, DownloadIcon, PlayIcon, StopIcon, TrashIcon } from "../../components/Icon";
import { Spinner } from "../../components/Spinner";
import { cx } from "../../lib/cx";
import type { CatalogModel, FoundryCommandError, ModelJob } from "../../lib/foundry";
import {
  errorLabel,
  formatSize,
} from "./format";

export type ModelActionsProps = {
  model: CatalogModel;
  job: ModelJob | undefined;
  busy: boolean;
  error: FoundryCommandError | null;
  onDownload: () => void;
  onCancelDownload: () => void;
  onLoad: () => void;
  onUnload: () => void;
  onRemove: () => void;
  onRetry: () => void;
};

export function ModelActions({
  model,
  job,
  busy,
  error,
  onDownload,
  onCancelDownload,
  onLoad,
  onUnload,
  onRemove,
  onRetry,
}: ModelActionsProps) {
  const { t, i18n } = useTranslation();
  const running = job?.status === "running";
  const retryable = job?.status === "failed" || job?.status === "cancelled";

  return (
    <div className="mt-4">
      {running ? (
        <div className="flex flex-wrap items-center gap-2" role="status" aria-live="polite">
          <button
            type="button"
            disabled
            className="inline-flex h-7 items-center gap-1.5 rounded-[7px] bg-accent px-3 text-[11.5px] font-medium text-accent-contrast opacity-60"
          >
            <Spinner size={11} />
            {t("models.jobs.downloading")}
          </button>
          <button
            type="button"
            onClick={onCancelDownload}
            className="inline-flex h-7 items-center rounded-[7px] border border-line px-2.5 text-[11.5px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary"
          >
            {t("models.jobs.cancel")}
          </button>
        </div>
      ) : (
        <div className="flex flex-wrap items-center gap-2">
          {!model.cached ? (
            <button
              type="button"
              onClick={onDownload}
              disabled={busy}
              className="inline-flex h-7 items-center gap-1.5 rounded-[7px] bg-accent px-3 text-[11.5px] font-medium text-accent-contrast transition-colors hover:bg-accent-hover disabled:opacity-60"
            >
              {busy ? <Spinner size={11} /> : <DownloadIcon size={13} />}
              {retryable ? t("models.actions.retryDownload") : t("models.actions.download")}
              {model.fileSizeMb !== null ? (
                <span className="font-normal text-accent-contrast/80">
                  {formatSize(model.fileSizeMb, i18n.language)}
                </span>
              ) : null}
            </button>
          ) : model.loaded ? (
            <button
              type="button"
              onClick={onUnload}
              disabled={busy}
              className="inline-flex h-7 items-center gap-1.5 rounded-[7px] border border-line bg-elevated px-3 text-[11.5px] font-medium text-primary transition-colors hover:bg-hover disabled:opacity-60"
            >
              {busy ? <Spinner size={11} /> : <StopIcon size={12} />}
              {t("models.actions.unload")}
            </button>
          ) : (
            <button
              type="button"
              onClick={onLoad}
              disabled={busy}
              className="inline-flex h-7 items-center gap-1.5 rounded-[7px] bg-accent px-3 text-[11.5px] font-medium text-accent-contrast transition-colors hover:bg-accent-hover disabled:opacity-60"
            >
              {busy ? <Spinner size={11} /> : <PlayIcon size={12} />}
              {t("models.actions.load")}
            </button>
          )}

          {model.cached ? (
            <button
              type="button"
              onClick={onRemove}
              disabled={busy || model.loaded}
              title={model.loaded ? t("models.actions.unloadFirst") : undefined}
              className={cx(
                "inline-flex h-7 items-center gap-1.5 rounded-[7px] border border-line px-2.5 text-[11.5px] font-medium transition-colors",
                model.loaded
                  ? "cursor-not-allowed text-tertiary opacity-60"
                  : "text-secondary hover:bg-hover hover:text-primary",
              )}
            >
              <TrashIcon size={13} />
              {t("models.actions.remove")}
            </button>
          ) : null}

          {job?.status === "cancelled" ? (
            <span className="text-[11px] text-tertiary">{t("models.actions.cancelled")}</span>
          ) : null}
        </div>
      )}

      {error ? (
        <div className="mt-2 flex items-start gap-2 rounded-[8px] border border-line bg-subtle px-2.5 py-2 text-[11.5px] text-secondary">
          <AlertIcon size={14} className="mt-0.5 shrink-0 text-danger" />
          <div className="min-w-0 flex-1">
            <p>{errorLabel(error, t)}</p>
            {error.detail ? (
              <p className="mt-0.5 break-words font-mono text-[10px] text-tertiary">
                {error.detail}
              </p>
            ) : null}
          </div>
          <button
            type="button"
            onClick={onRetry}
            className="h-6 shrink-0 rounded-[6px] border border-line bg-elevated px-2 text-[11px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary"
          >
            {t("models.retry")}
          </button>
        </div>
      ) : null}
    </div>
  );
}
