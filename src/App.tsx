import { useCallback, useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import i18n from "./i18n";
import { languages, type Language } from "./i18n/resources";
import { Onboarding } from "./components/Onboarding";
import { Rail } from "./components/Rail";
import { ContextListTarget } from "./components/ContextList";
import { ShortcutsModal } from "./components/ShortcutsModal";
import { ToastProvider } from "./components/Toast";
import { ConfirmDialog } from "./workspaces/models/ConfirmDialog";
import { catalogList, cliDetect, serverSetPreferences, startServer, toFoundryCommandError } from "./lib/foundry";
import { detectLanguage } from "./lib/language";
import {
  accents,
  applyAccent,
  applyDensity,
  applyTypography,
  applyTheme,
  densities,
  resolveTheme,
  themes,
  typographies,
  useSystemPrefersDark,
  type AccentPreference,
  type DensityPreference,
  type ThemePreference,
  type TypographyPreference,
} from "./lib/theme";
import { readStored, writeStored } from "./lib/storage";
import { usePersistentState } from "./lib/usePersistentState";
import { workspaces, type PrimaryWorkspace, type Workspace } from "./lib/workspaces";
import { ChatWorkspace } from "./workspaces/ChatWorkspace";
import { EmbeddingsWorkspace } from "./workspaces/EmbeddingsWorkspace";
import { ModelsWorkspace } from "./workspaces/ModelsWorkspace";
import { SettingsWorkspace } from "./workspaces/SettingsWorkspace";
import { SpeechWorkspace } from "./workspaces/SpeechWorkspace";

const workspaceForDigit: Record<string, Workspace> = {
  "1": "chat",
  "2": "models",
  "3": "speech",
  "4": "embeddings",
  "5": "settings",
};

const listWidthStorageKey = "foundryui.layout.listWidth";
const legacyListWidthStorageKey = "foundryui.models.listWidth";
const minListWidth = 260;
const maxListWidth = 460;
const defaultListWidth = 280;

type WorkspaceLeaveGuard = {
  message: string;
  onConfirmLeave?: () => Promise<void> | void;
};

type PendingWorkspaceChange = {
  next: Workspace;
  closeOnboarding: boolean;
  guard: WorkspaceLeaveGuard;
};

export default function App() {
  const [workspace, setWorkspace] = usePersistentState<Workspace>("foundryui.workspace", "chat", workspaces);
  const [theme, setTheme] = usePersistentState<ThemePreference>("foundryui.theme", "system", themes);
  const [accent, setAccent] = usePersistentState<AccentPreference>("foundryui.accent", "blue", accents);
  const [density, setDensity] = usePersistentState<DensityPreference>("foundryui.density", "normal", densities);
  const [typography, setTypography] = usePersistentState<TypographyPreference>(
    "foundryui.typography",
    "system",
    typographies,
  );
  const [language, setLanguage] = usePersistentState<Language>("foundryui.language", detectLanguage(), languages);
  const systemPrefersDark = useSystemPrefersDark();
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const [onboardingOpen, setOnboardingOpen] = useState(false);
  const [openChatConversationId, setOpenChatConversationId] = useState<number | null>(null);
  const [listTarget, setListTarget] = useState<HTMLDivElement | null>(null);
  const [pendingWorkspaceChange, setPendingWorkspaceChange] = useState<PendingWorkspaceChange | null>(null);
  const [confirmingWorkspaceChange, setConfirmingWorkspaceChange] = useState(false);
  const workspaceLeaveGuardsRef = useRef<Partial<Record<Workspace, WorkspaceLeaveGuard>>>({});

  useEffect(() => {
    let cancelled = false;

    async function checkFirstRun() {
      try {
        await cliDetect();
      } catch (error) {
        const failure = toFoundryCommandError(error);
        const blocksSetup = failure.code === "CliMissing" || failure.code === "CliOutdated";

        if (!cancelled && blocksSetup) {
          setOnboardingOpen(true);
        }

        return;
      }

      try {
        const models = await catalogList();

        if (!cancelled) {
          setOnboardingOpen(!models.some((model) => model.cached));
        }
      } catch {
        return;
      }
    }

    void checkFirstRun();

    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    applyTheme(resolveTheme(theme, systemPrefersDark));
  }, [theme, systemPrefersDark]);

  useEffect(() => {
    applyAccent(accent);
  }, [accent]);

  useEffect(() => {
    applyDensity(density);
  }, [density]);

  useEffect(() => {
    applyTypography(typography);
  }, [typography]);

  useEffect(() => {
    document.documentElement.lang = language;
    void i18n.changeLanguage(language);
  }, [language]);

  useEffect(() => {
    const raw = readStored("foundryui.server.settings");

    if (!raw) {
      return;
    }

    const parsed = parseServerSettings(raw);

    if (!parsed) {
      return;
    }

    serverSetPreferences({
      port: parsed.port,
      keepRunning: parsed.keepRunning,
      connectTimeoutSecs: 5,
      firstTokenTimeoutSecs: 120,
      tokenIdleTimeoutSecs: 60,
    })
      .then(() => {
        if (parsed.startOnOpen) {
          return startServer();
        }

        return null;
      })
      .catch(() => null);
  }, []);

  useEffect(() => {
    const saved = Number(readStored(listWidthStorageKey) ?? readStored(legacyListWidthStorageKey));

    const next = Number.isFinite(saved)
      ? Math.min(maxListWidth, Math.max(minListWidth, Math.round(saved)))
      : defaultListWidth;

    document.documentElement.style.setProperty("--list-width", `${next}px`);

    if (!Number.isFinite(saved)) {
      writeStored(listWidthStorageKey, String(next));
    }
  }, []);

  const applyWorkspaceChange = useCallback(
    (next: Workspace, options?: { closeOnboarding?: boolean }) => {
      setWorkspace(next);

      if (options?.closeOnboarding) {
        setOnboardingOpen(false);
      }
    },
    [setWorkspace],
  );

  const requestWorkspaceChange = useCallback(
    async (next: Workspace, options?: { closeOnboarding?: boolean }) => {
      if (next === workspace) {
        return;
      }

      const guard = workspaceLeaveGuardsRef.current[workspace];

      if (!guard) {
        applyWorkspaceChange(next, options);
        return;
      }

      setPendingWorkspaceChange({
        next,
        closeOnboarding: Boolean(options?.closeOnboarding),
        guard,
      });
    },
    [applyWorkspaceChange, workspace],
  );

  const confirmWorkspaceChange = useCallback(async () => {
    if (!pendingWorkspaceChange || confirmingWorkspaceChange) {
      return;
    }

    setConfirmingWorkspaceChange(true);

    try {
      if (pendingWorkspaceChange.guard.onConfirmLeave) {
        await pendingWorkspaceChange.guard.onConfirmLeave();
      }

      applyWorkspaceChange(pendingWorkspaceChange.next, {
        closeOnboarding: pendingWorkspaceChange.closeOnboarding,
      });
      setPendingWorkspaceChange(null);
    } catch {
      return;
    } finally {
      setConfirmingWorkspaceChange(false);
    }
  }, [applyWorkspaceChange, confirmingWorkspaceChange, pendingWorkspaceChange]);

  const cancelWorkspaceChange = useCallback(() => {
    if (confirmingWorkspaceChange) {
      return;
    }

    setPendingWorkspaceChange(null);
  }, [confirmingWorkspaceChange]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (!event.metaKey && !event.ctrlKey) {
        return;
      }

      const target = workspaceForDigit[event.key];

      if (target) {
        event.preventDefault();
        void requestWorkspaceChange(target);
        return;
      }

      if (event.key === "/") {
        event.preventDefault();
        setShortcutsOpen(true);
      }
    }

    window.addEventListener("keydown", onKeyDown);

    return () => window.removeEventListener("keydown", onKeyDown);
  }, [requestWorkspaceChange]);

  const openShortcuts = useCallback(() => setShortcutsOpen(true), []);
  const closeShortcuts = useCallback(() => setShortcutsOpen(false), []);
  const closeOnboarding = useCallback(() => setOnboardingOpen(false), []);
  const restartOnboarding = useCallback(() => setOnboardingOpen(true), []);
  const openModelsWorkspace = useCallback(() => {
    void requestWorkspaceChange("models");
  }, [requestWorkspaceChange]);
  const openChatConversation = useCallback(
    (conversationId: number) => {
      setOpenChatConversationId(conversationId);
      void requestWorkspaceChange("chat");
    },
    [requestWorkspaceChange],
  );
  const clearOpenChatConversation = useCallback(() => {
    setOpenChatConversationId(null);
  }, []);
  const chooseWorkspace = useCallback(
    (next: PrimaryWorkspace) => {
      void requestWorkspaceChange(next, { closeOnboarding: true });
    },
    [requestWorkspaceChange],
  );
  const setWorkspaceLeaveGuard = useCallback((scope: Workspace, guard: WorkspaceLeaveGuard | null) => {
    const next = { ...workspaceLeaveGuardsRef.current };

    if (guard) {
      next[scope] = guard;
    } else {
      delete next[scope];
    }

    workspaceLeaveGuardsRef.current = next;
  }, []);
  const setEmbeddingsLeaveGuard = useCallback(
    (guard: WorkspaceLeaveGuard | null) => {
      setWorkspaceLeaveGuard("embeddings", guard);
    },
    [setWorkspaceLeaveGuard],
  );
  const setSpeechLeaveGuard = useCallback(
    (guard: WorkspaceLeaveGuard | null) => {
      setWorkspaceLeaveGuard("speech", guard);
    },
    [setWorkspaceLeaveGuard],
  );

  function startListResize(event: ReactPointerEvent<HTMLDivElement>) {
    if (workspace === "settings") {
      return;
    }

    event.preventDefault();

    const startX = event.clientX;
    const root = document.documentElement;
    const startWidth = Number.parseFloat(getComputedStyle(root).getPropertyValue("--list-width")) || defaultListWidth;

    function onPointerMove(moveEvent: PointerEvent) {
      const delta = moveEvent.clientX - startX;
      const next = Math.min(maxListWidth, Math.max(minListWidth, Math.round(startWidth + delta)));

      root.style.setProperty("--list-width", `${next}px`);
    }

    function onPointerUp(upEvent: PointerEvent) {
      const delta = upEvent.clientX - startX;
      const next = Math.min(maxListWidth, Math.max(minListWidth, Math.round(startWidth + delta)));

      root.style.setProperty("--list-width", `${next}px`);
      writeStored(listWidthStorageKey, String(next));
      document.removeEventListener("pointermove", onPointerMove);
      document.removeEventListener("pointerup", onPointerUp);
    }

    document.addEventListener("pointermove", onPointerMove);
    document.addEventListener("pointerup", onPointerUp);
  }

  function renderWorkspace() {
    switch (workspace) {
      case "chat":
        return (
          <ChatWorkspace
            openConversationId={openChatConversationId}
            onOpenConversationHandled={clearOpenChatConversation}
            onOpenModels={openModelsWorkspace}
          />
        );
      case "models":
        return <ModelsWorkspace />;
      case "speech":
        return (
          <SpeechWorkspace
            onOpenModels={openModelsWorkspace}
            onOpenChatConversation={openChatConversation}
            onLeaveGuardChange={setSpeechLeaveGuard}
          />
        );
      case "embeddings":
        return (
          <EmbeddingsWorkspace
            onOpenModels={openModelsWorkspace}
            onLeaveGuardChange={setEmbeddingsLeaveGuard}
          />
        );
      case "settings":
        return (
          <SettingsWorkspace
            theme={theme}
            onThemeChange={setTheme}
            accent={accent}
            onAccentChange={setAccent}
            density={density}
            onDensityChange={setDensity}
            typography={typography}
            onTypographyChange={setTypography}
            language={language}
            onLanguageChange={setLanguage}
            onRestartOnboarding={restartOnboarding}
          />
        );
    }
  }

  return (
    <ToastProvider>
      <div
        className={[
          "app-shell relative grid h-full overflow-hidden bg-window",
          workspace === "settings"
            ? "grid-cols-[280px_minmax(0,1fr)]"
            : "grid-cols-[var(--list-width)_minmax(0,1fr)]",
        ].join(" ")}
      >
        <Rail active={workspace} onSelect={(next) => void requestWorkspaceChange(next)} onShortcuts={openShortcuts}>
          <div ref={setListTarget} className="min-h-0 flex-1" />
        </Rail>
        <ContextListTarget.Provider value={listTarget}>
          <main key={workspace} className="flex h-full min-h-0 min-w-0 flex-col">
            {renderWorkspace()}
          </main>
        </ContextListTarget.Provider>
        {workspace !== "settings" ? (
          <div
            role="separator"
            aria-orientation="vertical"
            aria-label={i18n.t("common.resizeList")}
            tabIndex={0}
            aria-valuemin={minListWidth}
            aria-valuemax={maxListWidth}
            onKeyDown={(event) => {
              if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) {
                return;
              }
              event.preventDefault();
              const root = document.documentElement;
              const current = Number.parseFloat(getComputedStyle(root).getPropertyValue("--list-width")) || defaultListWidth;
              const requested = event.key === "Home" ? minListWidth : event.key === "End" ? maxListWidth : current + (event.key === "ArrowRight" ? 20 : -20);
              const next = Math.min(maxListWidth, Math.max(minListWidth, requested));
              root.style.setProperty("--list-width", `${next}px`);
              writeStored(listWidthStorageKey, String(next));
              event.currentTarget.setAttribute("aria-valuenow", String(next));
            }}
            onPointerDown={startListResize}
            onDoubleClick={() => {
              document.documentElement.style.setProperty("--list-width", `${defaultListWidth}px`);
              writeStored(listWidthStorageKey, String(defaultListWidth));
            }}
            className="group absolute bottom-0 top-0 z-50 w-3 -translate-x-1/2 cursor-col-resize touch-none select-none"
            style={{ left: "var(--list-width)" }}
          >
            <div className="pointer-events-none mx-auto h-full w-px bg-line transition-colors group-hover:bg-line-strong" />
          </div>
        ) : null}
      </div>
      <ShortcutsModal open={shortcutsOpen} onClose={closeShortcuts} />
      <ConfirmDialog
        open={pendingWorkspaceChange !== null}
        title={i18n.t("common.leaveGuardTitle")}
        body={pendingWorkspaceChange?.guard.message ?? ""}
        confirmLabel={i18n.t("common.leaveGuardConfirm")}
        tone="danger"
        busy={confirmingWorkspaceChange}
        onConfirm={() => {
          void confirmWorkspaceChange();
        }}
        onClose={cancelWorkspaceChange}
      />
      {onboardingOpen ? (
        <Onboarding onClose={closeOnboarding} onChooseWorkspace={chooseWorkspace} />
      ) : null}
    </ToastProvider>
  );
}

function parseServerSettings(raw: string): {
  port: number | null;
  keepRunning: boolean;
  startOnOpen: boolean;
} | null {
  try {
    const value = JSON.parse(raw) as Partial<{
      port: number | null;
      keepRunning: boolean;
      startOnOpen: boolean;
    }>;

    return {
      port: typeof value.port === "number" && value.port > 0 ? Math.round(value.port) : null,
      keepRunning: Boolean(value.keepRunning),
      startOnOpen: Boolean(value.startOnOpen),
    };
  } catch {
    return null;
  }
}
