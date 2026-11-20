import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { CheckIcon, ChevronDownIcon, DownloadIcon } from "./Icon";
import { Spinner } from "./Spinner";
import { cx } from "../lib/cx";
import { type CatalogModel, type ModelJob } from "../lib/foundry";

type ModelPickerProps = {
  label: string;
  models: readonly CatalogModel[];
  selectedAlias: string | null;
  disabled?: boolean;
  loadingAlias?: string | null;
  jobs?: Readonly<Record<string, ModelJob>>;
  placement?: "up" | "down";
  onOpenModels: (() => void) | undefined;
  onUse: (alias: string) => void;
  onLoad: (alias: string) => void;
  onDownload: (alias: string) => void;
};

export function ModelPicker({
  label,
  models,
  selectedAlias,
  disabled = false,
  loadingAlias = null,
  jobs,
  placement = "down",
  onOpenModels,
  onUse,
  onLoad,
  onDownload,
}: ModelPickerProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);

  const loadedModels = models.filter((model) => model.loaded);
  const loadableModels = models.filter((model) => model.cached && !model.loaded);
  const downloadableModels = models.filter((model) => !model.cached);
  const cachedCount = models.filter((model) => model.cached).length;
  const selectedModel = models.find(
    (model) => model.alias === selectedAlias || model.id === selectedAlias,
  );

  let status: "none" | "notSelected" | "notLoaded" | "ok" = "ok";

  if (cachedCount === 0) {
    status = "none";
  } else if (selectedAlias === null) {
    status = "notSelected";
  } else if (selectedModel && !selectedModel.loaded) {
    status = "notLoaded";
  }

  const valueText =
    status === "ok"
      ? (selectedModel?.displayName ?? selectedAlias ?? "")
      : status === "none"
        ? t("models.picker.noModel")
        : status === "notSelected"
          ? t("models.picker.notSelected")
          : t("models.picker.notLoaded");

  useEffect(() => {
    if (!open) {
      return;
    }

    function onPointerDown(event: PointerEvent) {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    }

    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        setOpen(false);
        triggerRef.current?.focus();
      }
    }

    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);

    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  function useModel(alias: string) {
    onUse(alias);
    setOpen(false);
    triggerRef.current?.focus();
  }

  function loadModelAlias(alias: string) {
    onLoad(alias);
    setOpen(false);
    triggerRef.current?.focus();
  }

  function downloadModelAlias(alias: string) {
    onDownload(alias);
    setOpen(false);
    triggerRef.current?.focus();
  }

  return (
    <div ref={rootRef} className="relative">
      <button
        ref={triggerRef}
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={label}
        disabled={disabled}
        onClick={() => setOpen((current) => !current)}
        className={cx(
          "inline-flex h-7 max-w-[260px] items-center gap-1.5 rounded-[7px] border px-2 text-[11.5px] transition-colors",
          "border-line text-secondary hover:border-line-strong hover:text-primary",
          "disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:border-line disabled:hover:text-secondary",
        )}
      >
        <span className="shrink-0 text-[10px] font-semibold uppercase tracking-[0.04em] text-tertiary">
          {label}
        </span>
        <span
          className={cx(
            "min-w-0 truncate",
            status === "ok" ? "font-medium text-primary" : "text-secondary",
          )}
        >
          {valueText}
        </span>
        <ChevronDownIcon
          size={12}
          className={cx("shrink-0 text-tertiary transition-transform duration-150", open && "rotate-180")}
        />
      </button>

      {open ? (
        <div
          role="menu"
          aria-label={label}
          className={cx(
            "pop-in absolute z-30 max-h-[300px] min-w-[220px] overflow-y-auto rounded-lg border border-line bg-elevated p-1 shadow-[var(--shadow-popover)]",
            placement === "up" ? "bottom-[calc(100%+4px)] right-0" : "left-0 top-[calc(100%+4px)]",
          )}
        >
          {loadedModels.length > 0 ? (
            <Section label={t("models.picker.useSection")}>
              {loadedModels.map((model) => {
                const isSelected = model.alias === selectedAlias || model.id === selectedAlias;

                return (
                  <button
                    key={model.alias}
                    type="button"
                    role="menuitemradio"
                    aria-checked={isSelected}
                    onClick={() => useModel(model.alias)}
                    className={cx(
                      "flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-[12px] transition-colors",
                      isSelected
                        ? "bg-subtle text-primary"
                        : "text-secondary hover:bg-hover hover:text-primary",
                    )}
                  >
                    <span className="flex w-4 shrink-0 justify-center">
                      {isSelected ? <CheckIcon size={13} /> : null}
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block truncate">{model.alias}</span>
                      {model.displayName ? (
                        <span className="block truncate text-[10px] text-tertiary">{model.displayName}</span>
                      ) : null}
                    </span>
                  </button>
                );
              })}
            </Section>
          ) : null}

          {loadableModels.length > 0 ? (
            <Section label={t("models.picker.loadSection")}>
              {loadableModels.map((model) => {
                const loading = loadingAlias === model.alias;

                return (
                  <button
                    key={model.alias}
                    type="button"
                    role="menuitem"
                    onClick={() => loadModelAlias(model.alias)}
                    disabled={loadingAlias !== null}
                    className="flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-[12px] text-secondary transition-colors hover:bg-hover hover:text-primary disabled:opacity-60"
                  >
                    <span className="flex w-4 shrink-0 justify-center">
                      {loading ? <Spinner size={11} /> : null}
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block truncate">{model.alias}</span>
                      {model.displayName ? (
                        <span className="block truncate text-[10px] text-tertiary">{model.displayName}</span>
                      ) : null}
                    </span>
                    <span className="shrink-0 text-[10px] uppercase tracking-[0.04em] text-tertiary">
                      {t("models.picker.load")}
                    </span>
                  </button>
                );
              })}
            </Section>
          ) : null}

          {downloadableModels.length > 0 ? (
            <Section label={t("models.picker.downloadSection")}>
              {downloadableModels.map((model) => {
                const job = jobs?.[model.alias];
                const running = job?.status === "running";

                return (
                  <button
                    key={model.alias}
                    type="button"
                    role="menuitem"
                    onClick={() => downloadModelAlias(model.alias)}
                    disabled={running}
                    className="flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-[12px] text-secondary transition-colors hover:bg-hover hover:text-primary disabled:opacity-60"
                  >
                    <span className="flex w-4 shrink-0 justify-center">
                      {running ? <Spinner size={11} /> : <DownloadIcon size={12} />}
                    </span>
                    <span className="min-w-0 flex-1">
                      <span className="block truncate">{model.alias}</span>
                      {model.displayName ? (
                        <span className="block truncate text-[10px] text-tertiary">{model.displayName}</span>
                      ) : null}
                    </span>
                    <span className="shrink-0 text-[10px] uppercase tracking-[0.04em] text-tertiary">
                      {running
                        ? t("models.picker.downloading")
                        : job?.status === "failed"
                          ? t("models.picker.retryDownload")
                          : t("models.picker.download")}
                    </span>
                  </button>
                );
              })}
            </Section>
          ) : null}

          {models.length === 0 ? (
            <p className="px-2 py-2 text-[11.5px] text-tertiary">{t("models.picker.empty")}</p>
          ) : null}

          {onOpenModels ? (
            <div className="mt-1 border-t border-line pt-1">
              <button
                type="button"
                role="menuitem"
                onClick={() => {
                  onOpenModels();
                  setOpen(false);
                }}
                className="flex h-8 w-full items-center rounded-md px-2 text-left text-[12px] text-secondary transition-colors hover:bg-hover hover:text-primary"
              >
                {t("common.openModels")}
              </button>
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function Section({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="pb-0.5">
      <p className="px-2 pb-1 pt-1.5 text-[9.5px] font-semibold uppercase tracking-[0.06em] text-tertiary">
        {label}
      </p>
      {children}
    </div>
  );
}
