import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  cancelModelDownload,
  catalogList,
  cliDetect,
  downloadModel,
  FoundryCommandError,
  loadedModels,
  startServer,
  toFoundryCommandError,
  type CatalogModel,
  type CliInfo,
  type ModelJob,
  type ModelKind,
  type ServerSnapshot,
} from "../lib/foundry";
import { type InstallGuidance } from "../lib/onboardingInstallGuidance";
import { cx } from "../lib/cx";
import { useModelJobStream } from "../lib/useModelJobStream";
import type { PrimaryWorkspace, Workspace } from "../lib/workspaces";
import {
  AlertIcon,
  ChatIcon,
  CheckIcon,
  EmbeddingsIcon,
  ModelsIcon,
  RefreshIcon,
  SettingsIcon,
  SpeechIcon,
} from "./Icon";
import { KindBadge } from "../workspaces/models/ModelBadges";
import { errorLabel, formatSize, jobPercent } from "../workspaces/models/format";
import { Modal } from "./Modal";
import { Spinner } from "./Spinner";
import { Tooltip } from "./Tooltip";
import { useToast } from "./Toast";

const technicalSteps = ["cli", "server", "models", "ready"] as const;
const mixedTechnicalSteps = ["cli", "server", "models"] as const;
const usageSteps = ["workspaces", "chat", "speech", "embeddings", "models", "settings"] as const;

type TechnicalStepName = (typeof technicalSteps)[number];
type UsageStepName = (typeof usageSteps)[number];
type MachineTier = "lite" | "standard" | "pro";

const recommendations: { alias: string; kind: ModelKind; sizeMb: number }[] = [
  { alias: "qwen3-0.6b", kind: "chat", sizeMb: 529 },
  { alias: "whisper-tiny", kind: "speech", sizeMb: 131 },
  { alias: "qwen3-embedding-0.6b", kind: "embedding", sizeMb: 515 },
];

const startWorkspaces: PrimaryWorkspace[] = ["chat", "models", "speech", "embeddings"];

const workspaceIcons = {
  chat: ChatIcon,
  models: ModelsIcon,
  speech: SpeechIcon,
  embeddings: EmbeddingsIcon,
};

const usageIcons = {
  workspaces: ModelsIcon,
  chat: ChatIcon,
  speech: SpeechIcon,
  embeddings: EmbeddingsIcon,
  models: ModelsIcon,
  settings: SettingsIcon,
};

const usageSuggestedKinds: Partial<Record<UsageStepName, ModelKind[]>> = {
  chat: ["chat", "multimodal"],
  speech: ["speech"],
  embeddings: ["embedding"],
};

const targetSizeMbByTier: Record<MachineTier, Record<ModelKind, number>> = {
  lite: { chat: 3000, multimodal: 3000, speech: 1200, embedding: 1200 },
  standard: { chat: 8000, multimodal: 8000, speech: 2500, embedding: 2500 },
  pro: { chat: 14000, multimodal: 14000, speech: 5000, embedding: 5000 },
};

const machineTierThresholdsGb = {
  standard: 12,
  pro: 24,
} as const;

function machineTierFromMemory(totalMemoryBytes: number) {
  const gb = totalMemoryBytes / 1024 / 1024 / 1024;

  if (gb >= machineTierThresholdsGb.pro) {
    return "pro" as const;
  }

  if (gb >= machineTierThresholdsGb.standard) {
    return "standard" as const;
  }

  return "lite" as const;
}

function modelLabel(model: CatalogModel) {
  const display = model.displayName?.trim();

  return display && display.length > 0 ? display : model.alias;
}

function rankModelForTier(model: CatalogModel, tier: MachineTier) {
  const targetSize = targetSizeMbByTier[tier][model.kind];
  const size = model.fileSizeMb ?? targetSize;
  const delta = Math.abs(size - targetSize);
  const cachedBoost = model.cached ? -2000 : 0;
  const loadedBoost = model.loaded ? -3000 : 0;
  const unknownPenalty = model.fileSizeMb === null ? 2500 : 0;

  return delta + unknownPenalty + cachedBoost + loadedBoost;
}

