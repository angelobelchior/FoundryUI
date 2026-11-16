import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertIcon, LayersIcon, RefreshIcon } from "../../components/Icon";
import { IconButton } from "../../components/IconButton";
import { Modal } from "../../components/Modal";
import { Spinner } from "../../components/Spinner";
import type { FoundryCommandError, LoadedSummary } from "../../lib/foundry";
import { formatBytes, formatElapsed, formatSize } from "./format";

type LoadedPanelProps = {
  open: boolean;
  summary: LoadedSummary | null;
  loading: boolean;
  error: FoundryCommandError | null;
  busyAlias: string | null;
  onClose: () => void;
  onRefresh: () => void;
  onUnload: (alias: string) => void;
};

export function LoadedPanel({
  open,
  summary,
  loading,
  error,
  busyAlias,
  onClose,
  onRefresh,
  onUnload,
}: LoadedPanelProps) {
  const { t, i18n } = useTranslation();
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!open) {
      return;
    }

    setNow(Date.now());

    const timer = window.setInterval(() => setNow(Date.now()), 30_000);

    return () => window.clearInterval(timer);
  }, [open]);

  const overcommitted =
    summary !== null && summary.totalBytes > summary.availableMemoryBytes && summary.models.length > 0;

  return (
    <Modal open={open} title={t("models.loaded.title")} onClose={onClose} size="md">
      <div className="flex items-center justify-between gap-3">
        <p className="text-[11.5px] text-tertiary">
          {summary
            ? t("models.loaded.available", {
                size: formatBytes(summary.availableMemoryBytes, i18n.language),
              })
            : null}
        </p>
        <IconButton label={t("models.loaded.refresh")} onClick={onRefresh}>
          <RefreshIcon size={14} className={loading ? "animate-spin" : ""} />
        </IconButton>
      </div>

      {error ? (
        <div className="mt-3 flex items-start gap-2 rounded-[8px] border border-line bg-subtle px-3 py-2 text-[11.5px] text-secondary">
          <AlertIcon size={14} className="mt-0.5 shrink-0 text-danger" />
          <p className="min-w-0 flex-1 break-words">{error.message}</p>
        </div>
      ) : null}

      {summary && !summary.serverRunning ? (
        <p className="mt-3 rounded-[8px] bg-subtle px-3 py-2 text-[11.5px] text-secondary">
          {t("models.loaded.serverStopped")}
        </p>
      ) : null}

      {summary && summary.models.length > 0 ? (
        <>
          <dl className="mt-3 grid grid-cols-2 gap-px overflow-hidden rounded-[8px] border border-line bg-line">
            <div className="bg-canvas px-3 py-2.5">
              <dt className="text-[10px] font-medium uppercase tracking-[0.045em] text-tertiary">
                {t("models.loaded.total")}
              </dt>
              <dd className="mt-1 text-[14px] font-semibold text-primary">
                {formatBytes(summary.totalBytes, i18n.language)}
              </dd>
            </div>
            <div className="bg-canvas px-3 py-2.5">
              <dt className="text-[10px] font-medium uppercase tracking-[0.045em] text-tertiary">
                {t("models.fields.state")}
              </dt>
              <dd className="mt-1 text-[14px] font-semibold text-primary">
                {t("models.state.loaded")} · {summary.models.length}
              </dd>
            </div>
          </dl>

          {overcommitted ? (
            <div className="mt-2.5 flex items-start gap-2 rounded-[8px] bg-subtle px-3 py-2 text-[11.5px] text-secondary">
              <AlertIcon size={14} className="mt-0.5 shrink-0 text-danger" />
              <p className="min-w-0 flex-1">
                {t("models.loaded.warning", {
                  used: formatBytes(summary.totalBytes, i18n.language),
                  available: formatBytes(summary.availableMemoryBytes, i18n.language),
                })}
              </p>
            </div>
          ) : null}

          <ul className="mt-3 overflow-hidden rounded-[8px] border border-line">
            {summary.models.map((model) => {
              const busy = busyAlias === model.alias;

              return (
                <li
                  key={model.alias}
                  className="flex items-center gap-3 border-b border-line bg-canvas px-3 py-2.5 last:border-b-0"
                >
                  <LayersIcon size={15} className="shrink-0 text-accent" />
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-[12.5px] font-medium text-primary">{model.alias}</p>
                    <p className="mt-0.5 truncate text-[10.5px] tabular-nums text-tertiary">
                      {[
                        model.fileSizeMb === null
                          ? null
                          : formatSize(model.fileSizeMb, i18n.language),
                        t("models.loaded.since", {
                          duration: formatElapsed(Math.max(0, now - model.loadedSince), i18n.language),
                        }),
                      ]
                        .filter((value): value is string => Boolean(value))
                        .join(" · ")}
                    </p>
                  </div>
                  <button
                    type="button"
                    onClick={() => onUnload(model.alias)}
                    disabled={busy}
                    className="inline-flex h-6 shrink-0 items-center gap-1.5 rounded-[6px] border border-line bg-elevated px-2 text-[11px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary disabled:opacity-60"
                  >
                    {busy ? <Spinner size={10} /> : null}
                    {t("models.loaded.unload")}
                  </button>
                </li>
              );
            })}
          </ul>
        </>
      ) : null}

      {summary && summary.models.length === 0 && !loading ? (
        <div className="flex flex-col items-center px-4 py-10 text-center">
          <LayersIcon size={26} className="text-tertiary" />
          <p className="mt-3 text-[13px] font-medium text-secondary">
            {t("models.loaded.emptyTitle")}
          </p>
          <p className="mt-1 max-w-[42ch] text-[11.5px] leading-relaxed text-tertiary">
            {t("models.loaded.emptyBody")}
          </p>
        </div>
      ) : null}

      {loading && !summary ? (
        <div className="flex flex-col items-center justify-center gap-2.5 py-10 text-center text-[11.5px] text-tertiary">
          <Spinner size={14} />
          {t("models.loadingDetails")}
        </div>
      ) : null}
    </Modal>
  );
}
