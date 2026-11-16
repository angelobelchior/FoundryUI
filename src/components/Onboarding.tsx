import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  cancelModelDownload,
  catalogList,
  cliDetect,
  downloadModel,
  FoundryCommandError,
  startServer,
  toFoundryCommandError,
  type CatalogModel,
  type CliInfo,
  type ModelJob,
  type ModelKind,
  type ServerSnapshot,
} from "../lib/foundry";
import { cx } from "../lib/cx";
import { useModelJobStream } from "../lib/useModelJobStream";
import type { PrimaryWorkspace } from "../lib/workspaces";
import {
  AlertIcon,
  ChatIcon,
  CheckIcon,
  EmbeddingsIcon,
  ModelsIcon,
  RefreshIcon,
  SpeechIcon,
} from "./Icon";
import { KindBadge } from "../workspaces/models/ModelBadges";
import { errorLabel, formatSize, jobPercent } from "../workspaces/models/format";
import { Modal } from "./Modal";
import { Spinner } from "./Spinner";
import { useToast } from "./Toast";

const installerUrl = "https://aka.ms/foundry-local-installer";
const installCommand = "brew tap microsoft/foundrylocal && brew install foundrylocal";

const steps = ["cli", "server", "models", "ready"] as const;
type StepName = (typeof steps)[number];

const recommendations: { alias: string; kind: ModelKind; sizeMb: number }[] = [
  { alias: "qwen3-0.6b", kind: "chat", sizeMb: 529 },
  { alias: "whisper-tiny", kind: "speech", sizeMb: 131 },
  { alias: "qwen3-embedding-0.6b", kind: "embedding", sizeMb: 515 },
];

function isRecommended(alias: string) {
  return recommendations.some((recommendation) => recommendation.alias === alias);
}

const startWorkspaces: PrimaryWorkspace[] = ["chat", "models", "speech", "embeddings"];

const workspaceIcons = {
  chat: ChatIcon,
  models: ModelsIcon,
  speech: SpeechIcon,
  embeddings: EmbeddingsIcon,
};

type OnboardingProps = {
  onClose: () => void;
  onChooseWorkspace: (workspace: PrimaryWorkspace) => void;
};