function recommendedFromCatalog(step: UsageStepName, tier: MachineTier, catalog: CatalogModel[] | null) {
  if (!catalog || catalog.length === 0) {
    return [] as string[];
  }

  if (step === "models") {
    const picks: string[] = [];
    const selectedAliases = new Set<string>();

    for (const kind of ["chat", "speech", "embedding"] as const) {
      const match = catalog
        .filter((model) => model.kind === kind)
        .sort((left, right) => rankModelForTier(left, tier) - rankModelForTier(right, tier))[0];

      if (!match || selectedAliases.has(match.alias)) {
        continue;
      }

      selectedAliases.add(match.alias);
      picks.push(modelLabel(match));
    }

    return picks;
  }

  const allowedKinds = usageSuggestedKinds[step] ?? [];

  if (allowedKinds.length === 0) {
    return [];
  }

  return catalog
    .filter((model) => allowedKinds.includes(model.kind))
    .sort((left, right) => rankModelForTier(left, tier) - rankModelForTier(right, tier))
    .slice(0, 3)
    .map((model) => modelLabel(model));
}

function stringList(value: unknown) {
  if (!Array.isArray(value)) {
    return [] as string[];
  }

  return value.filter((item): item is string => typeof item === "string" && item.trim().length > 0);
}

export type OnboardingFlow = "critical-cli" | "technical" | "usage" | "mixed";

export type OnboardingEvent =
  | { type: "closed" }
  | { type: "critical-skipped" }
  | { type: "critical-complete" }
  | { type: "usage-complete" }
  | { type: "usage-never-show" };

export type OnboardingSpotlightTarget =
  | "rail-map"
  | "rail-chat"
  | "rail-speech"
  | "rail-embeddings"
  | "rail-models"
  | "rail-settings"
  | null;

type OnboardingProps = {
  flow: OnboardingFlow;
  installGuidance: InstallGuidance;
  onEvent: (event: OnboardingEvent) => void;
  onChooseWorkspace: (workspace: PrimaryWorkspace) => void;
  onSpotlightChange?: (target: OnboardingSpotlightTarget) => void;
  onUsageWorkspaceChange?: (workspace: Workspace | null) => void;
};

function isRecommended(alias: string) {
  return recommendations.some((recommendation) => recommendation.alias === alias);
}

