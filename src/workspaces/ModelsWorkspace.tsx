import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { FilterMenu, type FilterOption } from "../components/FilterMenu";
import { ContextList } from "../components/ContextList";
import { DetailPane } from "../components/DetailPane";
import { EmptyState, ListEmpty } from "../components/EmptyState";
import {
  AlertIcon,
  CloseIcon,
  FolderIcon,
  InfoIcon,
  LayersIcon,
  ModelsIcon,
  PlusIcon,
  RefreshIcon,
  SearchIcon,
} from "../components/Icon";
import { IconButton } from "../components/IconButton";
import { useToast } from "../components/Toast";
import {
  cancelModelDownload,
  catalogList,
  catalogModel,
  downloadModel,
  FoundryCommandError,
  loadModel,
  loadedModels,
  removeModel,
  toFoundryCommandError,
  unloadModel,
  type CatalogModel,
  type JobStatus,
  type LoadedSummary,
  type ModelJob,
} from "../lib/foundry";
import { useModelJobStream } from "../lib/useModelJobStream";
import { CachePanel } from "./models/CachePanel";
import { ConfirmDialog } from "./models/ConfirmDialog";
import { LoadedPanel } from "./models/LoadedPanel";
import { LocalModelsPanel } from "./models/LocalModelsPanel";
import { ModelCard } from "./models/ModelCard";
import { ModelList } from "./models/ModelList";
import { ModelOverview } from "./models/ModelOverview";
import { CatalogError, CatalogLoading, CatalogSkeleton, DetailError } from "./models/ModelStates";
import { errorLabel, formatBytes, formatSize, kindLabel, sortModels } from "./models/format";

const kindFilters = ["all", "chat", "multimodal", "speech", "embedding"] as const;
type KindFilter = (typeof kindFilters)[number];
type StateFilter = "all" | "cached" | "loaded";
type ActionKind = "download" | "load" | "unload" | "remove";

