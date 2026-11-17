import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ContextList } from "../components/ContextList";
import { DetailPane } from "../components/DetailPane";
import { Modal } from "../components/Modal";
import { ParamsSection } from "../components/ParamsSection";
import { AlertIcon, ChevronDownIcon, CopyIcon, RefreshIcon } from "../components/Icon";
import { IconButton } from "../components/IconButton";
import { Spinner } from "../components/Spinner";
import { useToast } from "../components/Toast";
import { languages, type Language } from "../i18n/resources";
import { copyText } from "../lib/copy";
import { cx } from "../lib/cx";
import {
  cacheInfo,
  clearCache,
  configSet,
  diagnosticsReport,
  diagnosticsSnapshot,
  loadedModels,
  restartServer,
  serverGetPreferences,
  serverSetPreferences,
  setCacheDirectory,
  startServer,
  stopServer,
  toFoundryCommandError,
  unloadModel,
  type CacheInfo,
  type ConfigShow,
  type FoundryCommandError,
  type LoadedModel,
  type ServerPreferences,
  type ServerSnapshot,
} from "../lib/foundry";
import { readStored, writeStored } from "../lib/storage";
import { setSpeechStorageSettings, speechStorageSettings } from "../lib/speech";
import {
  themes,
  type AccentPreference,
  type DensityPreference,
  type ThemePreference,
  type TypographyPreference,
} from "../lib/theme";
import { formatBytes } from "./models/format";

const sections = ["server", "models", "speech", "generation", "appearance", "diagnostics", "onboarding", "about"] as const;
type SettingsSection = (typeof sections)[number];

const languageNames: Record<Language, string> = {
  en: "English",
  "pt-BR": "Português (Brasil)",
  es: "Español",
};

const accentOptions = [
  { value: "blue", labelKey: "settings.appearance.accentBlue" },
  { value: "green", labelKey: "settings.appearance.accentGreen" },
  { value: "purple", labelKey: "settings.appearance.accentPurple" },
  { value: "orange", labelKey: "settings.appearance.accentOrange" },
] as const;

const densityOptions = [
  { value: "compact", labelKey: "settings.appearance.densityCompact" },
  { value: "normal", labelKey: "settings.appearance.densityNormal" },
  { value: "comfortable", labelKey: "settings.appearance.densityComfortable" },
] as const;

const typographyOptions = [
  { value: "system", labelKey: "settings.appearance.typographySystem" },
  { value: "humanist", labelKey: "settings.appearance.typographyHumanist" },
  { value: "serif", labelKey: "settings.appearance.typographySerif" },
] as const;

const settingsStorageKey = "foundryui.server.settings";

type ConfigField = {
  id: string;
  key: string;
  label: string;
  control: "text" | "select" | "port";
  options?: Array<{ value: string; label: string }>;
};

type SettingsWorkspaceProps = {
  theme: ThemePreference;
  onThemeChange: (theme: ThemePreference) => void;
  accent: AccentPreference;
  onAccentChange: (accent: AccentPreference) => void;
  density: DensityPreference;
  onDensityChange: (density: DensityPreference) => void;
  typography: TypographyPreference;
  onTypographyChange: (typography: TypographyPreference) => void;
  language: Language;
  onLanguageChange: (language: Language) => void;
  onRestartOnboarding: () => void;
};

type SavedServerSettings = {
  port: number | null;
  keepRunning: boolean;
  startOnOpen: boolean;
};

function SettingRow({ title, help, children }: { title: string; help: string; children: ReactNode }) {
  return (
    <div className="flex items-start justify-between gap-5 border-b border-line py-4 max-[980px]:flex-col max-[980px]:gap-2.5 last:border-b-0">
      <div className="min-w-0">
        <p className="text-[12.5px] font-medium text-primary">{title}</p>
        <p className="mt-0.5 max-w-[44ch] text-[11.5px] leading-relaxed text-secondary">{help}</p>
      </div>
      <div className="w-full max-w-[460px] shrink-0 pt-0.5 max-[980px]:max-w-none max-[980px]:pt-0">{children}</div>
    </div>
  );
}