export function Onboarding({
  flow,
  installGuidance,
  onEvent,
  onChooseWorkspace,
  onSpotlightChange,
  onUsageWorkspaceChange,
}: OnboardingProps) {
  const { t, i18n } = useTranslation();
  const toast = useToast();
  const [phase, setPhase] = useState<"technical" | "usage">(flow === "mixed" ? "technical" : "usage");
  const [technicalStep, setTechnicalStep] = useState<TechnicalStepName>("cli");
  const [usageStep, setUsageStep] = useState<UsageStepName>("workspaces");
  const [cli, setCli] = useState<CliInfo | null>(null);
  const [cliError, setCliError] = useState<FoundryCommandError | null>(null);
  const [checkingCli, setCheckingCli] = useState(false);
  const [server, setServer] = useState<ServerSnapshot | null>(null);
  const [serverError, setServerError] = useState<FoundryCommandError | null>(null);
  const [startingServer, setStartingServer] = useState(false);
  const [models, setModels] = useState<CatalogModel[] | null>(null);
  const [machineTier, setMachineTier] = useState<MachineTier>("standard");
  const [cachedAliases, setCachedAliases] = useState<Set<string>>(new Set());
  const [jobs, setJobs] = useState<Record<string, ModelJob>>({});
  const [downloadErrors, setDownloadErrors] = useState<Record<string, FoundryCommandError>>({});

  const technicalStepList: readonly TechnicalStepName[] = flow === "mixed" ? mixedTechnicalSteps : technicalSteps;
  const adaptiveTechnical = flow === "technical" || flow === "mixed";
  const needsTechnical = flow === "critical-cli" || flow === "technical" || (flow === "mixed" && phase === "technical");
  const usageActive = flow === "usage" || (flow === "mixed" && phase === "usage");
  const usageIndex = usageSteps.indexOf(usageStep);
  const modalPlacement = usageActive ? (usageStep === "workspaces" ? "top-right" : "top-left") : "center";

  useEffect(() => {
    if (!onSpotlightChange) {
      return;
    }

    if (!usageActive) {
      onSpotlightChange(null);
      return;
    }

    const mapping: Record<UsageStepName, OnboardingSpotlightTarget> = {
      workspaces: "rail-map",
      chat: "rail-chat",
      speech: "rail-speech",
      embeddings: "rail-embeddings",
      models: "rail-models",
      settings: "rail-settings",
    };

    onSpotlightChange(mapping[usageStep]);

    return () => {
      onSpotlightChange(null);
    };
  }, [onSpotlightChange, usageActive, usageStep]);

  useEffect(() => {
    if (!onUsageWorkspaceChange) {
      return;
    }

    if (!usageActive || usageStep === "workspaces") {
      onUsageWorkspaceChange(null);
      return;
    }

    onUsageWorkspaceChange(usageStep);
  }, [onUsageWorkspaceChange, usageActive, usageStep]);

  useEffect(() => {
    if (!usageActive) {
      return;
    }

    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") {
        return;
      }

      event.preventDefault();
      onEvent({ type: "closed" });
    }

    window.addEventListener("keydown", onKeyDown);

    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [onEvent, usageActive]);

  const markCached = useCallback((alias: string) => {
    setCachedAliases((current) => {
      if (current.has(alias)) {
        return current;
      }

      return new Set(current).add(alias);
    });
  }, []);

  const applyJob = useCallback(
    (job: ModelJob) => {
      if (!isRecommended(job.alias)) {
        return;
      }

      setJobs((current) => {
        const known = current[job.alias];

        if (known && known.updatedAt > job.updatedAt) {
          return current;
        }

        return { ...current, [job.alias]: job };
      });

      if (job.status === "succeeded") {
        markCached(job.alias);
      }

      const failure = job.error;

      if (job.status === "failed" && failure) {
        setDownloadErrors((current) => ({
          ...current,
          [job.alias]: new FoundryCommandError(failure),
        }));
      }
    },
    [markCached],
  );

  useModelJobStream(applyJob);

  const downloaded = useMemo(
    () => recommendations.filter((recommendation) => cachedAliases.has(recommendation.alias)).length,
    [cachedAliases],
  );

  const allRecommendedDownloaded = downloaded >= recommendations.length;

  const verifyCli = useCallback(async () => {
    setCheckingCli(true);

    try {
      const found = await cliDetect();

      setCli(found);
      setCliError(null);

      return true;
    } catch (error) {
      setCli(null);
      setCliError(toFoundryCommandError(error));

      return false;
    } finally {
      setCheckingCli(false);
    }
  }, []);

  const ensureServer = useCallback(async () => {
    setStartingServer(true);
    setServerError(null);

    try {
      setServer(await startServer());
      return true;
    } catch (error) {
      setServer(null);
      setServerError(toFoundryCommandError(error));
      return false;
    } finally {
      setStartingServer(false);
    }
  }, []);

  useEffect(() => {
    if (!needsTechnical) {
      return;
    }

    void verifyCli();
  }, [needsTechnical, verifyCli]);

  useEffect(() => {
    if (!needsTechnical || technicalStep !== "server") {
      return;
    }

    void ensureServer();
  }, [needsTechnical, technicalStep, ensureServer]);

  useEffect(() => {
    let disposed = false;

    catalogList()
      .then((next) => {
        if (disposed) {
          return;
        }

        setModels(next);
        setCachedAliases((current) => {
          const cached = next.filter((model) => model.cached).map((model) => model.alias);

          return cached.length === 0 ? current : new Set([...current, ...cached]);
        });
      })
      .catch(() => {
        if (!disposed) {
          setModels([]);
        }
      });

    return () => {
      disposed = true;
    };
  }, []);

  useEffect(() => {
    if (!usageActive) {
      return;
    }

    let disposed = false;

    loadedModels()
      .then((summary) => {
        if (disposed) {
          return;
        }

        setMachineTier(machineTierFromMemory(summary.totalMemoryBytes));
      })
      .catch(() => {
        if (!disposed) {
          setMachineTier("standard");
        }
      });

    return () => {
      disposed = true;
    };
  }, [usageActive]);

  useEffect(() => {
    if (!adaptiveTechnical || !needsTechnical) {
      return;
    }

    if (technicalStep === "cli" && !checkingCli && cli) {
      setTechnicalStep("server");
      return;
    }

    if (technicalStep === "server" && server?.running) {
      setTechnicalStep("models");
      return;
    }

    if (technicalStep === "models" && allRecommendedDownloaded) {
      if (flow === "mixed") {
        setPhase("usage");
      } else {
        setTechnicalStep("ready");
      }
    }
  }, [adaptiveTechnical, allRecommendedDownloaded, checkingCli, cli, flow, needsTechnical, server, technicalStep]);

  function closeByDismiss() {
    if (flow === "critical-cli") {
      onEvent({ type: "critical-skipped" });
      return;
    }

    onEvent({ type: "closed" });
  }

  async function copyCommand(command: string) {
    try {
      await navigator.clipboard.writeText(command);
      toast.push(t("onboarding.cli.copied"));
    } catch {
      toast.push(t("onboarding.cli.copyFailed"), "danger");
    }
  }

  async function openInstaller() {
    try {
      await openUrl(installGuidance.installerUrl);
    } catch {
      window.open(installGuidance.installerUrl, "_blank", "noreferrer");
    }
  }

  async function openDocs() {
    try {
      await openUrl(installGuidance.docsUrl);
    } catch {
      window.open(installGuidance.docsUrl, "_blank", "noreferrer");
    }
  }

  async function handleDownload(alias: string) {
    setDownloadErrors((current) => {
      if (!(alias in current)) {
        return current;
      }

      const next = { ...current };

      delete next[alias];

      return next;
    });

    try {
      applyJob(await downloadModel(alias));
    } catch (error) {
      setDownloadErrors((current) => ({ ...current, [alias]: toFoundryCommandError(error) }));
    }
  }

  async function handleCancel(alias: string) {
    try {
      await cancelModelDownload(alias);
    } catch (error) {
      setDownloadErrors((current) => ({ ...current, [alias]: toFoundryCommandError(error) }));
    }
  }

  function continueFromModels() {
    if (flow === "mixed") {
      setPhase("usage");
      return;
    }

    setTechnicalStep("ready");
  }

  function completeUsage(workspace?: PrimaryWorkspace) {
    if (workspace) {
      onChooseWorkspace(workspace);
    }

    onEvent({ type: "usage-complete" });
  }

  function selectWorkspaceFromTechnical(workspace: PrimaryWorkspace) {
    onChooseWorkspace(workspace);
    onEvent({ type: "closed" });
  }

  function summaryForTechnical(name: TechnicalStepName) {
    if (name === "cli" && cli) {
      return `v${cli.version}`;
    }

    if (name === "server" && server?.running) {
      return server.endpoint ?? (server.port === null ? null : `:${server.port}`);
    }

    if (name === "models") {
      return t("onboarding.models.downloadedCount", { count: downloaded });
    }

    return null;
  }

  function renderInstallGuidance() {
    const command = installGuidance.command;

    return (
      <>
        {command ? (
          <div className="mt-2.5 flex items-center gap-2 rounded-[7px] border border-line bg-subtle px-2.5 py-2">
            <code className="min-w-0 flex-1 break-all font-mono text-[10.5px] text-secondary">
              {command}
            </code>
            <button
              type="button"
              onClick={() => void copyCommand(command)}
              className="h-6 shrink-0 rounded-[5px] border border-line bg-elevated px-2 text-[10.5px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary"
            >
              {t("onboarding.cli.copy")}
            </button>
          </div>
        ) : (
          <p className="mt-2.5 text-[11px] leading-relaxed text-secondary">{t("onboarding.cli.noCommandForPlatform")}</p>
        )}
        <div className="mt-2.5 flex flex-wrap items-center gap-2">
          <button type="button" onClick={() => void verifyCli()} className={primaryButton}>
            <RefreshIcon size={12} />
            {t("onboarding.cli.verify")}
          </button>
          <button type="button" onClick={() => void openInstaller()} className={secondaryButton}>
            {t("onboarding.cli.installer")}
          </button>
          <button type="button" onClick={() => void openDocs()} className={secondaryButton}>
            {t("onboarding.cli.docs")}
          </button>
        </div>
      </>
    );
  }

  function renderCliStep() {
    if (checkingCli) {
      return (
        <p className="flex items-center gap-2 text-[12px] text-tertiary">
          <Spinner size={12} />
          {t("onboarding.cli.checking")}
        </p>
      );
    }

    if (cli) {
      return (
        <div>
          <p className="flex items-center gap-1.5 text-[12px] font-medium text-primary">
            <CheckIcon size={13} className="text-success" />
            {t("onboarding.cli.presentTitle", { version: cli.version })}
          </p>
          <p className="mt-1 break-all font-mono text-[10.5px] text-tertiary">{cli.path}</p>
          {flow === "critical-cli" ? (
            <button
              type="button"
              onClick={() => onEvent({ type: "critical-complete" })}
              className={`${primaryButton} mt-2.5`}
            >
              {t("onboarding.cli.finish")}
            </button>
          ) : (
            <button
              type="button"
              onClick={() => setTechnicalStep("server")}
              className={`${primaryButton} mt-2.5`}
            >
              {t("onboarding.cli.continue")}
            </button>
          )}
        </div>
      );
    }

    if (cliError && cliError.code !== "CliMissing") {
      return (
        <div>
          <div className={errorBox}>
            <AlertIcon size={13} className="mt-0.5 shrink-0 text-danger" />
            <p className="min-w-0 flex-1 break-words">{errorLabel(cliError, t)}</p>
          </div>
          {cliError.code === "CliOutdated" ? (
            <>
              <p className="mt-2.5 text-[12px] leading-relaxed text-secondary">{t("onboarding.cli.outdatedBody")}</p>
              {renderInstallGuidance()}
            </>
          ) : (
            <button type="button" onClick={() => void verifyCli()} className={`${primaryButton} mt-2.5`}>
              <RefreshIcon size={12} />
              {t("onboarding.cli.verify")}
            </button>
          )}
        </div>
      );
    }

    return (
      <div>
        <p className="text-[12px] leading-relaxed text-secondary">{t("onboarding.cli.missingBody")}</p>
        {renderInstallGuidance()}
      </div>
    );
  }

  function renderServerStep() {
    if (startingServer) {
      return (
        <p className="flex items-center gap-2 text-[12px] text-tertiary">
          <Spinner size={12} />
          {t("onboarding.server.starting")}
        </p>
      );
    }

    if (serverError) {
      return (
        <div>
          <div className={errorBox}>
            <AlertIcon size={13} className="mt-0.5 shrink-0 text-danger" />
            <p className="min-w-0 flex-1 break-words">{errorLabel(serverError, t)}</p>
          </div>
          <button type="button" onClick={() => void ensureServer()} className={`${primaryButton} mt-2.5`}>
            <RefreshIcon size={12} />
            {t("onboarding.server.retry")}
          </button>
        </div>
      );
    }

    if (server?.running) {
      return (
        <div>
          <p className="flex items-center gap-1.5 text-[12px] font-medium text-primary">
            <CheckIcon size={13} className="text-success" />
            {t("onboarding.server.running")}
          </p>
          <dl className="mt-2 grid grid-cols-[84px_1fr] gap-y-1 text-[11.5px]">
            <dt className="text-tertiary">{t("onboarding.server.endpoint")}</dt>
            <dd className="break-all font-mono text-secondary">{server.endpoint ?? "—"}</dd>
            {server.port === null ? null : (
              <>
                <dt className="text-tertiary">{t("onboarding.server.port")}</dt>
                <dd className="tabular-nums text-secondary">{server.port}</dd>
              </>
            )}
          </dl>
          <button type="button" onClick={() => setTechnicalStep("models")} className={`${primaryButton} mt-2.5`}>
            {t("onboarding.server.continue")}
          </button>
        </div>
      );
    }

    return (
      <button type="button" onClick={() => void ensureServer()} className={primaryButton}>
        <RefreshIcon size={12} />
        {t("onboarding.server.retry")}
      </button>
    );
  }

  function renderModelsStep() {
    return (
      <div>
        <p className="text-[12px] leading-relaxed text-secondary">{t("onboarding.models.body")}</p>
        <ul className="mt-2.5 flex flex-col gap-2">
          {recommendations.map((recommendation) => {
            const model = models?.find((entry) => entry.alias === recommendation.alias);
            const job = jobs[recommendation.alias];
            const running = job?.status === "running";
            const cached = cachedAliases.has(recommendation.alias) || model?.cached === true;
            const error = downloadErrors[recommendation.alias] ?? null;
            const fallbackKind = recommendation.kind;
            const sizeMb = model?.fileSizeMb ?? recommendation.sizeMb;

            return (
              <li key={recommendation.alias} className="rounded-[8px] border border-line bg-canvas px-3 py-2">
                <div className="flex items-center gap-3">
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="truncate text-[12px] font-medium text-primary">{recommendation.alias}</span>
                      <KindBadge kind={model?.kind ?? fallbackKind} />
                    </div>
                    <p className="mt-0.5 text-[10.5px] tabular-nums text-tertiary">
                      {formatSize(sizeMb, i18n.language)} ·{" "}
                      {running
                        ? t("onboarding.models.downloading")
                        : cached
                          ? t("onboarding.models.downloaded")
                          : t("onboarding.models.notDownloaded")}
                    </p>
                  </div>
                  {running ? (
                    <button type="button" onClick={() => void handleCancel(recommendation.alias)} className={secondaryButton}>
                      {t("onboarding.models.cancel")}
                    </button>
                  ) : cached ? (
                    <span className="inline-flex shrink-0 items-center gap-1 text-[11px] text-success">
                      <CheckIcon size={12} />
                      {t("onboarding.models.downloaded")}
                    </span>
                  ) : (
                    <button type="button" onClick={() => void handleDownload(recommendation.alias)} className={primaryButton}>
                      {t("onboarding.models.download")}
                    </button>
                  )}
                </div>
                {running && job ? (
                  <div
                    className="mt-2 h-1 overflow-hidden rounded-full bg-active"
                    role="progressbar"
                    aria-valuemin={0}
                    aria-valuemax={100}
                    aria-valuenow={jobPercent(job)}
                    aria-label={t("onboarding.models.downloading")}
                  >
                    <div
                      className="h-full rounded-full bg-accent transition-[width] duration-500 ease-out"
                      style={{ width: `${jobPercent(job)}%` }}
                    />
                  </div>
                ) : null}
                {error ? (
                  <div className="mt-2 flex items-start gap-2 text-[10.5px] text-secondary">
                    <AlertIcon size={12} className="mt-0.5 shrink-0 text-danger" />
                    <p className="min-w-0 flex-1 break-words">{errorLabel(error, t)}</p>
                    <button
                      type="button"
                      onClick={() => void handleDownload(recommendation.alias)}
                      className="shrink-0 text-[10.5px] font-medium text-accent hover:underline"
                    >
                      {t("onboarding.models.retry")}
                    </button>
                  </div>
                ) : null}
              </li>
            );
          })}
        </ul>
        <button type="button" onClick={continueFromModels} className={`${primaryButton} mt-3`}>
          {flow === "mixed" ? t("onboarding.models.continueUsage") : t("onboarding.models.continue")}
        </button>
      </div>
    );
  }

  function renderTechnicalReadyStep() {
    return (
      <div>
        <p className="text-[12px] leading-relaxed text-secondary">{t("onboarding.ready.body")}</p>
        <div className="mt-2.5 grid grid-cols-2 gap-2">
          {startWorkspaces.map((value) => {
            const Icon = workspaceIcons[value];

            return (
              <button
                key={value}
                type="button"
                onClick={() => selectWorkspaceFromTechnical(value)}
                className="flex items-center gap-2.5 rounded-[8px] border border-line bg-canvas px-3 py-2.5 text-left transition-colors hover:border-line-strong hover:bg-hover"
              >
                <Icon size={16} className="shrink-0 text-accent" />
                <span className="text-[12px] font-medium text-primary">{t(`rail.${value}`)}</span>
              </button>
            );
          })}
        </div>
        <button type="button" onClick={() => onEvent({ type: "closed" })} className={`${secondaryButton} mt-3`}>
          {t("onboarding.ready.finishTechnical")}
        </button>
      </div>
    );
  }

  function renderTechnicalStep(name: TechnicalStepName) {
    switch (name) {
      case "cli":
        return renderCliStep();
      case "server":
        return renderServerStep();
      case "models":
        return renderModelsStep();
      case "ready":
        return renderTechnicalReadyStep();
    }
  }

  function goUsageBack() {
    const next = usageSteps[Math.max(0, usageIndex - 1)];

    if (next) {
      setUsageStep(next);
    }
  }

  function goUsageNext() {
    const next = usageSteps[Math.min(usageSteps.length - 1, usageIndex + 1)];

    if (next) {
      setUsageStep(next);
    }
  }

  function renderCritical() {
    return (
      <>
        <p className="text-[11.5px] leading-relaxed text-tertiary">{t("onboarding.critical.subtitle")}</p>
        <div className="mt-3 rounded-[10px] border border-line bg-canvas px-3.5 py-3">
          <p className="text-[12.5px] font-medium text-primary">{t("onboarding.steps.cli")}</p>
          <div className="mt-3">{renderCliStep()}</div>
        </div>
        <div className="mt-4 flex justify-center border-t border-line pt-3">
          <button type="button" onClick={() => onEvent({ type: "critical-skipped" })} className="text-[11.5px] text-tertiary transition-colors hover:text-primary">
            {t("onboarding.skip")}
          </button>
        </div>
      </>
    );
  }

  function renderTechnical() {
    const stepIndex = technicalStepList.indexOf(technicalStep);

    return (
      <>
        <p className="text-[11.5px] leading-relaxed text-tertiary">{t("onboarding.subtitle")}</p>
        <ol className="mt-3 flex flex-col gap-1.5">
          {technicalStepList.map((name, index) => {
            const done = index < stepIndex;
            const current = index === stepIndex;
            const summary = done ? summaryForTechnical(name) : null;

            return (
              <li
                key={name}
                className={cx(
                  "rounded-[10px] border px-3.5 py-3",
                  current ? "border-line bg-canvas" : "border-transparent",
                )}
              >
                <button
                  type="button"
                  onClick={() => setTechnicalStep(name)}
                  disabled={current}
                  className={cx(
                    "flex w-full items-center gap-2.5 text-left",
                    current ? "cursor-default" : "cursor-pointer",
                  )}
                >
                  <span
                    className={cx(
                      "inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-full border text-[10px] font-semibold tabular-nums",
                      done
                        ? "border-accent bg-accent text-accent-contrast"
                        : current
                          ? "border-accent text-accent"
                          : "border-line text-tertiary",
                    )}
                  >
                    {done ? <CheckIcon size={11} /> : index + 1}
                  </span>
                  <p
                    className={cx(
                      "min-w-0 flex-1 text-[12.5px]",
                      current || done ? "font-medium text-primary" : "text-tertiary",
                    )}
                  >
                    {t(`onboarding.steps.${name}`)}
                  </p>
                  {summary ? (
                    <span className="shrink-0 truncate font-mono text-[10px] text-tertiary">{summary}</span>
                  ) : null}
                </button>
                {current ? <div className="detail-enter mt-3 pl-[30px]">{renderTechnicalStep(name)}</div> : null}
              </li>
            );
          })}
        </ol>
        <div className="mt-4 flex justify-center border-t border-line pt-3">
          <button type="button" onClick={() => onEvent({ type: "closed" })} className="text-[11.5px] text-tertiary transition-colors hover:text-primary">
            {t("onboarding.skip")}
          </button>
        </div>
      </>
    );
  }

  function renderUsage() {
    const StepIcon = usageIcons[usageStep];
    const progress = ((usageIndex + 1) / usageSteps.length) * 100;
    const stepBody = t(`onboarding.usage.${usageStep}.body`);
    const requirementKey = `onboarding.usage.${usageStep}.requirement`;
    const requirementTextRaw = String(i18n.t(requirementKey as never));
    const requirementText = requirementTextRaw === requirementKey ? null : requirementTextRaw;
    const suggestedFromCatalog = recommendedFromCatalog(usageStep, machineTier, models);
    const suggestedModelsByTierKey = `onboarding.usage.${usageStep}.suggestedModels.${machineTier}`;
    const suggestedModelsByTier = stringList(
      i18n.t(suggestedModelsByTierKey as never, { returnObjects: true } as never) as unknown,
    );
    const suggestedModelsLegacy = stringList(
      i18n.t(`onboarding.usage.${usageStep}.suggestedModels` as never, { returnObjects: true } as never) as unknown,
    );
    const suggestedModels = suggestedFromCatalog.length > 0 ? suggestedFromCatalog : suggestedModelsByTier.length > 0 ? suggestedModelsByTier : suggestedModelsLegacy;
    const canFinishUsage = usageStep === "settings";

    return (
      <section className="detail-enter shrink-0 border-b border-accent/40 bg-gradient-to-r from-accent/10 via-elevated to-elevated px-4 py-4">
        <div className="flex flex-wrap items-center gap-2.5">
          <span className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-full border border-accent bg-accent text-accent-contrast">
            <StepIcon size={14} />
          </span>
          <p className="min-w-[180px] flex-1 truncate text-[13.5px] leading-5 font-bold text-primary">
            {t(`onboarding.usage.steps.${usageStep}`)}
          </p>
          <span className="shrink-0 text-[10.5px] font-medium tabular-nums text-tertiary">
            {usageIndex + 1}/{usageSteps.length}
          </span>
          <div className="ml-auto flex items-center gap-1">
            <button type="button" onClick={goUsageBack} disabled={usageIndex === 0} className={compactSecondaryButton}>
              {t("onboarding.usage.back")}
            </button>
            {canFinishUsage ? (
              <button type="button" onClick={() => completeUsage()} className={compactPrimaryButton}>
                {t("onboarding.usage.finish")}
              </button>
            ) : (
              <button type="button" onClick={goUsageNext} className={compactPrimaryButton}>
                {t("onboarding.usage.next")}
              </button>
            )}
          </div>
        </div>

        <div className="mt-2.5">
          <div className="rounded-[10px] border border-accent/55 bg-accent/12 px-3 py-3 shadow-[0_0_0_1px_rgba(59,130,246,0.2)]">
            <p className="text-[10px] font-semibold uppercase tracking-[0.05em] text-accent">
              {t("onboarding.usage.currentTitle")}
            </p>
            <p className="mt-1.5 text-[12.25px] leading-[1.52] text-primary">{stepBody}</p>

            {requirementText ? (
              <span className="mt-2 inline-flex min-h-6 items-center rounded-[7px] border border-accent/45 bg-canvas/86 px-2 py-1 text-[11px] font-medium text-accent">
                {requirementText}
              </span>
            ) : null}

            {suggestedModels.length > 0 ? (
              <div className="mt-2">
                <p className="text-[10px] font-semibold uppercase tracking-[0.05em] text-tertiary">{t("onboarding.usage.suggestedModelsTitle")}</p>
                <div className="mt-1.5 flex flex-wrap gap-1.5">
                  {suggestedModels.map((model) => (
                    <span key={`${usageStep}-${model}`} className="inline-flex min-h-6 items-center rounded-[7px] border border-line bg-canvas/82 px-2 py-1 text-[10.75px] font-medium text-secondary">
                      {model}
                    </span>
                  ))}
                </div>
              </div>
            ) : null}
          </div>

          <div className="mt-2.5 flex items-center justify-between gap-2">
            <div className="inline-flex items-center gap-1 rounded-[7px] border border-accent/45 bg-accent/10 px-1.5 py-1">
              {usageSteps.map((name, index) => {
                const done = index < usageIndex;
                const current = index === usageIndex;
                const Icon = usageIcons[name];

                return (
                  <Tooltip key={name} label={t(`onboarding.usage.steps.${name}`)} side="bottom" showDelayMs={160}>
                    <button
                      type="button"
                      onClick={() => setUsageStep(name)}
                      aria-label={t(`onboarding.usage.steps.${name}`)}
                      className={cx(
                        "inline-flex h-7 w-7 items-center justify-center rounded-[6px] border transition-colors",
                        current
                          ? "border-accent bg-accent text-accent-contrast"
                          : done
                            ? "border-accent text-accent hover:bg-hover"
                            : "border-line bg-elevated text-tertiary hover:border-line-strong hover:text-primary",
                      )}
                    >
                      {done ? <CheckIcon size={12} /> : <Icon size={12} />}
                    </button>
                  </Tooltip>
                );
              })}
            </div>

            <div className="flex items-center gap-2">
              <button type="button" onClick={() => onEvent({ type: "closed" })} className="text-[10.5px] font-medium text-tertiary transition-colors hover:text-primary">
                {t("onboarding.skip")}
              </button>
              <button type="button" onClick={() => onEvent({ type: "usage-never-show" })} className="text-[10.5px] font-medium text-tertiary transition-colors hover:text-primary">
                {t("onboarding.neverShow")}
              </button>
            </div>
          </div>
        </div>

        <div className="mt-2.5 h-1.5 overflow-hidden rounded-full bg-accent/20">
          <div
            className="h-full rounded-full bg-accent-hover transition-[width] duration-300 ease-out"
            style={{ width: `${progress}%` }}
          />
        </div>
      </section>
    );
  }

  if (usageActive) {
    return renderUsage();
  }

  return (
    <Modal
      open
      title={flow === "critical-cli" ? t("onboarding.critical.title") : t("onboarding.title")}
      onClose={closeByDismiss}
      size={flow === "critical-cli" ? "md" : "lg"}
      placement={modalPlacement}
    >
      {flow === "critical-cli" ? renderCritical() : renderTechnical()}
    </Modal>
  );
}

const primaryButton =
  "inline-flex h-8 items-center gap-1.5 rounded-[7px] bg-accent px-3 text-[12px] font-medium text-accent-contrast transition-colors hover:bg-accent-hover disabled:opacity-50";

const secondaryButton =
  "inline-flex h-8 items-center gap-1.5 rounded-[7px] border border-line bg-elevated px-3 text-[12px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary disabled:opacity-50";

const compactPrimaryButton =
  "inline-flex h-7 items-center gap-1 rounded-[6px] bg-accent px-3 text-[11.5px] font-medium text-accent-contrast transition-colors hover:bg-accent-hover disabled:opacity-50";

const compactSecondaryButton =
  "inline-flex h-7 items-center gap-1 rounded-[6px] border border-line bg-elevated px-3 text-[11.5px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary disabled:opacity-50";

const errorBox =
  "flex items-start gap-2 rounded-[7px] border border-line bg-subtle px-2.5 py-2 text-[11.5px] text-secondary";