export function ModelsWorkspace() {
  const { t, i18n } = useTranslation();
  const toast = useToast();
  const [models, setModels] = useState<CatalogModel[] | null>(null);
  const [listError, setListError] = useState<FoundryCommandError | null>(null);
  const [loading, setLoading] = useState(true);
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<KindFilter>("all");
  const [device, setDevice] = useState("all");
  const [stateFilter, setStateFilter] = useState<StateFilter>("all");
  const [selectedAlias, setSelectedAlias] = useState<string | null>(null);
  const [detail, setDetail] = useState<CatalogModel | null>(null);
  const [detailError, setDetailError] = useState<FoundryCommandError | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailNonce, setDetailNonce] = useState(0);
  const [cardOpen, setCardOpen] = useState(false);
  const [jobs, setJobs] = useState<Record<string, ModelJob>>({});
  const [errors, setErrors] = useState<Record<string, FoundryCommandError>>({});
  const [busyAlias, setBusyAlias] = useState<string | null>(null);
  const [removeTarget, setRemoveTarget] = useState<CatalogModel | null>(null);
  const [removing, setRemoving] = useState(false);
  const [loadedOpen, setLoadedOpen] = useState(false);
  const [loadedSummary, setLoadedSummary] = useState<LoadedSummary | null>(null);
  const [loadedLoading, setLoadedLoading] = useState(false);
  const [loadedError, setLoadedError] = useState<FoundryCommandError | null>(null);
  const [cacheOpen, setCacheOpen] = useState(false);
  const [localOpen, setLocalOpen] = useState(false);
  const searchRef = useRef<HTMLInputElement>(null);
  const jobStatusRef = useRef<Record<string, JobStatus>>({});
  const lastActionRef = useRef<Record<string, ActionKind>>({});

  const loadCatalog = useCallback(async () => {
    setLoading(true);
    setListError(null);

    try {
      const next = await catalogList();

      setModels(next);
      setSelectedAlias((current) => {
        if (current && next.some((model) => model.alias === current)) {
          return current;
        }

        return sortModels(next, i18n.language)[0]?.alias ?? null;
      });
    } catch (error) {
      setListError(toFoundryCommandError(error));
      setModels(null);
    } finally {
      setLoading(false);
    }
  }, [i18n.language]);

  const applyLoadedSummary = useCallback((summary: LoadedSummary) => {
    setLoadedSummary(summary);
    setLoadedError(null);

    const resident = new Set(summary.models.flatMap((model) => [model.alias, model.id]));

    setModels((current) =>
      current?.map((model) => ({
        ...model,
        loaded: resident.has(model.alias) || resident.has(model.id),
      })) ?? current,
    );
  }, []);

  const refreshLoaded = useCallback(async () => {
    setLoadedLoading(true);
    setLoadedError(null);

    try {
      applyLoadedSummary(await loadedModels());
    } catch (error) {
      setLoadedError(toFoundryCommandError(error));
    } finally {
      setLoadedLoading(false);
    }
  }, [applyLoadedSummary]);

  useEffect(() => {
    void loadCatalog();
  }, [loadCatalog]);

  useEffect(() => {
    void refreshLoaded();
  }, [refreshLoaded]);

  useModelJobStream((job, hydrated) => {
    setJobs((current) => {
      const known = current[job.alias];

      if (known && known.updatedAt > job.updatedAt) {
        return current;
      }

      if (hydrated && job.status !== "running") {
        jobStatusRef.current[job.alias] = job.status;
      }

      return { ...current, [job.alias]: job };
    });
  });

  useEffect(() => {
    for (const job of Object.values(jobs)) {
      const previous = jobStatusRef.current[job.alias];

      if (previous === job.status) {
        continue;
      }

      jobStatusRef.current[job.alias] = job.status;

      if (job.status === "succeeded") {
        toast.push(t("models.actions.downloaded", { alias: job.alias }));
        void loadCatalog();
        setDetailNonce((current) => current + 1);
      } else if (job.status === "cancelled") {
        toast.push(t("models.actions.cancelled"));
      } else if (job.status === "failed") {
        setErrors((current) => ({
          ...current,
          [job.alias]: new FoundryCommandError(
            job.error ?? { code: "CommandFailed", messageKey: "", message: "", detail: null },
          ),
        }));
      }
    }
  }, [jobs, t, toast, loadCatalog]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "f") {
        event.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
      }
    }

    window.addEventListener("keydown", onKeyDown);

    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  useEffect(() => {
    if (!selectedAlias) {
      setDetail(null);
      setDetailError(null);
      setDetailLoading(false);

      return;
    }

    let cancelled = false;

    setDetailLoading(true);
    setDetailError(null);

    catalogModel(selectedAlias)
      .then((next) => {
        if (!cancelled) {
          setDetail(next);
        }
      })
      .catch((error) => {
        if (!cancelled) {
          setDetail(null);
          setDetailError(toFoundryCommandError(error));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setDetailLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [selectedAlias, detailNonce]);

  const selectedSummary = useMemo(
    () => models?.find((model) => model.alias === selectedAlias) ?? null,
    [models, selectedAlias],
  );
  const paneModel = detail?.alias === selectedAlias ? detail : selectedSummary;

  const filtered = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase(i18n.language);

    return (models ?? []).filter((model) => {
      if (kind !== "all" && model.kind !== kind) {
        return false;
      }

      if (device !== "all" && (model.device ?? "").toLowerCase() !== device) {
        return false;
      }

      if (stateFilter === "cached" && !model.cached) {
        return false;
      }

      if (stateFilter === "loaded" && !model.loaded) {
        return false;
      }

      if (!needle) {
        return true;
      }

      const haystack = [model.alias, model.displayName, model.id, model.family]
        .filter((value): value is string => Boolean(value))
        .join(" ")
        .toLocaleLowerCase(i18n.language);

      return haystack.includes(needle);
    });
  }, [models, query, kind, device, stateFilter, i18n.language]);

  useEffect(() => {
    if (!models) {
      return;
    }

    if (selectedAlias && filtered.some((model) => model.alias === selectedAlias)) {
      return;
    }

    setSelectedAlias(filtered[0]?.alias ?? null);
  }, [filtered, models, selectedAlias]);

  const visibleModel = filtered.length > 0 ? paneModel : null;
  const subtitle = visibleModel
    ? [kindLabel(visibleModel.kind, t), visibleModel.family].filter(Boolean).join(" · ")
    : "";

  const kindOptions: FilterOption[] = useMemo(
    () => [
      { value: "all", label: t("models.kinds.all"), count: models?.length ?? 0 },
      ...kindFilters.slice(1).map((value) => ({
        value,
        label: t(`models.kinds.${value}`),
        count: models?.filter((model) => model.kind === value).length ?? 0,
      })),
    ],
    [models, t],
  );

  const deviceOptions: FilterOption[] = useMemo(() => {
    const counts = new Map<string, number>();

    for (const model of models ?? []) {
      const key = model.device?.toLowerCase();

      if (key) {
        counts.set(key, (counts.get(key) ?? 0) + 1);
      }
    }

    return [
      { value: "all", label: t("models.filters.all"), count: models?.length ?? 0 },
      ...Array.from(counts.entries())
        .sort(([left], [right]) => left.localeCompare(right))
        .map(([value, count]) => ({ value, label: value.toUpperCase(), count })),
    ];
  }, [models, t]);

  const stateOptions: FilterOption[] = useMemo(
    () => [
      { value: "all", label: t("models.filters.all"), count: models?.length ?? 0 },
      {
        value: "cached",
        label: t("models.state.cached"),
        count: models?.filter((model) => model.cached).length ?? 0,
      },
      {
        value: "loaded",
        label: t("models.state.loaded"),
        count: models?.filter((model) => model.loaded).length ?? 0,
      },
    ],
    [models, t],
  );

  const hasFilters =
    query.trim().length > 0 || kind !== "all" || device !== "all" || stateFilter !== "all";

  function clearFilters() {
    setQuery("");
    setKind("all");
    setDevice("all");
    setStateFilter("all");
    searchRef.current?.focus();
  }

  function selectModel(alias: string) {
    setSelectedAlias(alias);
    setCardOpen(false);
  }

  function reloadDetail() {
    setDetailNonce((current) => current + 1);
  }

  function clearActionError(alias: string) {
    setErrors((current) => {
      if (!(alias in current)) {
        return current;
      }

      const next = { ...current };

      delete next[alias];

      return next;
    });
  }

  function recordActionError(alias: string, error: unknown) {
    setErrors((current) => ({ ...current, [alias]: toFoundryCommandError(error) }));
  }

  async function handleDownload(alias: string) {
    lastActionRef.current[alias] = "download";
    clearActionError(alias);

    try {
      const job = await downloadModel(alias);

      jobStatusRef.current[alias] = job.status;
      setJobs((current) => ({ ...current, [alias]: job }));
    } catch (error) {
      recordActionError(alias, error);
    }
  }

  async function handleCancelDownload(alias: string) {
    clearActionError(alias);

    try {
      await cancelModelDownload(alias);
    } catch (error) {
      recordActionError(alias, error);
    }
  }

  async function handleLoad(alias: string) {
    lastActionRef.current[alias] = "load";
    clearActionError(alias);
    setBusyAlias(alias);

    try {
      applyLoadedSummary(await loadModel(alias));
      toast.push(t("models.actions.loaded", { alias }));
      setDetailNonce((current) => current + 1);
    } catch (error) {
      recordActionError(alias, error);
    } finally {
      setBusyAlias(null);
    }
  }

  async function handleUnload(alias: string) {
    lastActionRef.current[alias] = "unload";
    clearActionError(alias);
    setBusyAlias(alias);

    try {
      applyLoadedSummary(await unloadModel(alias));
      toast.push(t("models.actions.unloaded", { alias }));
      setDetailNonce((current) => current + 1);
    } catch (error) {
      recordActionError(alias, error);
    } finally {
      setBusyAlias(null);
    }
  }

  async function confirmRemove() {
    if (!removeTarget) {
      return;
    }

    const alias = removeTarget.alias;

    lastActionRef.current[alias] = "remove";
    clearActionError(alias);
    setRemoving(true);

    try {
      const freed = await removeModel(alias);

      toast.push(
        t("models.actions.removed", {
          alias,
          size: formatBytes(freed.freedBytes, i18n.language),
        }),
      );
      setRemoveTarget(null);
      await loadCatalog();
      setDetailNonce((current) => current + 1);

      if (loadedOpen) {
        void refreshLoaded();
      }
    } catch (error) {
      recordActionError(alias, error);
      setRemoveTarget(null);
    } finally {
      setRemoving(false);
    }
  }

  function handleRetry(alias: string) {
    clearActionError(alias);

    const action = lastActionRef.current[alias];

    if (action === "load") {
      void handleLoad(alias);
    } else if (action === "unload") {
      void handleUnload(alias);
    } else if (action === "remove") {
      const target = models?.find((model) => model.alias === alias);

      if (target) {
        setRemoveTarget(target);
      }
    } else {
      void handleDownload(alias);
    }
  }

  function openLoaded() {
    setLoadedOpen(true);
    void refreshLoaded();
  }

  function handleCatalogChanged() {
    void loadCatalog();
    setDetailNonce((current) => current + 1);
  }

  const actionProps = visibleModel
    ? {
        job: jobs[visibleModel.alias],
        busy:
          busyAlias === visibleModel.alias ||
          (removing && removeTarget?.alias === visibleModel.alias),
        error: errors[visibleModel.alias] ?? null,
        onDownload: () => void handleDownload(visibleModel.alias),
        onCancelDownload: () => void handleCancelDownload(visibleModel.alias),
        onLoad: () => void handleLoad(visibleModel.alias),
        onUnload: () => void handleUnload(visibleModel.alias),
        onRemove: () => setRemoveTarget(visibleModel),
        onRetry: () => handleRetry(visibleModel.alias),
      }
    : null;

  return (
    <>
      <div className="contents">
        <ContextList
          title={t("models.listTitle")}
          action={
            <div className="flex items-center gap-0.5">
              <span className="mr-1 text-[10.5px] tabular-nums text-tertiary">
                {models ? filtered.length : ""}
              </span>
              <IconButton label={t("models.loaded.open")} onClick={openLoaded}>
                <LayersIcon size={14} />
              </IconButton>
              <IconButton label={t("models.cache.open")} onClick={() => setCacheOpen(true)}>
                <FolderIcon size={14} />
              </IconButton>
              <IconButton label={t("models.local.open")} onClick={() => setLocalOpen(true)}>
                <PlusIcon size={14} />
              </IconButton>
              <IconButton label={t("models.refresh")} onClick={() => void loadCatalog()}>
                <RefreshIcon size={14} className={loading && models ? "animate-spin" : ""} />
              </IconButton>
            </div>
          }
          toolbar={
            <>
              <div className="relative">
                <SearchIcon
                  size={13}
                  className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-tertiary"
                />
                <input
                  ref={searchRef}
                  type="text"
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Escape" && query) {
                      event.preventDefault();
                      setQuery("");
                    }
                  }}
                  placeholder={t("models.searchPlaceholder")}
                  aria-label={t("models.searchPlaceholder")}
                  disabled={!models}
                  className="h-[var(--control-size)] w-full rounded-[var(--item-radius)] border border-line bg-elevated pl-7 pr-7 text-[12px] text-primary outline-none transition-colors placeholder:text-tertiary focus:border-accent disabled:cursor-not-allowed disabled:opacity-70"
                />
                {query ? (
                  <button
                    type="button"
                    aria-label={t("models.clearSearch")}
                    title={t("models.clearSearch")}
                    onClick={() => {
                      setQuery("");
                      searchRef.current?.focus();
                    }}
                    className="absolute right-1 top-1/2 inline-flex h-5 w-5 -translate-y-1/2 items-center justify-center rounded text-tertiary transition-colors hover:bg-hover hover:text-primary"
                  >
                    <CloseIcon size={12} />
                  </button>
                ) : null}
              </div>
              <div className="mt-2 flex flex-wrap items-center gap-1.5 border-t border-line pt-2">
                <FilterMenu
                  label={t("models.filters.kind")}
                  value={kind}
                  options={kindOptions}
                  onChange={(value) => setKind(value as KindFilter)}
                />
                <FilterMenu
                  label={t("models.filters.device")}
                  value={device}
                  options={deviceOptions}
                  onChange={setDevice}
                />
                <FilterMenu
                  label={t("models.filters.state")}
                  value={stateFilter}
                  options={stateOptions}
                  onChange={(value) => setStateFilter(value as StateFilter)}
                />
                {hasFilters ? (
                  <button
                    type="button"
                    onClick={clearFilters}
                    className="h-7 rounded-[7px] px-1.5 text-[11.5px] text-tertiary transition-colors hover:text-primary"
                  >
                    {t("models.filters.clear")}
                  </button>
                ) : null}
              </div>
            </>
          }
        >
          {loading && !models ? <CatalogSkeleton /> : null}

          {!loading && listError ? (
            <ListEmpty
              icon={AlertIcon}
              title={t("models.errorTitle")}
              body={errorLabel(listError, t)}
            />
          ) : null}

          {!loading && !listError && filtered.length === 0 ? (
            <div className="flex flex-col items-center px-4 py-8 text-center">
              <p className="text-[12px] font-medium text-secondary">{t("models.noMatchesTitle")}</p>
              <p className="mt-1 text-[11px] leading-relaxed text-tertiary">
                {t("models.noMatchesBody")}
              </p>
              {hasFilters ? (
                <button
                  type="button"
                  onClick={clearFilters}
                  className="mt-3 h-7 rounded-[7px] border border-line px-2.5 text-[11.5px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary"
                >
                  {t("models.filters.clear")}
                </button>
              ) : null}
            </div>
          ) : null}

          {!loading && !listError && filtered.length > 0 ? (
            <ModelList
              models={filtered}
              jobs={jobs}
              errors={errors}
              busyAlias={busyAlias}
              selectedAlias={selectedAlias}
              onSelect={selectModel}
            />
          ) : null}
        </ContextList>
      </div>

      <DetailPane
        title={visibleModel ? visibleModel.alias : t("models.title")}
        subtitle={subtitle}
        actions={
          visibleModel ? (
            <IconButton label={t("models.openCard")} onClick={() => setCardOpen(true)}>
              <InfoIcon size={16} />
            </IconButton>
          ) : null
        }
      >
        {visibleModel && actionProps ? (
          <div>
            {detailError ? (
              <div className="mx-auto w-full max-w-[760px] px-4 pt-4 sm:px-6">
                <DetailError
                  message={errorLabel(detailError, t)}
                  detail={detailError.detail}
                  onRetry={reloadDetail}
                />
              </div>
            ) : null}
            <ModelOverview
              model={visibleModel}
              machine={loadedSummary}
              loading={detailLoading}
              onOpenCard={() => setCardOpen(true)}
              {...actionProps}
            />
          </div>
        ) : loading ? (
          <CatalogLoading />
        ) : listError ? (
          <CatalogError
            message={errorLabel(listError, t)}
            detail={listError.detail}
            onRetry={() => void loadCatalog()}
          />
        ) : models && models.length > 0 ? (
          <EmptyState
            icon={SearchIcon}
            title={t("models.noMatchesTitle")}
            body={t("models.noMatchesBody")}
          />
        ) : (
          <EmptyState
            icon={ModelsIcon}
            title={t("models.emptyTitle")}
            body={t("models.emptyBody")}
          />
        )}
      </DetailPane>

      {visibleModel && actionProps ? (
        <ModelCard
          model={visibleModel}
          open={cardOpen}
          onClose={() => setCardOpen(false)}
          {...actionProps}
        />
      ) : null}

      <LoadedPanel
        open={loadedOpen}
        summary={loadedSummary}
        loading={loadedLoading}
        error={loadedError}
        busyAlias={busyAlias}
        onClose={() => setLoadedOpen(false)}
        onRefresh={() => void refreshLoaded()}
        onUnload={(alias) => void handleUnload(alias)}
      />

      <CachePanel
        open={cacheOpen}
        onClose={() => setCacheOpen(false)}
        onChanged={handleCatalogChanged}
      />

      <LocalModelsPanel
        open={localOpen}
        onClose={() => setLocalOpen(false)}
        onChanged={handleCatalogChanged}
      />

      <ConfirmDialog
        open={removeTarget !== null}
        title={t("models.actions.confirmRemoveTitle", { alias: removeTarget?.alias ?? "" })}
        body={t("models.actions.confirmRemoveBody", {
          size: formatSize(removeTarget?.fileSizeMb ?? 0, i18n.language),
        })}
        confirmLabel={t("models.actions.remove")}
        tone="danger"
        busy={removing}
        onConfirm={() => void confirmRemove()}
        onClose={() => setRemoveTarget(null)}
      />
    </>
  );
}
