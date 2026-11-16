import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertIcon, FolderIcon } from "../../components/Icon";
import { Modal } from "../../components/Modal";
import { Spinner } from "../../components/Spinner";
import { useToast } from "../../components/Toast";
import {
  cacheInfo,
  clearCache,
  removeModel,
  setCacheDirectory,
  toFoundryCommandError,
  type CacheInfo,
  type FoundryCommandError,
  type ModelSummary,
} from "../../lib/foundry";
import { ConfirmDialog } from "./ConfirmDialog";
import { errorLabel, formatBytes, formatSize } from "./format";

type CachePanelProps = {
  open: boolean;
  onClose: () => void;
  onChanged: () => void;
};

export function CachePanel({ open, onClose, onChanged }: CachePanelProps) {
  const { t, i18n } = useTranslation();
  const toast = useToast();
  const [info, setInfo] = useState<CacheInfo | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<FoundryCommandError | null>(null);
  const [busy, setBusy] = useState(false);
  const [removeTarget, setRemoveTarget] = useState<ModelSummary | null>(null);
  const [clearStep, setClearStep] = useState(0);
  const [changeOpen, setChangeOpen] = useState(false);
  const [path, setPath] = useState("");
  const [changeConfirm, setChangeConfirm] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);

    try {
      setInfo(await cacheInfo());
    } catch (next) {
      setError(toFoundryCommandError(next));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!open) {
      return;
    }

    setRemoveTarget(null);
    setClearStep(0);
    setChangeOpen(false);
    setChangeConfirm(false);
    void refresh();
  }, [open, refresh]);

  async function removeSelected() {
    if (!removeTarget) {
      return;
    }

    const target = removeTarget;

    setBusy(true);
    setError(null);

    try {
      const freed = await removeModel(target.alias);

      toast.push(
        t("models.cache.removed", {
          alias: target.alias,
          size: formatBytes(freed.freedBytes, i18n.language),
        }),
      );
      setRemoveTarget(null);
      await refresh();
      onChanged();
    } catch (next) {
      setError(toFoundryCommandError(next));
    } finally {
      setBusy(false);
    }
  }

  async function clearAll() {
    setBusy(true);
    setError(null);

    try {
      const freed = await clearCache();

      toast.push(
        t("models.cache.cleared", { size: formatBytes(freed.freedBytes, i18n.language) }),
      );
      setClearStep(0);
      await refresh();
      onChanged();
    } catch (next) {
      setError(toFoundryCommandError(next));
    } finally {
      setBusy(false);
    }
  }

  async function changeDirectory() {
    setBusy(true);
    setError(null);

    try {
      const next = await setCacheDirectory(path.trim());

      setInfo(next);
      setChangeConfirm(false);
      setChangeOpen(false);
      setPath("");
      toast.push(t("models.cache.changed"));
      onChanged();
    } catch (next) {
      setError(toFoundryCommandError(next));
    } finally {
      setBusy(false);
    }
  }

  const loadedCount = info?.models.filter((model) => model.loaded).length ?? 0;

  return (
    <>
      <Modal open={open} title={t("models.cache.title")} onClose={onClose} size="lg">
        {error ? (
          <div className="mb-3 flex items-start gap-2 rounded-[8px] border border-line bg-subtle px-3 py-2 text-[11.5px] text-secondary">
            <AlertIcon size={14} className="mt-0.5 shrink-0 text-danger" />
            <p className="min-w-0 flex-1 break-words">{errorLabel(error, t)}</p>
            {error.detail ? (
              <p className="hidden break-words font-mono text-[10px] text-tertiary sm:block">
                {error.detail}
              </p>
            ) : null}
          </div>
        ) : null}

        {info ? (
          <>
            <div className="rounded-[8px] border border-line px-3 py-2.5">
              <div className="flex items-center gap-2">
                <FolderIcon size={14} className="shrink-0 text-tertiary" />
                <p className="text-[10px] font-medium uppercase tracking-[0.045em] text-tertiary">
                  {t("models.cache.location")}
                </p>
                <span className="ml-auto rounded-full bg-subtle px-1.5 py-px text-[9.5px] font-medium text-tertiary">
                  {info.userSet ? t("models.cache.customBadge") : t("models.cache.defaultBadge")}
                </span>
              </div>
              <p className="mt-1 break-all font-mono text-[11px] text-secondary">{info.location}</p>
              <button
                type="button"
                onClick={() => setChangeOpen((current) => !current)}
                className="mt-2 h-6 rounded-[6px] border border-line px-2 text-[11px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary"
              >
                {t("models.cache.change")}
              </button>
              {changeOpen ? (
                <div className="mt-2.5 border-t border-line pt-2.5">
                  <p className="text-[11px] leading-relaxed text-tertiary">
                    {t("models.cache.changeHelp")}
                  </p>
                  <div className="mt-2 flex items-center gap-2">
                    <input
                      type="text"
                      value={path}
                      onChange={(event) => setPath(event.target.value)}
                      placeholder={t("models.cache.changePlaceholder")}
                      aria-label={t("models.cache.change")}
                      className="h-7 min-w-0 flex-1 rounded-[6px] border border-line bg-elevated px-2 font-mono text-[11px] text-primary outline-none placeholder:text-tertiary focus:border-accent"
                    />
                    <button
                      type="button"
                      onClick={() => setChangeConfirm(true)}
                      disabled={!path.trim() || busy}
                      className="inline-flex h-7 shrink-0 items-center rounded-[7px] bg-accent px-2.5 text-[11px] font-medium text-accent-contrast transition-colors hover:bg-accent-hover disabled:opacity-50"
                    >
                      {t("models.cache.change")}
                    </button>
                  </div>
                </div>
              ) : null}
            </div>

            <div className="mt-4 flex items-center justify-between gap-3">
              <div>
                <p className="text-[11px] font-semibold uppercase tracking-[0.055em] text-tertiary">
                  {t("models.cache.models")}
                </p>
                <p className="mt-0.5 text-[11.5px] tabular-nums text-secondary">
                  {t("models.cache.summary", {
                    count: info.models.length,
                    size: formatBytes(info.totalBytes, i18n.language),
                  })}
                </p>
              </div>
              <button
                type="button"
                onClick={() => setClearStep(1)}
                disabled={info.models.length === 0 || loadedCount > 0 || busy}
                title={
                  loadedCount > 0
                    ? t("models.cache.loadedBlocksClear")
                    : info.models.length === 0
                      ? t("models.cache.empty")
                      : undefined
                }
                className="h-7 shrink-0 rounded-[7px] border border-line px-2.5 text-[11px] font-medium text-danger transition-colors hover:bg-hover disabled:cursor-not-allowed disabled:text-tertiary disabled:opacity-70"
              >
                {t("models.cache.clear")}
              </button>
            </div>

            {info.models.length === 0 ? (
              <div className="mt-3 rounded-[8px] bg-subtle px-3 py-7 text-center">
                <p className="text-[12px] font-medium text-secondary">{t("models.cache.empty")}</p>
              </div>
            ) : (
              <ul className="mt-2 overflow-hidden rounded-[8px] border border-line">
                {info.models.map((model) => (
                  <li
                    key={model.id}
                    className="flex items-center gap-3 border-b border-line bg-canvas px-3 py-2 last:border-b-0"
                  >
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-[12px] font-medium text-primary">{model.alias}</p>
                      <p className="mt-0.5 truncate font-mono text-[10px] text-tertiary">
                        {model.id}
                      </p>
                    </div>
                    <span className="shrink-0 text-[10.5px] tabular-nums text-tertiary">
                      {model.fileSizeMb === null ? null : formatSize(model.fileSizeMb, i18n.language)}
                    </span>
                    <button
                      type="button"
                      onClick={() => setRemoveTarget(model)}
                      disabled={model.loaded || busy}
                      title={model.loaded ? t("models.cache.loadedHint") : undefined}
                      className="h-6 shrink-0 rounded-[6px] border border-line px-2 text-[11px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary disabled:cursor-not-allowed disabled:opacity-50"
                    >
                      {t("models.cache.remove")}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </>
        ) : null}

        {loading && !info ? (
          <div className="flex flex-col items-center justify-center gap-2.5 py-10 text-center text-[11.5px] text-tertiary">
            <Spinner size={14} />
            {t("models.loadingDetails")}
          </div>
        ) : null}
      </Modal>

      <ConfirmDialog
        open={removeTarget !== null}
        title={t("models.cache.removeTitle", { alias: removeTarget?.alias ?? "" })}
        body={t("models.cache.removeBody", {
          size: formatSize(removeTarget?.fileSizeMb ?? 0, i18n.language),
        })}
        confirmLabel={t("models.cache.remove")}
        tone="danger"
        busy={busy}
        onConfirm={() => void removeSelected()}
        onClose={() => setRemoveTarget(null)}
      />

      <ConfirmDialog
        open={clearStep === 1}
        title={t("models.cache.clearTitle")}
        body={t("models.cache.clearBody", {
          count: info?.models.length ?? 0,
          size: formatBytes(info?.totalBytes ?? 0, i18n.language),
        })}
        confirmLabel={t("models.cache.clear")}
        tone="danger"
        onConfirm={() => setClearStep(2)}
        onClose={() => setClearStep(0)}
      />

      <ConfirmDialog
        open={clearStep === 2}
        title={t("models.cache.clearSecondTitle")}
        body={t("models.cache.clearSecondBody")}
        confirmLabel={t("models.cache.clear")}
        tone="danger"
        busy={busy}
        onConfirm={() => void clearAll()}
        onClose={() => setClearStep(0)}
      />

      <ConfirmDialog
        open={changeConfirm}
        title={t("models.cache.changeConfirmTitle")}
        body={t("models.cache.changeConfirmBody", { path: path.trim() })}
        confirmLabel={t("models.cache.change")}
        busy={busy}
        onConfirm={() => void changeDirectory()}
        onClose={() => setChangeConfirm(false)}
      />
    </>
  );
}