export function SettingsWorkspace({
  theme,
  onThemeChange,
  accent,
  onAccentChange,
  density,
  onDensityChange,
  typography,
  onTypographyChange,
  language,
  onLanguageChange,
  onRestartOnboarding,
}: SettingsWorkspaceProps) {
  const { t, i18n } = useTranslation();
  const { push } = useToast();
  const [section, setSection] = useState<SettingsSection>("server");
  const [banner, setBanner] = useState<FoundryCommandError | null>(null);
  const [serverPrefs, setServerPrefs] = useState<ServerPreferences | null>(null);
  const [startOnOpen, setStartOnOpen] = useState(false);
  const [portDraft, setPortDraft] = useState("");
  const [serverSnapshot, setServerSnapshot] = useState<ServerSnapshot | null>(null);
  const [savingServer, setSavingServer] = useState(false);
  const [cache, setCache] = useState<CacheInfo | null>(null);
  const [cachePathDraft, setCachePathDraft] = useState("");
  const [updatingCachePath, setUpdatingCachePath] = useState(false);
  const [clearingCache, setClearingCache] = useState(false);
  const [loadedModelsList, setLoadedModelsList] = useState<LoadedModel[]>([]);
  const [unloadingAlias, setUnloadingAlias] = useState<string | null>(null);
  const [clearAfterUnloadOpen, setClearAfterUnloadOpen] = useState(false);
  const [unloadingForClear, setUnloadingForClear] = useState(false);
  const [logs, setLogs] = useState("");
  const [statusJson, setStatusJson] = useState("");
  const [reportOutput, setReportOutput] = useState("");
  const [speechStoragePathDraft, setSpeechStoragePathDraft] = useState("");
  const [speechStorageDefault, setSpeechStorageDefault] = useState("");
  const [speechStorageCustom, setSpeechStorageCustom] = useState(false);
  const [savingSpeechStorage, setSavingSpeechStorage] = useState(false);
  const [diagnosticsBusy, setDiagnosticsBusy] = useState(false);
  const [configDrafts, setConfigDrafts] = useState<Record<string, string>>({});
  const [updatingConfigKey, setUpdatingConfigKey] = useState<string | null>(null);
  const [cliInfoText, setCliInfoText] = useState("");

  const sectionLabels: Record<SettingsSection, string> = {
    server: t("settings.sections.server"),
    models: t("settings.sections.models"),
    speech: t("settings.sections.speech"),
    generation: t("settings.sections.generation"),
    appearance: t("settings.sections.appearance"),
    diagnostics: t("settings.sections.diagnostics"),
    onboarding: t("settings.sections.onboarding"),
    about: t("settings.sections.about"),
  };

  const themeLabels: Record<ThemePreference, string> = {
    system: t("settings.appearance.system"),
    light: t("settings.appearance.light"),
    dark: t("settings.appearance.dark"),
  };

  const configKeys = useMemo<ConfigField[]>(
    () => [
      { id: "port", key: "port", label: t("settings.config.port"), control: "port" },
      {
        id: "idle-timeout-minutes",
        key: "idle-timeout-minutes",
        label: t("settings.config.idleTimeout"),
        control: "select",
        options: [
          { value: "disabled", label: t("settings.config.idleTimeoutOptions.disabled") },
          { value: "1", label: t("settings.config.idleTimeoutOptions.1m") },
          { value: "5", label: t("settings.config.idleTimeoutOptions.5m") },
          { value: "10", label: t("settings.config.idleTimeoutOptions.10m") },
          { value: "15", label: t("settings.config.idleTimeoutOptions.15m") },
          { value: "30", label: t("settings.config.idleTimeoutOptions.30m") },
          { value: "60", label: t("settings.config.idleTimeoutOptions.60m") },
          { value: "120", label: t("settings.config.idleTimeoutOptions.120m") },
        ],
      },
      {
        id: "log-level",
        key: "log-level",
        label: t("settings.config.logLevel"),
        control: "select",
        options: [
          { value: "trace", label: t("settings.config.logLevelOptions.trace") },
          { value: "debug", label: t("settings.config.logLevelOptions.debug") },
          { value: "info", label: t("settings.config.logLevelOptions.info") },
          { value: "warning", label: t("settings.config.logLevelOptions.warning") },
          { value: "error", label: t("settings.config.logLevelOptions.error") },
        ],
      },
    ],
    [t],
  );

  const refreshDiagnostics = useCallback(async () => {
    setDiagnosticsBusy(true);

    try {
      const snapshot = await diagnosticsSnapshot();

      setCliInfoText(`${snapshot.cli.version}\n${snapshot.cli.path}`);
      setServerSnapshot(snapshot.server);
      setCache(snapshot.cache);
      setCachePathDraft(snapshot.cache.location);
      setLogs(snapshot.logs);
      setStatusJson(JSON.stringify(snapshot.status, null, 2));
      setConfigDrafts((current) => ({ ...toConfigDrafts(snapshot.config), ...current }));
      setLoadedModelsList(snapshot.loaded.models);
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setDiagnosticsBusy(false);
    }
  }, []);

  useEffect(() => {
    Promise.all([serverGetPreferences(), refreshDiagnostics(), speechStorageSettings()])
      .then(([preferences, _, speechStorage]) => {
        const saved = readSavedServerSettings();

        setServerPrefs(preferences);
        setStartOnOpen(saved?.startOnOpen ?? false);
        setPortDraft((saved?.port ?? preferences.port ?? "").toString());
        setSpeechStoragePathDraft(speechStorage.directory);
        setSpeechStorageDefault(speechStorage.defaultDirectory);
        setSpeechStorageCustom(speechStorage.custom);
      })
      .catch((error) => {
        setBanner(toFoundryCommandError(error));
      });
  }, [refreshDiagnostics]);

  async function saveServerSettings() {
    if (!serverPrefs || savingServer) {
      return;
    }

    setSavingServer(true);

    const parsedPort = Number(portDraft.trim());
    const port = Number.isFinite(parsedPort) && parsedPort > 0 ? Math.round(parsedPort) : null;

    try {
      const saved = await serverSetPreferences({
        ...serverPrefs,
        port,
        keepRunning: serverPrefs.keepRunning,
      });

      setServerPrefs(saved);
      setPortDraft(saved.port?.toString() ?? "");
      writeSavedServerSettings({
        port: saved.port,
        keepRunning: saved.keepRunning,
        startOnOpen,
      });
      setBanner(null);
      push(t("settings.server.saved"), "neutral");
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setSavingServer(false);
    }
  }

  async function toggleKeepRunning(next: boolean) {
    if (!serverPrefs) {
      return;
    }

    try {
      const saved = await serverSetPreferences({ ...serverPrefs, keepRunning: next });

      setServerPrefs(saved);
      writeSavedServerSettings({
        port: saved.port,
        keepRunning: saved.keepRunning,
        startOnOpen,
      });
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    }
  }

  function toggleStartOnOpen(next: boolean) {
    setStartOnOpen(next);

    if (serverPrefs) {
      writeSavedServerSettings({
        port: serverPrefs.port,
        keepRunning: serverPrefs.keepRunning,
        startOnOpen: next,
      });
    }
  }

  async function performServerAction(action: "start" | "stop" | "restart") {
    try {
      const snapshot =
        action === "start" ? await startServer() : action === "stop" ? await stopServer() : await restartServer();

      setServerSnapshot(snapshot);
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    }
  }

  async function chooseCacheDirectory() {
    const selected = await open({
      directory: true,
      multiple: false,
      defaultPath: cachePathDraft.trim() || cache?.location || "/Volumes",
    });

    if (typeof selected === "string") {
      setCachePathDraft(selected);
    }
  }

  async function applyCacheDirectory() {
    if (!cachePathDraft.trim() || updatingCachePath) {
      return;
    }

    setUpdatingCachePath(true);

    try {
      const next = await setCacheDirectory(cachePathDraft.trim());

      setCache(next);
      setCachePathDraft(next.location);
      setBanner(null);
      push(t("settings.models.cacheSaved"), "neutral");
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setUpdatingCachePath(false);
    }
  }

  async function chooseSpeechStorageDirectory() {
    const selected = await open({
      directory: true,
      multiple: false,
      defaultPath: speechStoragePathDraft.trim() || speechStorageDefault || "/Volumes",
    });

    if (typeof selected === "string") {
      setSpeechStoragePathDraft(selected);
    }
  }

  async function applySpeechStorageDirectory() {
    if (!speechStoragePathDraft.trim() || savingSpeechStorage) {
      return;
    }

    setSavingSpeechStorage(true);

    try {
      const next = await setSpeechStorageSettings(speechStoragePathDraft.trim());

      setSpeechStoragePathDraft(next.directory);
      setSpeechStorageDefault(next.defaultDirectory);
      setSpeechStorageCustom(next.custom);
      setBanner(null);
      push(t("settings.speech.saved"), "neutral");
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setSavingSpeechStorage(false);
    }
  }

  async function resetSpeechStorageDirectory() {
    if (savingSpeechStorage) {
      return;
    }

    setSavingSpeechStorage(true);

    try {
      const next = await setSpeechStorageSettings(null);

      setSpeechStoragePathDraft(next.directory);
      setSpeechStorageDefault(next.defaultDirectory);
      setSpeechStorageCustom(next.custom);
      setBanner(null);
      push(t("settings.speech.saved"), "neutral");
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setSavingSpeechStorage(false);
    }
  }

  async function clearAllCache() {
    if (clearingCache) {
      return;
    }

    setClearingCache(true);

    try {
      await clearCache();
      const [next, loadedSummary] = await Promise.all([cacheInfo(), loadedModels()]);

      setCache(next);
      setCachePathDraft(next.location);
      setLoadedModelsList(loadedSummary.models);
      setBanner(null);
    } catch (error) {
      const failure = toFoundryCommandError(error);

      if (failure.message.includes("cannot remove a loaded model from cache")) {
        try {
          const summary = await loadedModels();

          setLoadedModelsList(summary.models);

          if (summary.models.length > 0) {
            setClearAfterUnloadOpen(true);
          } else {
            setBanner(failure);
          }
        } catch {
          setBanner(failure);
        }
      } else {
        setBanner(failure);
      }
    } finally {
      setClearingCache(false);
    }
  }

  async function unloadThenClearCache() {
    if (unloadingForClear) {
      return;
    }

    setUnloadingForClear(true);

    try {
      for (const model of loadedModelsList) {
        setUnloadingAlias(model.alias);
        await unloadModel(model.alias);
      }

      await clearCache();

      const [next, loadedSummary] = await Promise.all([cacheInfo(), loadedModels()]);

      setCache(next);
      setCachePathDraft(next.location);
      setLoadedModelsList(loadedSummary.models);
      setClearAfterUnloadOpen(false);
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setUnloadingForClear(false);
      setUnloadingAlias(null);
    }
  }

  async function unloadLoaded(alias: string) {
    if (unloadingAlias) {
      return;
    }

    setUnloadingAlias(alias);

    try {
      await unloadModel(alias);
      const summary = await loadedModels();

      setLoadedModelsList(summary.models);
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setUnloadingAlias(null);
    }
  }

  async function copyLogs() {
    const copied = await copyText(logs);

    push(t(copied ? "settings.diagnostics.logsCopied" : "settings.diagnostics.logsCopyFailed"), copied ? "neutral" : "danger");
  }

  async function applyConfigKey(key: string) {
    const value = configDrafts[key] ?? "";

    setUpdatingConfigKey(key);

    try {
      const next = await configSet(key, value);

      setConfigDrafts(toConfigDrafts(next));
      setBanner(null);
      push(t("settings.config.saved"), "neutral");
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setUpdatingConfigKey(null);
    }
  }

  async function runIssueReport() {
    try {
      setReportOutput(await diagnosticsReport());
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    }
  }

  return (
    <>
      <ContextList title={t("settings.listTitle")}>
        <ul className="flex flex-col gap-1 pb-2" role="listbox" aria-label={t("settings.listTitle")}>
          {sections.map((value) => (
            <li key={value}>
              <button
                type="button"
                onClick={() => setSection(value)}
                aria-current={section === value ? "page" : undefined}
                className={cx(
                  "flex w-full items-center justify-between rounded-[var(--item-radius)] border px-[var(--item-pad-x)] py-[var(--item-pad-y)] text-left transition-colors",
                  section === value
                    ? "border-line-strong bg-elevated shadow-[var(--shadow-thumb)]"
                    : "border-transparent hover:border-line hover:bg-hover",
                )}
              >
                <span className="truncate text-[12px] font-medium text-primary">{sectionLabels[value]}</span>
              </button>
            </li>
          ))}
        </ul>
      </ContextList>

      <DetailPane
        title={t("settings.title")}
        subtitle={sectionLabels[section]}
        actions={
          section === "diagnostics" ? (
            <IconButton label={t("settings.diagnostics.refresh")} onClick={() => void refreshDiagnostics()}>
              {diagnosticsBusy ? <Spinner size={13} /> : <RefreshIcon size={14} />}
            </IconButton>
          ) : undefined
        }
      >
        <div className="mx-auto w-full max-w-[920px] px-4 py-4 sm:px-5 sm:py-5">

          {banner ? (
            <div className="mb-4 flex items-center gap-2 rounded-[8px] border border-line bg-subtle px-3 py-2 text-[12px] text-danger">
              <AlertIcon size={14} className="shrink-0" />
              <span>{banner.message}</span>
            </div>
          ) : null}

          {section === "server" ? (
            <>
              <SettingRow title={t("settings.server.portLabel")} help={t("settings.server.portHelp")}>
                <div className="flex flex-wrap items-center gap-2">
                  <input
                    value={portDraft}
                    onChange={(event) => setPortDraft(event.target.value)}
                    placeholder={t("settings.server.auto")}
                    className="h-7 w-full max-w-[140px] rounded-md border border-line bg-elevated px-2 text-[12px] text-primary outline-none"
                  />
                  <button
                    type="button"
                    onClick={() => void saveServerSettings()}
                    disabled={savingServer}
                    className="inline-flex h-7 items-center rounded-[7px] border border-line px-2.5 text-[11.5px] text-secondary transition-colors hover:bg-hover hover:text-primary"
                  >
                    {savingServer ? t("settings.server.saving") : t("settings.server.save")}
                  </button>
                </div>
              </SettingRow>

              <SettingRow title={t("settings.server.startOnOpenLabel")} help={t("settings.server.startOnOpenHelp")}>
                <Toggle checked={startOnOpen} onChange={toggleStartOnOpen} />
              </SettingRow>

              <SettingRow title={t("settings.server.keepRunningLabel")} help={t("settings.server.keepRunningHelp")}>
                <Toggle checked={serverPrefs?.keepRunning ?? false} onChange={(next) => void toggleKeepRunning(next)} />
              </SettingRow>

              <div className="flex flex-wrap items-center gap-2 py-4">
                <button type="button" onClick={() => void performServerAction("start")} className={secondaryButton}>
                  {t("settings.server.start")}
                </button>
                <button type="button" onClick={() => void performServerAction("stop")} className={secondaryButton}>
                  {t("settings.server.stop")}
                </button>
                <button type="button" onClick={() => void performServerAction("restart")} className={secondaryButton}>
                  {t("settings.server.restart")}
                </button>
              </div>

              <dl className="grid grid-cols-1 gap-y-1.5 text-[11.5px] text-secondary sm:grid-cols-[120px_1fr] sm:gap-y-1">
                <dt className="text-tertiary">{t("settings.server.endpoint")}</dt>
                <dd className="break-all font-mono">{serverSnapshot?.endpoint ?? "—"}</dd>
                <dt className="text-tertiary">{t("settings.server.port")}</dt>
                <dd>{serverSnapshot?.port ?? "—"}</dd>
                <dt className="text-tertiary">PID</dt>
                <dd>{serverSnapshot?.pid ?? "—"}</dd>
              </dl>
            </>
          ) : null}

          {section === "models" ? (
            <>
              <SettingRow title={t("settings.models.cacheLabel")} help={t("settings.models.cacheHelp")}>
                <div className="flex flex-wrap items-center gap-2">
                  <input
                    value={cachePathDraft}
                    onChange={(event) => setCachePathDraft(event.target.value)}
                    className="h-7 w-full max-w-[360px] rounded-md border border-line bg-elevated px-2 text-[11.5px] text-primary outline-none"
                  />
                  <button type="button" onClick={() => void chooseCacheDirectory()} className={secondaryButton}>
                    {t("settings.models.browse")}
                  </button>
                  <button type="button" onClick={() => void applyCacheDirectory()} className={secondaryButton}>
                    {updatingCachePath ? t("settings.models.saving") : t("settings.models.save")}
                  </button>
                </div>
              </SettingRow>

              <SettingRow title={t("settings.models.cacheSizeLabel")} help={t("settings.models.cacheSizeHelp")}>
                <span className="text-[12px] text-secondary">{cache ? formatBytes(cache.totalBytes, i18n.language) : "—"}</span>
              </SettingRow>

              <SettingRow title={t("settings.models.loadedLabel")} help={t("settings.models.loadedHelp")}>
                <div className="w-full max-w-[360px] space-y-1.5">
                  {!loadedModelsList.length ? (
                    <span className="text-[11.5px] text-secondary">{t("settings.models.noneLoaded")}</span>
                  ) : (
                    loadedModelsList.map((model) => (
                      <div key={model.alias} className="flex items-center justify-between gap-2 rounded-md border border-line bg-subtle px-2 py-1.5">
                        <span className="truncate text-[11.5px] text-secondary">{model.alias}</span>
                        <button
                          type="button"
                          onClick={() => void unloadLoaded(model.alias)}
                          disabled={Boolean(unloadingAlias)}
                          className={secondaryButton}
                        >
                          {unloadingAlias === model.alias ? t("settings.models.unloading") : t("settings.models.unload")}
                        </button>
                      </div>
                    ))
                  )}
                </div>
              </SettingRow>

              <SettingRow title={t("settings.models.clearLabel")} help={t("settings.models.clearHelp")}>
                <button type="button" onClick={() => void clearAllCache()} className={secondaryButton}>
                  {clearingCache ? t("settings.models.clearing") : t("settings.models.clear")}
                </button>
              </SettingRow>

              <p className="pt-3 text-[11px] text-secondary">{t("settings.models.scopeWarning")}</p>
            </>
          ) : null}

          {section === "speech" ? (
            <>
              <SettingRow title={t("settings.speech.storageLabel")} help={t("settings.speech.storageHelp")}>
                <div className="flex flex-wrap items-center gap-2">
                  <input
                    value={speechStoragePathDraft}
                    onChange={(event) => setSpeechStoragePathDraft(event.target.value)}
                    className="h-7 w-full max-w-[360px] rounded-md border border-line bg-elevated px-2 text-[11.5px] text-primary outline-none"
                  />
                  <button type="button" onClick={() => void chooseSpeechStorageDirectory()} className={secondaryButton}>
                    {t("settings.speech.browse")}
                  </button>
                  <button type="button" onClick={() => void applySpeechStorageDirectory()} className={secondaryButton}>
                    {savingSpeechStorage ? t("settings.speech.saving") : t("settings.speech.save")}
                  </button>
                </div>
              </SettingRow>

              <SettingRow title={t("settings.speech.defaultLabel")} help={t("settings.speech.defaultHelp")}>
                <div className="flex flex-wrap items-center gap-2">
                  <span className="w-full max-w-[360px] truncate text-[11.5px] text-secondary">{speechStorageDefault || "—"}</span>
                  <button
                    type="button"
                    onClick={() => void resetSpeechStorageDirectory()}
                    disabled={!speechStorageCustom || savingSpeechStorage}
                    className={secondaryButton}
                  >
                    {t("settings.speech.reset")}
                  </button>
                </div>
              </SettingRow>
            </>
          ) : null}

          {section === "generation" ? (
            <div className="py-4">
              <ParamsSection scope="global" help={t("settings.generation.help")} />
            </div>
          ) : null}

          {section === "appearance" ? (
            <>
              <SettingRow title={t("settings.appearance.themeLabel")} help={t("settings.appearance.themeHelp")}>
                <SegmentedSelect
                  value={theme}
                  onChange={(next) => onThemeChange(next as ThemePreference)}
                  options={themes.map((value) => ({ value, label: themeLabels[value] }))}
                />
              </SettingRow>

              <SettingRow title={t("settings.appearance.accentLabel")} help={t("settings.appearance.accentHelp")}>
                <SegmentedSelect
                  value={accent}
                  onChange={(next) => onAccentChange(next as AccentPreference)}
                  options={accentOptions.map((option) => ({ value: option.value, label: t(option.labelKey) }))}
                />
              </SettingRow>

              <SettingRow title={t("settings.appearance.densityLabel")} help={t("settings.appearance.densityHelp")}>
                <SegmentedSelect
                  value={density}
                  onChange={(next) => onDensityChange(next as DensityPreference)}
                  options={densityOptions.map((option) => ({ value: option.value, label: t(option.labelKey) }))}
                />
              </SettingRow>

              <SettingRow title={t("settings.appearance.typographyLabel")} help={t("settings.appearance.typographyHelp")}>
                <SegmentedSelect
                  value={typography}
                  onChange={(next) => onTypographyChange(next as TypographyPreference)}
                  options={typographyOptions.map((option) => ({ value: option.value, label: t(option.labelKey) }))}
                />
              </SettingRow>

              <SettingRow title={t("settings.language.label")} help={t("settings.language.help")}>
                <div className="relative">
                  <select
                    aria-label={t("settings.language.label")}
                    value={language}
                    onChange={(event) => onLanguageChange(event.target.value as Language)}
                    className="h-7 appearance-none rounded-md border border-line bg-elevated pl-2.5 pr-7 text-[12px] text-primary outline-none transition-colors hover:border-line-strong"
                  >
                    {languages.map((value) => (
                      <option key={value} value={value}>
                        {languageNames[value]}
                      </option>
                    ))}
                  </select>
                  <ChevronDownIcon
                    size={14}
                    className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 text-tertiary"
                  />
                </div>
              </SettingRow>
            </>
          ) : null}

          {section === "diagnostics" ? (
            <div className="flex flex-col gap-4 py-2">
              <div>
                <p className="mb-1 text-[12px] font-medium text-primary">{t("settings.diagnostics.cli")}</p>
                <pre className="overflow-x-auto rounded-[8px] border border-line bg-subtle px-3 py-2 text-[11px] text-secondary">{cliInfoText || "—"}</pre>
              </div>

              <div>
                <p className="mb-1 text-[12px] font-medium text-primary">{t("settings.diagnostics.status")}</p>
                <pre className="max-h-[180px] overflow-auto rounded-[8px] border border-line bg-subtle px-3 py-2 text-[11px] text-secondary">{statusJson || "{}"}</pre>
              </div>

              <div>
                <div className="mb-1 flex items-center justify-between">
                  <p className="text-[12px] font-medium text-primary">{t("settings.diagnostics.logs")}</p>
                  <button type="button" onClick={() => void copyLogs()} className={secondaryButton}>
                    <CopyIcon size={12} />
                    {t("settings.diagnostics.copyLogs")}
                  </button>
                </div>
                <pre className="max-h-[220px] overflow-auto rounded-[8px] border border-line bg-subtle px-3 py-2 text-[11px] text-secondary">{logs || "—"}</pre>
              </div>

              <div>
                <p className="mb-2 text-[12px] font-medium text-primary">{t("settings.config.title")}</p>
                <div className="space-y-2">
                  {configKeys.map((item) => (
                    <div key={item.id} className="flex flex-col gap-1.5 sm:flex-row sm:items-center sm:gap-2">
                      <label className="text-[11.5px] text-secondary sm:w-[140px] sm:shrink-0">{item.label}</label>
                      {item.control === "port" ? (
                        <div className="flex flex-1 flex-wrap items-center gap-2">
                          <label className="inline-flex items-center gap-1.5 text-[11.5px] text-secondary">
                            <input
                              type="checkbox"
                              checked={isPortAuto(configDrafts[item.key] ?? "")}
                              onChange={(event) =>
                                setConfigDrafts((current) => ({
                                  ...current,
                                  [item.key]: event.target.checked ? "auto" : "",
                                }))
                              }
                            />
                            {t("settings.config.auto")}
                          </label>
                          {isPortAuto(configDrafts[item.key] ?? "") ? null : (
                            <input
                              value={configDrafts[item.key] ?? ""}
                              inputMode="numeric"
                              onChange={(event) =>
                                setConfigDrafts((current) => ({
                                  ...current,
                                  [item.key]: event.target.value.replace(/\D+/g, ""),
                                }))
                              }
                              onBlur={() => {
                                const value = (configDrafts[item.key] ?? "").trim();

                                if (!value) {
                                  setConfigDrafts((current) => ({ ...current, [item.key]: "auto" }));
                                }
                              }}
                              className="h-7 w-full max-w-[160px] rounded-md border border-line bg-elevated px-2 text-[11.5px] text-primary outline-none"
                            />
                          )}
                        </div>
                      ) : item.control === "select" ? (
                        <div className="relative flex-1">
                          <select
                            value={
                              item.key === "idle-timeout-minutes"
                                ? normalizeIdleTimeoutValue(configDrafts[item.key] ?? "")
                                : normalizeLogLevelValue(configDrafts[item.key] ?? "")
                            }
                            onChange={(event) =>
                              setConfigDrafts((current) => ({ ...current, [item.key]: event.target.value }))
                            }
                            className="h-7 w-full min-w-0 appearance-none rounded-md border border-line bg-elevated pl-2 pr-7 text-[11.5px] text-primary outline-none transition-colors hover:border-line-strong"
                          >
                            {item.options?.map((option) => (
                              <option key={option.value} value={option.value}>
                                {option.label}
                              </option>
                            ))}
                          </select>
                          <ChevronDownIcon
                            size={14}
                            className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 text-tertiary"
                          />
                        </div>
                      ) : (
                        <input
                          value={configDrafts[item.key] ?? ""}
                          onChange={(event) =>
                            setConfigDrafts((current) => ({ ...current, [item.key]: event.target.value }))
                          }
                          className="h-7 flex-1 rounded-md border border-line bg-elevated px-2 text-[11.5px] text-primary outline-none"
                        />
                      )}
                      <button
                        type="button"
                        onClick={() => void applyConfigKey(item.key)}
                        disabled={
                          updatingConfigKey === item.key ||
                          (item.key === "port" && !isPortAuto(configDrafts.port ?? "") && !(configDrafts.port ?? "").trim())
                        }
                        className={secondaryButton}
                      >
                        {updatingConfigKey === item.key ? t("settings.config.saving") : t("settings.config.save")}
                      </button>
                    </div>
                  ))}
                </div>
              </div>

              <div>
                <button type="button" onClick={() => void runIssueReport()} className={secondaryButton}>
                  {t("settings.diagnostics.openIssue")}
                </button>
                {reportOutput ? (
                  <pre className="mt-2 max-h-[200px] overflow-auto rounded-[8px] border border-line bg-subtle px-3 py-2 text-[11px] text-secondary">{reportOutput}</pre>
                ) : null}
              </div>
            </div>
          ) : null}

          {section === "onboarding" ? (
            <SettingRow title={t("settings.onboarding.label")} help={t("settings.onboarding.help")}>
              <button
                type="button"
                onClick={onRestartOnboarding}
                className="inline-flex h-[var(--control-size)] items-center rounded-[var(--item-radius)] border border-line bg-elevated px-3 text-[12px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary"
              >
                {t("settings.onboarding.run")}
              </button>
            </SettingRow>
          ) : null}

          {section === "about" ? (
            <div className="flex flex-col py-4">
              <p className="text-[13px] font-semibold text-primary">FoundryUI</p>
              <p className="mt-0.5 text-[11.5px] text-tertiary">{t("settings.about.version", { version: __APP_VERSION__ })}</p>
              <p className="mt-3 max-w-[46ch] text-[12px] leading-relaxed text-secondary">{t("settings.about.body")}</p>
              <p className="mt-1.5 text-[11.5px] text-tertiary">{t("settings.about.stack")}</p>
            </div>
          ) : null}
        </div>
      </DetailPane>

      <Modal
        open={clearAfterUnloadOpen}
        title={t("settings.models.clearNeedsUnloadTitle")}
        onClose={() => {
          if (!unloadingForClear) {
            setClearAfterUnloadOpen(false);
          }
        }}
      >
        <div className="space-y-3">
          <p className="text-[12px] leading-relaxed text-secondary">{t("settings.models.clearNeedsUnloadBody")}</p>
          <div className="rounded-md border border-line bg-subtle px-3 py-2 text-[11.5px] text-secondary">
            {loadedModelsList.map((model) => model.alias).join(" · ")}
          </div>
          <div className="flex justify-end gap-2">
            <button
              type="button"
              onClick={() => setClearAfterUnloadOpen(false)}
              disabled={unloadingForClear}
              className={secondaryButton}
            >
              {t("common.cancel")}
            </button>
            <button
              type="button"
              onClick={() => void unloadThenClearCache()}
              disabled={unloadingForClear}
              className={secondaryButton}
            >
              {unloadingForClear ? t("settings.models.unloading") : t("settings.models.clearAfterUnload")}
            </button>
          </div>
        </div>
      </Modal>
    </>
  );
}