export function Onboarding({ onClose, onChooseWorkspace }: OnboardingProps) {
  const { t, i18n } = useTranslation();
  const toast = useToast();
  const [step, setStep] = useState<StepName>("cli");
  const [cli, setCli] = useState<CliInfo | null>(null);
  const [cliError, setCliError] = useState<FoundryCommandError | null>(null);
  const [checkingCli, setCheckingCli] = useState(true);
  const [server, setServer] = useState<ServerSnapshot | null>(null);
  const [serverError, setServerError] = useState<FoundryCommandError | null>(null);
  const [startingServer, setStartingServer] = useState(false);
  const [models, setModels] = useState<CatalogModel[] | null>(null);
  const [cachedAliases, setCachedAliases] = useState<Set<string>>(new Set());
  const [jobs, setJobs] = useState<Record<string, ModelJob>>({});
  const [downloadErrors, setDownloadErrors] = useState<Record<string, FoundryCommandError>>({});

  const stepIndex = steps.indexOf(step);

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

  async function recheckCli() {
    if (await verifyCli()) {
      setStep("server");
    }
  }

  const ensureServer = useCallback(async () => {
    setStartingServer(true);
    setServerError(null);

    try {
      setServer(await startServer());
    } catch (error) {
      setServer(null);
      setServerError(toFoundryCommandError(error));
    } finally {
      setStartingServer(false);
    }
  }, []);

  useEffect(() => {
    void verifyCli();
  }, [verifyCli]);

  useEffect(() => {
    if (step === "server") {
      void ensureServer();
    }
  }, [step, ensureServer]);

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

  const downloaded = useMemo(
    () => recommendations.filter((recommendation) => cachedAliases.has(recommendation.alias)).length,
    [cachedAliases],
  );

  async function copyCommand() {
    try {
      await navigator.clipboard.writeText(installCommand);
      toast.push(t("onboarding.cli.copied"));
    } catch {
      toast.push(t("onboarding.cli.copyFailed"), "danger");
    }
  }

  async function openInstaller() {
    try {
      await openUrl(installerUrl);
    } catch {
      window.open(installerUrl, "_blank", "noreferrer");
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

  function summaryFor(name: StepName) {
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

  function installGuidance() {
    return (
      <>
        <div className="mt-2.5 flex items-center gap-2 rounded-[7px] border border-line bg-subtle px-2.5 py-2">
          <code className="min-w-0 flex-1 break-all font-mono text-[10.5px] text-secondary">
            {installCommand}
          </code>
          <button
            type="button"
            onClick={() => void copyCommand()}
            className="h-6 shrink-0 rounded-[5px] border border-line bg-elevated px-2 text-[10.5px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary"
          >
            {t("onboarding.cli.copy")}
          </button>
        </div>
        <div className="mt-2.5 flex flex-wrap items-center gap-2">
          <button type="button" onClick={() => void recheckCli()} className={primaryButton}>
            <RefreshIcon size={12} />
            {t("onboarding.cli.verify")}
          </button>
          <button type="button" onClick={() => void openInstaller()} className={secondaryButton}>
            {t("onboarding.cli.installer")}
          </button>
        </div>
      </>
    );
  }

  function renderCli() {
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
          <button
            type="button"
            onClick={() => setStep("server")}
            className={`${primaryButton} mt-2.5`}
          >
            {t("onboarding.cli.continue")}
          </button>
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
              <p className="mt-2.5 text-[12px] leading-relaxed text-secondary">
                {t("onboarding.cli.outdatedBody")}
              </p>
              {installGuidance()}
            </>
          ) : (
            <button
              type="button"
              onClick={() => void recheckCli()}
              className={`${primaryButton} mt-2.5`}
            >
              <RefreshIcon size={12} />
              {t("onboarding.cli.verify")}
            </button>
          )}
        </div>
      );
    }

    return (
      <div>
        <p className="text-[12px] leading-relaxed text-secondary">
          {t("onboarding.cli.missingBody")}
        </p>
        {installGuidance()}
      </div>
    );
  }

  function renderServer() {
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
          <button
            type="button"
            onClick={() => void ensureServer()}
            className={`${primaryButton} mt-2.5`}
          >
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
          <button
            type="button"
            onClick={() => setStep("models")}
            className={`${primaryButton} mt-2.5`}
          >
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

  function renderModels() {
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
              <li
                key={recommendation.alias}
                className="rounded-[8px] border border-line bg-canvas px-3 py-2"
              >
                <div className="flex items-center gap-3">
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="truncate text-[12px] font-medium text-primary">
                        {recommendation.alias}
                      </span>
                      <KindBadge kind={model?.kind ?? fallbackKind} />
                    </div>
                    <p className="mt-0.5 text-[10.5px] tabular-nums text-tertiary">
                      {formatSize(sizeMb, i18n.language)} ·{" "}
                      {running
                        ? t("onboarding.models.downloading", {
                            percent: job ? jobPercent(job) : 0,
                          })
                        : cached
                          ? t("onboarding.models.downloaded")
                          : t("onboarding.models.notDownloaded")}
                    </p>
                  </div>
                  {running ? (
                    <button
                      type="button"
                      onClick={() => void handleCancel(recommendation.alias)}
                      className={secondaryButton}
                    >
                      {t("onboarding.models.cancel")}
                    </button>
                  ) : cached ? (
                    <span className="inline-flex shrink-0 items-center gap-1 text-[11px] text-success">
                      <CheckIcon size={12} />
                      {t("onboarding.models.downloaded")}
                    </span>
                  ) : (
                    <button
                      type="button"
                      onClick={() => void handleDownload(recommendation.alias)}
                      className={primaryButton}
                    >
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
                    aria-label={t("onboarding.models.downloading", {
                      percent: jobPercent(job),
                    })}
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
        <button type="button" onClick={() => setStep("ready")} className={`${primaryButton} mt-3`}>
          {t("onboarding.models.continue")}
        </button>
      </div>
    );
  }

  function renderReady() {
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
                onClick={() => onChooseWorkspace(value)}
                className="flex items-center gap-2.5 rounded-[8px] border border-line bg-canvas px-3 py-2.5 text-left transition-colors hover:border-line-strong hover:bg-hover"
              >
                <Icon size={16} className="shrink-0 text-accent" />
                <span className="text-[12px] font-medium text-primary">{t(`rail.${value}`)}</span>
              </button>
            );
          })}
        </div>
      </div>
    );
  }

  function renderStep(name: StepName) {
    switch (name) {
      case "cli":
        return renderCli();
      case "server":
        return renderServer();
      case "models":
        return renderModels();
      case "ready":
        return renderReady();
    }
  }

  return (
    <Modal open title={t("onboarding.title")} onClose={onClose} size="lg">
      <p className="text-[11.5px] leading-relaxed text-tertiary">{t("onboarding.subtitle")}</p>

      <ol className="mt-3 flex flex-col gap-1.5">
        {steps.map((name, index) => {
          const done = index < stepIndex;
          const current = index === stepIndex;
          const summary = done ? summaryFor(name) : null;

          return (
            <li
              key={name}
              className={cx(
                "rounded-[10px] border px-3.5 py-3",
                current ? "border-line bg-canvas" : "border-transparent",
              )}
            >
              <div className="flex items-center gap-2.5">
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
                  <span className="shrink-0 truncate font-mono text-[10px] text-tertiary">
                    {summary}
                  </span>
                ) : null}
              </div>
              {current ? <div className="mt-3 pl-[30px]">{renderStep(name)}</div> : null}
            </li>
          );
        })}
      </ol>

      <div className="mt-4 flex justify-center border-t border-line pt-3">
        <button
          type="button"
          onClick={onClose}
          className="text-[11.5px] text-tertiary transition-colors hover:text-primary"
        >
          {t("onboarding.skip")}
        </button>
      </div>
    </Modal>
  );
}

const primaryButton =
  "inline-flex h-8 items-center gap-1.5 rounded-[7px] bg-accent px-3 text-[12px] font-medium text-accent-contrast transition-colors hover:bg-accent-hover disabled:opacity-50";

const secondaryButton =
  "inline-flex h-8 items-center gap-1.5 rounded-[7px] border border-line bg-elevated px-3 text-[12px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary disabled:opacity-50";

const errorBox =
  "flex items-start gap-2 rounded-[7px] border border-line bg-subtle px-2.5 py-2 text-[11.5px] text-secondary";