function SegmentedSelect({
  value,
  onChange,
  options,
}: {
  value: string;
  onChange: (value: string) => void;
  options: Array<{ value: string; label: string }>;
}) {
  return (
    <div className="inline-flex max-w-full flex-wrap items-center gap-0.5 rounded-[7px] bg-subtle p-0.5">
      {options.map((option) => {
        const selected = option.value === value;

        return (
          <button
            key={option.value}
            type="button"
            onClick={() => onChange(option.value)}
            className={cx(
              "h-[24px] rounded-[5px] px-2.5 text-[11.5px] font-medium transition-colors",
              selected ? "bg-elevated text-primary shadow-[var(--shadow-thumb)]" : "text-secondary hover:text-primary",
            )}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}

function Toggle({ checked, onChange }: { checked: boolean; onChange: (next: boolean) => void }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className={cx(
        "relative inline-flex h-6 w-11 items-center rounded-full border transition-colors",
        checked ? "border-accent bg-accent" : "border-line bg-subtle",
      )}
    >
      <span
        className={cx(
          "inline-flex h-4.5 w-4.5 rounded-full bg-white transition-transform",
          checked ? "translate-x-[21px]" : "translate-x-[3px]",
        )}
      />
    </button>
  );
}

function toConfigDrafts(config: ConfigShow | null): Record<string, string> {
  if (!config) {
    return {};
  }

  const byKey = new Map(config.settings.map((setting) => [setting.key, setting.value]));

  return {
    port: valueToInput(byKey.get("port")),
    "idle-timeout-minutes": valueToInput(
      byKey.get("idle-timeout-minutes") ?? byKey.get("idle-timeout"),
    ),
    "log-level": valueToInput(byKey.get("log-level")),
  };
}

function normalizeIdleTimeoutValue(value: string): string {
  const normalized = value.trim().toLowerCase();

  if (!normalized || normalized === "0" || normalized === "off") {
    return "disabled";
  }

  return normalized.endsWith("m") ? normalized.slice(0, -1) : normalized;
}

function normalizeLogLevelValue(value: string): string {
  const normalized = value.trim().toLowerCase();

  if (!normalized) {
    return "info";
  }

  if (normalized === "warn") {
    return "warning";
  }

  return normalized;
}

function isPortAuto(value: string): boolean {
  const normalized = value.trim().toLowerCase();

  return !normalized || normalized === "auto";
}

function valueToInput(value: unknown): string {
  if (value === null || value === undefined) {
    return "";
  }

  return typeof value === "string" ? value : JSON.stringify(value);
}

function readSavedServerSettings(): SavedServerSettings | null {
  const raw = readStored(settingsStorageKey);

  if (!raw) {
    return null;
  }

  try {
    const value = JSON.parse(raw) as Partial<SavedServerSettings>;

    return {
      port: typeof value.port === "number" && value.port > 0 ? Math.round(value.port) : null,
      keepRunning: Boolean(value.keepRunning),
      startOnOpen: Boolean(value.startOnOpen),
    };
  } catch {
    return null;
  }
}

function writeSavedServerSettings(settings: SavedServerSettings) {
  writeStored(settingsStorageKey, JSON.stringify(settings));
}

const secondaryButton =
  "inline-flex h-[var(--control-size-sm)] items-center gap-1.5 rounded-[var(--item-radius)] border border-line px-2.5 text-[11.5px] text-secondary transition-colors hover:bg-hover hover:text-primary disabled:opacity-50";
