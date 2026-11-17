import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ContextList } from "../components/ContextList";
import { DetailPane } from "../components/DetailPane";
import { EmptyState, ListEmpty } from "../components/EmptyState";
import { ModelAccessPanel } from "../components/ModelAccessPanel";
import { Modal } from "../components/Modal";
import { SegmentedControl } from "../components/SegmentedControl";
import {
  AlertIcon,
  ArchiveIcon,
  ChatIcon,
  CloseIcon,
  CopyIcon,
  DownloadIcon,
  FolderIcon,
  MicIcon,
  PaperclipIcon,
  PlayIcon,
  PlusIcon,
  RefreshIcon,
  SearchIcon,
  SpeechIcon,
  StarIcon,
  StopIcon,
  TrashIcon,
} from "../components/Icon";
import { ContextMenu, type ContextMenuItem } from "../components/ContextMenu";
import { IconButton } from "../components/IconButton";
import { Spinner } from "../components/Spinner";
import { useToast } from "../components/Toast";
import { copyText } from "../lib/copy";
import { listConversations, type Conversation } from "../lib/chat";
import {
  downloadModel,
  loadModel,
  toFoundryCommandError,
  workspaceSnapshot,
  type CatalogModel,
  type FoundryCommandError,
  type JobStatus,
  type LoadedModel,
  type ModelJob,
} from "../lib/foundry";
import {
  audioData,
  cancelRecording,
  deleteTranscription,
  discardAudio,
  importAudio,
  languageOptions,
  listAudioInputDevices,
  listTranscriptions,
  microphonePermission,
  openMicrophoneSettings,
  openSpeechStorageFolder,
  pickAudioFile,
  readSpeechInputPreference,
  readSpeechModelPreference,
  recordingLevel,
  saveTranscriptionText,
  sendToChat,
  setTranscriptionArchived,
  setTranscriptionFavorite,
  startRecording,
  stopRecording,
  transcribe,
  updateTranscription,
  writeSpeechInputPreference,
  writeSpeechModelPreference,
  type AudioInfo,
  type AudioInputDevice,
  type MicrophonePermission,
  type Transcription,
  type TranscriptionDeleteMode,
} from "../lib/speech";
import { useModelJobStream } from "../lib/useModelJobStream";
import { ConfirmDialog } from "./models/ConfirmDialog";
import { errorLabel } from "./models/format";
import { FlowStepper, type FlowStep } from "./speech/FlowStepper";
import { TranscriptionSummary, type SummaryField } from "./speech/TranscriptionSummary";

type SpeechWorkspaceProps = {
  onOpenModels?: () => void;
  onOpenChatConversation?: (conversationId: number) => void;
  onLeaveGuardChange?: (guard: { message: string; onConfirmLeave?: () => Promise<void> | void } | null) => void;
};

type AudioSource = "file" | "microphone";

type StagedAudio = AudioInfo & {
  origin: AudioSource;
};

type MenuState = {
  transcription: Transcription;
  x: number;
  y: number;
};

type ChatTargetMode = "new" | "existing";

const maxRecordingMs = 300_000;

export function SpeechWorkspace({ onOpenModels, onOpenChatConversation, onLeaveGuardChange }: SpeechWorkspaceProps) {
  const { t, i18n } = useTranslation();
  const { push } = useToast();
  const [transcriptions, setTranscriptions] = useState<Transcription[] | null>(null);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [composing, setComposing] = useState(false);
  const [step, setStep] = useState<FlowStep>(1);
  const [flowTransition, setFlowTransition] = useState<"forward" | "backward">("forward");
  const [sourceMode, setSourceMode] = useState<AudioSource>("file");
  const [stagedAudio, setStagedAudio] = useState<StagedAudio | null>(null);
  const [audioUrl, setAudioUrl] = useState<string | null>(null);
  const [audioLoading, setAudioLoading] = useState(false);
  const [draftText, setDraftText] = useState("");
  const [models, setModels] = useState<CatalogModel[] | null>(null);
  const [loaded, setLoaded] = useState<LoadedModel[]>([]);
  const [selectedModelAlias, setSelectedModelAlias] = useState("");
  const [downloadAlias, setDownloadAlias] = useState("");
  const [language, setLanguage] = useState("");
  const [temperature, setTemperature] = useState("0");
  const [permission, setPermission] = useState<MicrophonePermission>("undetermined");
  const [devices, setDevices] = useState<AudioInputDevice[]>([]);
  const [selectedDeviceId, setSelectedDeviceId] = useState("");
  const [devicesLoading, setDevicesLoading] = useState(false);
  const [recordingStartedAt, setRecordingStartedAt] = useState<number | null>(null);
  const [recordingTick, setRecordingTick] = useState(() => Date.now());
  const [recordingLevelValue, setRecordingLevelValue] = useState(0);
  const [draggingAudio, setDraggingAudio] = useState(false);
  const [jobs, setJobs] = useState<Record<string, ModelJob>>({});
  const [banner, setBanner] = useState<FoundryCommandError | null>(null);
  const [transcribing, setTranscribing] = useState(false);
  const [savingEdit, setSavingEdit] = useState(false);
  const [savingText, setSavingText] = useState(false);
  const [loadingModelAlias, setLoadingModelAlias] = useState<string | null>(null);
  const [sendToChatOpen, setSendToChatOpen] = useState(false);
  const [sendToChatBusy, setSendToChatBusy] = useState(false);
  const [chatTargetMode, setChatTargetMode] = useState<ChatTargetMode>("new");
  const [chatConversations, setChatConversations] = useState<Conversation[]>([]);
  const [chatDestinationId, setChatDestinationId] = useState<number | null>(null);
  const [chatModelAlias, setChatModelAlias] = useState("");
  const [lastSentConversationId, setLastSentConversationId] = useState<number | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<Transcription | null>(null);
  const [deleteMode, setDeleteMode] = useState<TranscriptionDeleteMode>("all");
  const [deleting, setDeleting] = useState(false);
  const [archivedView, setArchivedView] = useState(false);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [listFilterOpen, setListFilterOpen] = useState(false);
  const [listQuery, setListQuery] = useState("");
  const draftIdRef = useRef<number | null>(null);
  const stagedPathRef = useRef<string | null>(null);
  const dropHandlerRef = useRef<(path: string) => void>(() => undefined);
  const jobStatusRef = useRef<Record<string, JobStatus>>({});
  const selected = transcriptions?.find((item) => item.id === selectedId) ?? null;
  const showingResult = !composing && selected !== null;
  const listLoaded = transcriptions !== null;
  const showingFlow = composing;
  const showingEmpty = !composing && !archivedView && listLoaded && transcriptions.length === 0;
  const showingArchivedEmpty = !composing && archivedView && listLoaded && selected === null;
  const showingDetailLoading = !composing && !listLoaded;
  const bannerDetail =
    banner?.code === "BadResponse" && banner.detail?.trim().length
      ? banner.detail.trim()
      : null;
  const speechModels = useMemo(
    () => (models ?? []).filter((model) => model.kind === "speech"),
    [models],
  );
  const cachedSpeechModels = useMemo(
    () => speechModels.filter((model) => model.cached),
    [speechModels],
  );
  const uncachedSpeechModels = useMemo(
    () => speechModels.filter((model) => !model.cached),
    [speechModels],
  );
  const loadedSpeechAliases = useMemo(() => new Set(loaded.map((model) => model.alias)), [loaded]);
  const loadedSpeechModels = useMemo(
    () => cachedSpeechModels.filter((model) => loadedSpeechAliases.has(model.alias)),
    [cachedSpeechModels, loadedSpeechAliases],
  );
  const cachedChatModelAliases = useMemo(
    () =>
      (models ?? [])
        .filter((model) => (model.kind === "chat" || model.kind === "multimodal") && model.cached)
        .map((model) => model.alias),
    [models],
  );
  const preferredChatModelAliases = useMemo(() => {
    const loadedPreferred = loaded
      .filter(isLoadedChatModel)
      .map((model) => model.alias)
      .filter((alias, index, list) => list.indexOf(alias) === index)
      .filter((alias) => cachedChatModelAliases.includes(alias));
    const fallback = cachedChatModelAliases.filter((alias) => !loadedPreferred.includes(alias));

    return [...loadedPreferred, ...fallback];
  }, [cachedChatModelAliases, loaded]);
  const loadableSpeechAliases = useMemo(
    () => cachedSpeechModels.filter((model) => !loadedSpeechAliases.has(model.alias)).map((model) => model.alias),
    [cachedSpeechModels, loadedSpeechAliases],
  );
  const activeDownload = downloadAlias ? jobs[downloadAlias] : undefined;
  const elapsedMs = recordingStartedAt === null ? 0 : Math.max(0, recordingTick - recordingStartedAt);
  const audioReady = stagedAudio !== null && recordingStartedAt === null;
  const canGoStep2 = Boolean(selectedModelAlias);
  const canGoStep3 = canGoStep2 && audioReady;
  const furthestStep: FlowStep = !canGoStep2 ? 1 : !canGoStep3 ? 2 : 3;
  const hasFlowResult = showingFlow && selected !== null;
  const dirty = selected !== null && draftText !== selected.text;
  const permissionBlocked = permission === "denied" || permission === "restricted";
  const sendToChatTargetsAvailable =
    chatConversations.length > 0 || preferredChatModelAliases.length > 0;
  const canSendToExistingConversation = chatConversations.length > 0;
  const canSendToNewConversation = preferredChatModelAliases.length > 0;
  const filteredTranscriptions = useMemo(() => {
    const query = listQuery.trim().toLocaleLowerCase(i18n.language);

    if (!query) {
      return transcriptions ?? [];
    }

    return (transcriptions ?? []).filter((item) => {
      const text = item.text.toLocaleLowerCase(i18n.language);
      const model = item.modelAlias.toLocaleLowerCase(i18n.language);
      const source = item.sourcePath.toLocaleLowerCase(i18n.language);

      return text.includes(query) || model.includes(query) || source.includes(query);
    });
  }, [i18n.language, listQuery, transcriptions]);
  const languageLabel = languageName(language, i18n.language, t("speech.model.autoLanguage"));
  const flowSummary: SummaryField[] = stagedAudio
    ? [
        { label: t("speech.summary.model"), value: selectedModelAlias || "—" },
        { label: t("speech.summary.language"), value: languageLabel },
        { label: t("speech.summary.temperature"), value: formatTemperature(Number(temperature)) },
        { label: t("speech.summary.duration"), value: formatDuration(stagedAudio.durationMs) },
        {
          label: t("speech.summary.origin"),
          value:
            stagedAudio.origin === "file"
              ? t("speech.capture.originFile")
              : t("speech.capture.originRecording"),
        },
        { label: t("speech.summary.file"), value: stagedAudio.name || fileName(stagedAudio.path) },
        {
          label: t("speech.summary.path"),
          value: (
            <span className="break-all font-mono text-[11px] leading-relaxed text-secondary">
              {stagedAudio.path}
            </span>
          ),
          wide: true,
        },
      ]
    : [];
  const viewSummary: SummaryField[] = selected
    ? [
        { label: t("speech.summary.model"), value: selected.modelAlias },
        {
          label: t("speech.summary.language"),
          value: languageName(
            selected.language ?? selected.params?.requestedLanguage ?? "",
            i18n.language,
            t("speech.model.autoLanguage"),
          ),
        },
        {
          label: t("speech.summary.temperature"),
          value: formatTemperature(selected.params?.temperature ?? 0),
        },
        { label: t("speech.summary.duration"), value: formatDuration(selected.durationMs) },
        { label: t("speech.summary.file"), value: fileName(selected.sourcePath) },
        {
          label: t("speech.summary.path"),
          value: (
            <span className="break-all font-mono text-[11px] leading-relaxed text-secondary">
              {selected.sourcePath}
            </span>
          ),
          wide: true,
        },
      ]
    : [];
  const missingDownloadedSpeechModels = models !== null && cachedSpeechModels.length === 0;
  const missingLoadedSpeechModels =
    models !== null && cachedSpeechModels.length > 0 && loadedSpeechModels.length === 0;
  const panelClass = "min-w-0 rounded-[9px] border border-line bg-canvas p-3.5 sm:p-4";
  const sectionTitleClass = "text-[11px] font-semibold uppercase tracking-[0.055em] text-tertiary";
  const hintClass = "mt-1 text-[11.5px] leading-relaxed text-secondary";
  const fieldClass =
    "h-8 w-full min-w-0 rounded-md border border-line bg-canvas px-2.5 text-[12px] text-primary outline-none transition-colors focus:border-line-strong disabled:cursor-not-allowed disabled:opacity-50";
  const fieldLabelClass = "text-[11px] text-secondary";
  const secondaryButtonClass =
    "inline-flex h-7 min-w-0 items-center justify-center gap-1.5 rounded-md border border-line bg-canvas px-2.5 text-[11.5px] text-secondary transition-colors hover:bg-hover hover:text-primary disabled:cursor-not-allowed disabled:opacity-50";
  const primaryButtonClass =
    "inline-flex h-8 min-w-0 items-center justify-center gap-1.5 rounded-md bg-accent px-3 text-[12px] font-medium text-accent-contrast transition-colors hover:bg-accent-hover disabled:cursor-not-allowed disabled:opacity-45";
  const ghostButtonClass =
    "inline-flex h-7 min-w-0 items-center justify-center gap-1.5 rounded-md px-2.5 text-[11.5px] text-secondary transition-colors hover:bg-hover hover:text-primary disabled:opacity-50";
  const panelMotion =
    flowTransition === "forward"
      ? "speech-flow-panel speech-flow-panel-forward"
      : "speech-flow-panel speech-flow-panel-backward";

  const refreshTranscriptions = useCallback(async (archived = false, selectFirst = true) => {
    const items = await listTranscriptions(archived);

    setTranscriptions(items);
    setSelectedId((current) => {
      if (current !== null && items.some((item) => item.id === current)) {
        return current;
      }

      return selectFirst ? (items[0]?.id ?? null) : null;
    });
  }, []);

  const refreshModelState = useCallback(async () => {
    const snapshot = await workspaceSnapshot();

    setModels(snapshot.catalog);
    setLoaded(snapshot.loaded.models);
  }, []);

  const refreshPermission = useCallback(async () => {
    setPermission(await microphonePermission());
  }, []);

  const refreshDevices = useCallback(async () => {
    setDevicesLoading(true);

    try {
      setDevices(await listAudioInputDevices());
    } catch {
      setDevices([]);
    } finally {
      setDevicesLoading(false);
    }
  }, []);

  const goToFlowStep = useCallback(
    (next: FlowStep) => {
      setFlowTransition(next >= step ? "forward" : "backward");
      setStep(next);
    },
    [step],
  );

  useEffect(() => {
    let live = true;

    Promise.all([
      refreshTranscriptions(),
      refreshModelState(),
      refreshPermission(),
      refreshDevices(),
    ]).catch((error) => {
      if (live) {
        setBanner(toFoundryCommandError(error));
      }
    });

    return () => {
      live = false;
    };
  }, [refreshDevices, refreshModelState, refreshPermission, refreshTranscriptions]);

  useEffect(() => {
    if (step > 1 && !canGoStep2) {
      setFlowTransition("backward");
      setStep(1);
      return;
    }

    if (step === 3 && !canGoStep3) {
      setFlowTransition("backward");
      setStep(2);
    }
  }, [canGoStep2, canGoStep3, step]);

  useEffect(() => {
    if (composing || transcriptions === null || filteredTranscriptions.length === 0) {
      return;
    }

    const first = filteredTranscriptions[0];

    if (!first) {
      return;
    }

    if (selectedId === null || !filteredTranscriptions.some((item) => item.id === selectedId)) {
      setSelectedId(first.id);
    }
  }, [composing, filteredTranscriptions, selectedId, transcriptions]);

  useEffect(() => {
    if (selectedId === draftIdRef.current) {
      return;
    }

    draftIdRef.current = selectedId;
    setDraftText(
      selectedId === null
        ? ""
        : ((transcriptions?.find((item) => item.id === selectedId)?.text) ?? ""),
    );
  }, [selectedId, transcriptions]);

  useLayoutEffect(() => {
    if (!onLeaveGuardChange) {
      return;
    }

    const pendingDraft = composing;

    if (!pendingDraft) {
      onLeaveGuardChange(null);
      return;
    }

    onLeaveGuardChange({
      message: t("speech.flow.leaveConfirm"),
    });

    return () => {
      onLeaveGuardChange(null);
    };
  }, [composing, onLeaveGuardChange, t]);

  useEffect(() => {
    if (!cachedSpeechModels.length) {
      setSelectedModelAlias("");

      return;
    }

    const preferred = readSpeechModelPreference();

    setSelectedModelAlias((current) =>
      cachedSpeechModels.some((model) => model.alias === current)
        ? current
        : cachedSpeechModels.some((model) => model.alias === preferred)
          ? (preferred ?? "")
          : (cachedSpeechModels[0]?.alias ?? ""),
    );
  }, [cachedSpeechModels]);

  useEffect(() => {
    if (!selectedModelAlias || !cachedSpeechModels.some((model) => model.alias === selectedModelAlias)) {
      return;
    }

    writeSpeechModelPreference(selectedModelAlias);
  }, [cachedSpeechModels, selectedModelAlias]);

  useEffect(() => {
    if (chatTargetMode === "existing" && !canSendToExistingConversation && canSendToNewConversation) {
      setChatTargetMode("new");
      return;
    }

    if (chatTargetMode === "new" && !canSendToNewConversation && canSendToExistingConversation) {
      setChatTargetMode("existing");
    }
  }, [
    canSendToExistingConversation,
    canSendToNewConversation,
    chatTargetMode,
  ]);

  useEffect(() => {
    setChatDestinationId((current) => {
      if (current !== null && chatConversations.some((conversation) => conversation.id === current)) {
        return current;
      }

      return chatConversations[0]?.id ?? null;
    });
  }, [chatConversations]);

  useEffect(() => {
    setChatModelAlias((current) =>
      preferredChatModelAliases.includes(current)
        ? current
        : (preferredChatModelAliases[0] ?? ""),
    );
  }, [preferredChatModelAliases]);

  useEffect(() => {
    setDownloadAlias((current) =>
      uncachedSpeechModels.some((model) => model.alias === current)
        ? current
        : (uncachedSpeechModels[0]?.alias ?? ""),
    );
  }, [uncachedSpeechModels]);

  useEffect(() => {
    setSelectedDeviceId((current) => {
      if (devices.some((device) => device.id === current)) {
        return current;
      }

      const preferred = readSpeechInputPreference();

      if (preferred && devices.some((device) => device.id === preferred)) {
        return preferred;
      }

      return devices.find((device) => device.isDefault)?.id ?? devices[0]?.id ?? "";
    });
  }, [devices]);

  useEffect(() => {
    if (selectedDeviceId) {
      writeSpeechInputPreference(selectedDeviceId);
    }
  }, [selectedDeviceId]);

  useEffect(() => {
    stagedPathRef.current = stagedAudio?.path ?? null;
  }, [stagedAudio]);

  useEffect(() => {
    return () => {
      const current = stagedPathRef.current;

      if (current) {
        void discardAudio(current);
      }
    };
  }, []);

  useEffect(() => {
    if (!stagedAudio) {
      setAudioUrl(null);
      setAudioLoading(false);

      return;
    }

    let live = true;
    setAudioLoading(true);

    audioData(stagedAudio.path)
      .then((url) => {
        if (live) {
          setAudioUrl(url);
        }
      })
      .catch((error) => {
        if (live) {
          setAudioUrl(null);
          setBanner(toFoundryCommandError(error));
        }
      })
      .finally(() => {
        if (live) {
          setAudioLoading(false);
        }
      });

    return () => {
      live = false;
    };
  }, [stagedAudio]);

  useEffect(() => {
    if (recordingStartedAt === null) {
      setRecordingLevelValue(0);

      return;
    }

    let live = true;
    const timer = window.setInterval(() => {
      setRecordingTick(Date.now());
      recordingLevel()
        .then((value) => {
          if (live) {
            setRecordingLevelValue(value);
          }
        })
        .catch(() => {
          if (live) {
            setRecordingLevelValue(0);
          }
        });
    }, 80);

    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [recordingStartedAt]);

  dropHandlerRef.current = (path) => {
    void importFromPath(path);
  };

  useEffect(() => {
    let unlisten: (() => void) | null = null;

    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "enter") {
          setDraggingAudio(true);
          return;
        }

        if (event.payload.type === "leave") {
          setDraggingAudio(false);
          return;
        }

        if (event.payload.type === "drop") {
          setDraggingAudio(false);
          const path = event.payload.paths[0];

          if (path) {
            dropHandlerRef.current(path);
          }

          return;
        }

        setDraggingAudio(true);
      })
      .then((stop) => {
        unlisten = stop;
      })
      .catch(() => undefined);

    return () => {
      unlisten?.();
    };
  }, []);

  useModelJobStream((job, hydrated) => {
    setJobs((current) => {
      const known = current[job.alias];

      if (known && known.updatedAt > job.updatedAt) {
        return current;
      }

      return { ...current, [job.alias]: job };
    });

    if (hydrated) {
      jobStatusRef.current[job.alias] = job.status;

      return;
    }

    const previous = jobStatusRef.current[job.alias];

    jobStatusRef.current[job.alias] = job.status;

    if (previous === job.status) {
      return;
    }

    if (job.status === "succeeded") {
      push(t("speech.model.downloadDone", { model: job.alias }), "neutral");
      void refreshModelState();

      return;
    }

    if (job.status === "failed" && job.error) {
      push(errorLabel(toFoundryCommandError(job.error), t), "danger");
    }
  });

  function startNewTranscription() {
    if (transcribing || recordingStartedAt !== null) {
      return;
    }

    const previous = stagedAudio?.path;

    setComposing(true);
    setSelectedId(null);
    setSourceMode("file");
    setStagedAudio(null);
    setAudioUrl(null);
    setDraftText("");
    setBanner(null);
    setFlowTransition("forward");
    setStep(1);
    onLeaveGuardChange?.({
      message: t("speech.flow.leaveConfirm"),
    });

    if (previous) {
      void discardAudio(previous);
    }
  }

  function openTranscription(id: number) {
    if (transcribing || recordingStartedAt !== null) {
      return;
    }

    if (stagedAudio) {
      const path = stagedAudio.path;

      setStagedAudio(null);
      setAudioUrl(null);
      void discardAudio(path);
    }

    setComposing(false);
    setSelectedId(id);
    setBanner(null);
    onLeaveGuardChange?.(null);
  }

  async function importFromPath(path: string) {
    if (transcribing || recordingStartedAt !== null) {
      push(t("speech.flow.busy"), "neutral");
      return;
    }

    try {
      const info = await importAudio(path);
      const previous = stagedAudio?.path;

      setStagedAudio({ ...info, origin: "file" });
      setComposing(true);
      setSourceMode("file");
      setBanner(null);
      goToFlowStep(canGoStep2 ? 2 : 1);
      onLeaveGuardChange?.({
        message: t("speech.flow.leaveConfirm"),
      });

      if (previous && previous !== info.path) {
        await discardAudio(previous);
      }
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    }
  }

  async function pickAndImport() {
    const path = await pickAudioFile();

    if (path) {
      await importFromPath(path);
    }
  }

  async function revealStorageFolder() {
    try {
      await openSpeechStorageFolder();
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    }
  }

  async function revealMicrophoneSettings() {
    try {
      await openMicrophoneSettings();
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    }
  }

  async function startRecordingSession() {
    if (recordingStartedAt !== null || transcribing) {
      return;
    }

    try {
      const startedAt = await startRecording(selectedDeviceId || null);

      setRecordingStartedAt(startedAt);
      setRecordingTick(Date.now());
      setComposing(true);
      setSourceMode("microphone");
      setBanner(null);
      onLeaveGuardChange?.({
        message: t("speech.flow.leaveConfirm"),
      });
      await refreshPermission();
    } catch (error) {
      const failure = toFoundryCommandError(error);

      setBanner(failure);

      if (failure.code === "MicrophoneDenied") {
        setPermission("denied");
      }
    }
  }

  async function finishRecording() {
    if (recordingStartedAt === null) {
      return;
    }

    try {
      const info = await stopRecording();
      const previous = stagedAudio?.path;

      setRecordingStartedAt(null);
      setRecordingLevelValue(0);
      setStagedAudio({ ...info, origin: "microphone" });
      setBanner(null);

      if (previous && previous !== info.path) {
        await discardAudio(previous);
      }
    } catch (error) {
      setRecordingStartedAt(null);
      setRecordingLevelValue(0);
      setBanner(toFoundryCommandError(error));
    }
  }

  async function cancelRecordingSession() {
    if (recordingStartedAt === null) {
      return;
    }

    try {
      await cancelRecording();
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setRecordingStartedAt(null);
      setRecordingLevelValue(0);
    }
  }

  async function discardStagedAudio() {
    if (!stagedAudio) {
      return;
    }

    const path = stagedAudio.path;

    setStagedAudio(null);
    setAudioUrl(null);

    try {
      await discardAudio(path);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    }
  }

  async function runTranscription() {
    const transcriptionPath = stagedAudio?.path ?? null;

    if (!transcriptionPath || !selectedModelAlias || transcribing || recordingStartedAt !== null) {
      return;
    }

    const value = Number(temperature.trim());

    setTranscribing(true);

    try {
      const result = await transcribe({
        path: transcriptionPath,
        modelAlias: selectedModelAlias,
        language: language || null,
        temperature: Number.isFinite(value) ? value : 0,
      });

      setTranscriptions((current) => mergeTranscriptions(current ?? [], [result.transcription]));
      setSelectedId(result.transcription.id);
      setDraftText(result.transcription.text);
      setBanner(null);

      if (archivedView) {
        setArchivedView(false);
        await refreshTranscriptions(false);
        setSelectedId(result.transcription.id);
      }

      if (result.autoLoaded) {
        push(t("speech.model.autoLoaded", { model: selectedModelAlias }), "neutral");
        void refreshModelState();
      }
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setTranscribing(false);
    }
  }

  async function saveEditedText() {
    if (!selected || savingEdit) {
      return;
    }

    setSavingEdit(true);

    try {
      const next = await updateTranscription(selected.id, draftText);

      setTranscriptions((current) => mergeTranscriptions(current ?? [], [next]));
      push(t("speech.editor.saved"), "neutral");
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setSavingEdit(false);
    }
  }

  async function copyTranscriptionText() {
    const text = draftText.trim();

    if (!text) {
      return;
    }

    const copied = await copyText(text);

    push(t(copied ? "speech.editor.copied" : "speech.editor.copyFailed"), copied ? "neutral" : "danger");
  }

  async function saveTextFile() {
    if (!selected || savingText) {
      return;
    }

    setSavingText(true);

    try {
      const receipt = await saveTranscriptionText(selected.id);
      const file = receipt.path.split(/[\\/]/).pop() ?? receipt.path;

      push(t("speech.editor.savedFile", { file }), "neutral");
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setSavingText(false);
    }
  }

  async function openSendToChatDialog() {
    const text = draftText.trim();

    if (!text) {
      push(t("speech.sendToChat.empty"), "danger");
      return;
    }

    try {
      const conversations = await listConversations(false);

      setChatConversations(conversations);

      if (conversations.length > 0) {
        setChatTargetMode("existing");
        setChatDestinationId(conversations[0]?.id ?? null);
      } else if (preferredChatModelAliases.length > 0) {
        setChatTargetMode("new");
      }

      if (conversations.length === 0 && preferredChatModelAliases.length === 0) {
        push(t("speech.sendToChat.noTarget"), "danger");
        return;
      }

      setSendToChatOpen(true);
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    }
  }

  async function confirmSendToChat() {
    const text = draftText.trim();

    if (!text || sendToChatBusy) {
      return;
    }

    if (chatTargetMode === "existing" && chatDestinationId === null) {
      return;
    }

    if (chatTargetMode === "new" && !chatModelAlias) {
      return;
    }

    setSendToChatBusy(true);

    try {
      const destination = await sendToChat({
        conversationId: chatTargetMode === "existing" ? chatDestinationId : null,
        modelAlias: chatTargetMode === "new" ? chatModelAlias : null,
        text,
      });

      setLastSentConversationId(destination.id);
      setSendToChatOpen(false);
      push(
        t("speech.sendToChat.sent", {
          title: destination.title || t("chat.untitled"),
        }),
        "neutral",
      );
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setSendToChatBusy(false);
    }
  }

  function openLastSentConversation() {
    if (lastSentConversationId === null) {
      return;
    }

    onOpenChatConversation?.(lastSentConversationId);
  }

  async function confirmDelete() {
    const target = deleteTarget;

    if (!target || deleting) {
      return;
    }

    setDeleting(true);

    try {
      const result = await deleteTranscription(target.id, deleteMode);

      if (result.removed) {
        const remaining = (transcriptions ?? []).filter((item) => item.id !== target.id);

        setTranscriptions(remaining);

        if (selectedId === target.id) {
          setSelectedId(remaining[0]?.id ?? null);
        }
      } else {
        const updatedTranscription = result.transcription;

        if (updatedTranscription) {
          setTranscriptions((current) => mergeTranscriptions(current ?? [], [updatedTranscription]));
        }
      }

      setDeleteTarget(null);
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setDeleting(false);
    }
  }

  async function switchArchivedView() {
    if (transcribing || recordingStartedAt !== null) {
      return;
    }

    const next = !archivedView;

    setArchivedView(next);
    setComposing(false);
    setTranscriptions(null);
    setSelectedId(null);
    setListQuery("");
    onLeaveGuardChange?.(null);

    try {
      const items = await listTranscriptions(next);

      setTranscriptions(items);
      setSelectedId(items[0]?.id ?? null);
    } catch (error) {
      setTranscriptions([]);
      setBanner(toFoundryCommandError(error));
    }
  }

  async function toggleFavorite(transcription: Transcription) {
    try {
      const updated = await setTranscriptionFavorite(
        transcription.id,
        !transcription.favorite,
      );

      setTranscriptions((current) => mergeTranscriptions(current ?? [], [updated]));
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    }
  }

  async function toggleArchived(transcription: Transcription) {
    try {
      const updated = await setTranscriptionArchived(
        transcription.id,
        !transcription.archived,
      );
      const remaining = (transcriptions ?? []).filter((item) => item.id !== updated.id);

      setTranscriptions(remaining);

      if (selectedId === updated.id) {
        setSelectedId(remaining[0]?.id ?? null);
      }

      push(t(updated.archived ? "speech.archivedToast" : "speech.unarchivedToast"), "neutral");
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    }
  }

  async function triggerDownload() {
    if (!downloadAlias) {
      return;
    }

    try {
      await downloadModel(downloadAlias);
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    }
  }

  async function loadSpeechModelAlias(alias: string) {
    if (loadingModelAlias) {
      return;
    }

    setLoadingModelAlias(alias);

    try {
      await loadModel(alias);
      await refreshModelState();
      setBanner(null);
      push(t("models.actions.loaded", { alias }), "neutral");
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setLoadingModelAlias(null);
    }
  }

  const menuItems: ContextMenuItem[] = menu
    ? [
        {
          label: menu.transcription.favorite ? t("speech.unfavorite") : t("speech.favorite"),
          icon: StarIcon,
          onSelect: () => void toggleFavorite(menu.transcription),
        },
        {
          label: archivedView ? t("speech.unarchive") : t("speech.archive"),
          icon: ArchiveIcon,
          onSelect: () => void toggleArchived(menu.transcription),
        },
        {
          label: t("speech.deleteForever"),
          icon: TrashIcon,
          tone: "danger",
          onSelect: () => {
            setDeleteMode("all");
            setDeleteTarget(menu.transcription);
          },
        },
      ]
    : [];
  const detailTitle = showingResult && selected
    ? excerpt(selected.text)
    : showingFlow
      ? t("speech.flow.newTranscription")
      : archivedView
        ? t("speech.archivedTitle")
        : t("speech.listTitle");
  const detailSubtitle = showingFlow
    ? t("speech.flow.stepOf", { current: step })
    : showingResult && selected
      ? `${selected.modelAlias} · ${formatWhen(selected.createdAt, i18n.language)}`
      : null;
  const sourceOptions = [
    {
      value: "file" as const,
      icon: PaperclipIcon,
      label: t("speech.capture.file"),
      hint: t("speech.capture.fileHint"),
    },
    {
      value: "microphone" as const,
      icon: MicIcon,
      label: t("speech.capture.microphone"),
      hint: t("speech.capture.microphoneHint"),
    },
  ];
  const flowSteps = [
    { value: 1 as const, label: t("speech.flow.step1") },
    { value: 2 as const, label: t("speech.flow.step2") },
    { value: 3 as const, label: t("speech.flow.step3") },
  ];

  return (
    <>
      <ContextList
        title={archivedView ? t("speech.archivedTitle") : t("speech.listTitle")}
        action={
          <div className="flex items-center gap-1">
            <IconButton
              label={archivedView ? t("speech.archivedBack") : t("speech.archivedTitle")}
              onClick={() => void switchArchivedView()}
              disabled={composing || transcribing || recordingStartedAt !== null}
              className={archivedView ? "border-line-strong bg-active text-primary" : ""}
            >
              <ArchiveIcon size={14} />
            </IconButton>
            <IconButton
              label={t("speech.listFilterPlaceholder")}
              onClick={() => setListFilterOpen((current) => !current)}
            >
              <SearchIcon size={14} />
            </IconButton>
            {archivedView ? null : (
              <IconButton
                label={t("speech.listNew")}
                onClick={startNewTranscription}
                disabled={transcribing || recordingStartedAt !== null}
              >
                <PlusIcon size={14} />
              </IconButton>
            )}
          </div>
        }
        toolbar={
          listFilterOpen || listQuery.trim() ? (
            <div className="relative">
              <SearchIcon
                size={13}
                className="pointer-events-none absolute left-2 top-1/2 -translate-y-1/2 text-tertiary"
              />
              <input
                autoFocus
                type="text"
                value={listQuery}
                onChange={(event) => setListQuery(event.target.value)}
                placeholder={t("speech.listFilterPlaceholder")}
                aria-label={t("speech.listFilterPlaceholder")}
                onBlur={() => {
                  if (!listQuery.trim()) {
                    setListFilterOpen(false);
                  }
                }}
                className="h-[var(--control-size)] w-full rounded-[var(--item-radius)] border border-line bg-elevated pl-7 pr-7 text-[12px] text-primary outline-none transition-colors placeholder:text-tertiary focus:border-accent"
              />
              {listQuery ? (
                <button
                  type="button"
                  aria-label={t("models.clearSearch")}
                  title={t("models.clearSearch")}
                  onClick={() => setListQuery("")}
                  className="absolute right-1 top-1/2 inline-flex h-6 w-6 -translate-y-1/2 items-center justify-center rounded text-tertiary transition-colors hover:bg-hover hover:text-primary"
                >
                  <CloseIcon size={12} />
                </button>
              ) : null}
            </div>
          ) : null
        }
      >
        {transcriptions === null ? (
          <div className="flex justify-center py-6">
            <Spinner size={14} className="text-tertiary" />
          </div>
        ) : transcriptions.length === 0 ? (
          <ListEmpty
            icon={archivedView ? ArchiveIcon : SpeechIcon}
            title={
              archivedView ? t("speech.archivedEmptyTitle") : t("speech.listEmptyTitle")
            }
            body={archivedView ? t("speech.archivedEmptyBody") : t("speech.listEmptyBody")}
          />
        ) : filteredTranscriptions.length === 0 ? (
          <ListEmpty
            icon={SearchIcon}
            title={t("speech.listNoMatchesTitle")}
            body={t("speech.listNoMatchesBody")}
          />
        ) : (
          <ul className="flex flex-col gap-1 pb-2">
            {filteredTranscriptions.map((item) => {
              const active = item.id === selectedId;

              return (
                <li
                  key={item.id}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    setMenu({ transcription: item, x: event.clientX, y: event.clientY });
                  }}
                >
                  <button
                    type="button"
                    disabled={transcribing || recordingStartedAt !== null}
                    onClick={() => openTranscription(item.id)}
                    onKeyDown={(event) => {
                      if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
                        event.preventDefault();

                        const rect = event.currentTarget.getBoundingClientRect();

                        setMenu({ transcription: item, x: rect.right - 12, y: rect.top + 26 });
                      }
                    }}
                    aria-current={active ? "true" : undefined}
                    className={[
                      "flex w-full flex-col gap-1 rounded-[var(--item-radius)] border px-[var(--item-pad-x)] py-[var(--item-pad-y)] text-left transition-colors disabled:cursor-not-allowed disabled:opacity-60",
                      active
                        ? "border-line-strong bg-elevated shadow-[var(--shadow-thumb)]"
                        : "border-transparent hover:border-line hover:bg-hover",
                    ].join(" ")}
                  >
                    <span className="flex w-full min-w-0 items-center gap-1.5">
                      {item.favorite ? (
                        <StarIcon size={11} className="shrink-0 fill-current text-accent" />
                      ) : null}
                      <span className="line-clamp-2 min-w-0 flex-1 text-[12px] leading-[1.4] text-primary">
                        {excerpt(item.text)}
                      </span>
                    </span>
                    <span className="flex items-center justify-between gap-2 text-[10.5px]">
                      <span className="min-w-0 truncate text-secondary">
                        {item.modelAlias}
                        {item.audioExists ? "" : ` · ${t("speech.capture.audioMissing")}`}
                      </span>
                      <span className="shrink-0 tabular-nums text-tertiary">{formatWhen(item.createdAt, i18n.language)}</span>
                    </span>
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </ContextList>

      {menu ? (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          label={t("speech.transcriptionActions")}
          items={menuItems}
          onClose={() => setMenu(null)}
        />
      ) : null}

      <DetailPane
        title={detailTitle}
        {...(detailSubtitle ? { subtitle: detailSubtitle } : {})}
        actions={
          showingResult && selected ? (
            <>
              <IconButton
                label={selected.favorite ? t("speech.unfavorite") : t("speech.favorite")}
                onClick={() => void toggleFavorite(selected)}
                className={selected.favorite ? "text-accent" : ""}
              >
                <StarIcon size={14} className={selected.favorite ? "fill-current" : ""} />
              </IconButton>
              <IconButton
                label={selected.archived ? t("speech.unarchive") : t("speech.archive")}
                onClick={() => void toggleArchived(selected)}
                className={selected.archived ? "border-line-strong bg-active text-primary" : ""}
              >
                <ArchiveIcon size={14} />
              </IconButton>
              <IconButton
                label={t("speech.deleteForever")}
                onClick={() => {
                  setDeleteMode("all");
                  setDeleteTarget(selected);
                }}
              >
                <TrashIcon size={14} />
              </IconButton>
            </>
          ) : undefined
        }
      >
        <div className="mx-auto flex min-h-full w-full max-w-[760px] min-w-0 flex-col gap-4 px-4 py-5 sm:px-6 sm:py-6">
          {banner ? (
            <div role="alert" className="flex items-start gap-2 rounded-[8px] border border-line bg-subtle px-3 py-2.5 text-[12px] text-danger">
              <AlertIcon size={14} className="shrink-0" />
              <div className="min-w-0 flex-1">
                <p>{errorLabel(banner, t)}</p>
                {bannerDetail ? (
                  <p className="mt-0.5 break-words text-[11px] text-tertiary">
                    {t("speech.errorDetail", { detail: bannerDetail })}
                  </p>
                ) : null}
              </div>
            </div>
          ) : null}

          {showingDetailLoading ? (
            <div className="flex flex-1 items-center justify-center py-12">
              <Spinner size={16} className="text-tertiary" />
            </div>
          ) : null}

          {showingArchivedEmpty ? (
            <div className="flex min-h-0 flex-1 items-center justify-center">
              <EmptyState
                icon={ArchiveIcon}
                title={t("speech.archivedEmptyTitle")}
                body={t("speech.archivedEmptyBody")}
              />
            </div>
          ) : null}

          {showingEmpty ? (
            <div className="flex min-h-0 flex-1 items-center justify-center">
              <EmptyState
                icon={SpeechIcon}
                title={t("speech.listEmptyTitle")}
                body={t("speech.listEmptyBody")}
              />
            </div>
          ) : null}

          {showingFlow ? (
            <>
              {missingDownloadedSpeechModels || missingLoadedSpeechModels ? (
                <ModelAccessPanel
                  title={
                    missingDownloadedSpeechModels
                      ? t("common.noDownloadedModelTitle", { kind: t("models.kinds.speech") })
                      : t("common.noLoadedModelTitle", { kind: t("models.kinds.speech") })
                  }
                  body={
                    missingDownloadedSpeechModels
                      ? t("common.noDownloadedModelBody", { kind: t("models.kinds.speech") })
                      : t("common.noLoadedModelBody", { kind: t("models.kinds.speech") })
                  }
                  listTitle={t("common.downloadedModels")}
                  models={loadableSpeechAliases}
                  loadLabel={t("models.actions.load")}
                  loadingLabel={t("common.loading")}
                  openModelsLabel={t("common.openModels")}
                  loadingAlias={loadingModelAlias}
                  onLoad={(alias) => void loadSpeechModelAlias(alias)}
                  onOpenModels={() => onOpenModels?.()}
                />
              ) : null}

              <FlowStepper
                label={t("speech.flow.stepsLabel")}
                steps={flowSteps}
                current={step}
                furthest={furthestStep}
                onSelect={goToFlowStep}
              />

              {step === 1 ? (
                <section className={`${panelClass} ${panelMotion}`}>
                  <div className="min-w-0">
                    <h2 className={sectionTitleClass}>{t("speech.flow.step1Title")}</h2>
                    <p className={hintClass}>{t("speech.flow.step1Hint")}</p>
                  </div>

                  <div className="mt-4 grid gap-3 sm:grid-cols-2">
                    <div className="flex min-w-0 flex-col gap-1 sm:col-span-2">
                      <span className={fieldLabelClass}>{t("speech.model.model")}</span>
                      {cachedSpeechModels.length > 1 ? (
                        <select
                          value={selectedModelAlias}
                          onChange={(event) => setSelectedModelAlias(event.target.value)}
                          className={fieldClass}
                        >
                          {cachedSpeechModels.map((model) => (
                            <option key={model.alias} value={model.alias}>
                              {model.alias}
                            </option>
                          ))}
                        </select>
                      ) : (
                        <div className="flex h-8 items-center rounded-md border border-line bg-subtle px-2.5 text-[12px] text-primary">
                          {selectedModelAlias || t("speech.model.noneCached")}
                        </div>
                      )}
                    </div>

                    <label className="flex min-w-0 flex-col gap-1">
                      <span className={fieldLabelClass}>{t("speech.model.language")}</span>
                      <select
                        value={language}
                        onChange={(event) => setLanguage(event.target.value)}
                        className={fieldClass}
                      >
                        <option value="">{t("speech.model.autoLanguage")}</option>
                        {languageOptions.map((code) => (
                          <option key={code} value={code}>
                            {languageName(code, i18n.language, code)}
                          </option>
                        ))}
                      </select>
                    </label>

                    <div className="flex min-w-0 flex-col gap-1">
                      <div className="flex items-baseline justify-between gap-2">
                        <span className={fieldLabelClass}>{t("speech.model.temperature")}</span>
                        <span className="tabular-nums text-[11px] text-secondary">
                          {formatTemperature(Number(temperature))}
                        </span>
                      </div>
                      <input
                        type="range"
                        min={0}
                        max={1}
                        step={0.1}
                        value={Number(temperature)}
                        onChange={(event) => setTemperature(event.target.value)}
                        aria-label={t("speech.model.temperature")}
                        className="h-8 w-full accent-accent"
                      />
                      <span className="text-[10.5px] leading-relaxed text-tertiary">
                        {t("speech.model.temperatureHint")}
                      </span>
                    </div>
                  </div>

                  {uncachedSpeechModels.length ? (
                    <details className="mt-4 border-t border-line pt-3">
                      <summary className="cursor-pointer text-[11.5px] font-medium text-secondary">
                        {t("speech.model.downloadShortcut")}
                      </summary>
                      <div className="mt-2 grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
                        <select
                          aria-label={t("speech.model.downloadShortcut")}
                          value={downloadAlias}
                          onChange={(event) => setDownloadAlias(event.target.value)}
                          className={fieldClass}
                        >
                          {uncachedSpeechModels.map((model) => (
                            <option key={model.alias} value={model.alias}>
                              {model.alias}
                            </option>
                          ))}
                        </select>
                        <button
                          type="button"
                          onClick={() => void triggerDownload()}
                          disabled={!downloadAlias || activeDownload?.status === "running"}
                          className={`${secondaryButtonClass} h-8 w-full sm:w-auto`}
                        >
                          {activeDownload?.status === "running" ? <Spinner size={11} /> : <DownloadIcon size={12} />}
                          {downloadLabel(activeDownload, t)}
                        </button>
                      </div>
                    </details>
                  ) : null}

                  <div className="mt-4 flex flex-wrap items-center justify-between gap-2 border-t border-line pt-3">
                    <p className="text-[10.5px] text-tertiary">
                      {canGoStep2 ? t("speech.flow.step1Ready") : t("speech.flow.stepNeedModel")}
                    </p>
                    <button
                      type="button"
                      onClick={() => goToFlowStep(2)}
                      disabled={!canGoStep2}
                      className={primaryButtonClass}
                    >
                      {t("speech.flow.nextToAudio")}
                    </button>
                  </div>
                </section>
              ) : null}

              {step === 2 ? (
                <section className={`${panelClass} ${panelMotion}`}>
                  <div className="min-w-0">
                    <h2 className={sectionTitleClass}>{t("speech.capture.title")}</h2>
                    <p className={hintClass}>{t("speech.flow.step2Hint")}</p>
                  </div>

                  <fieldset className="mt-4 min-w-0">
                    <legend className="sr-only">{t("speech.capture.legend")}</legend>
                    <div className="grid gap-2 sm:grid-cols-2">
                      {sourceOptions.map((option) => {
                        const active = sourceMode === option.value;
                        const Icon = option.icon;

                        return (
                          <label
                            key={option.value}
                            className={[
                              "flex min-w-0 cursor-pointer items-start gap-2.5 rounded-[8px] border px-3 py-2.5 text-left transition-colors focus-within:ring-2 focus-within:ring-accent/40",
                              active
                                ? "border-accent bg-accent/[0.06]"
                                : "border-line bg-canvas hover:border-line-strong hover:bg-hover",
                              recordingStartedAt !== null || transcribing
                                ? "cursor-not-allowed opacity-60"
                                : "",
                            ].join(" ")}
                          >
                            <input
                              type="radio"
                              name="speech-audio-source"
                              value={option.value}
                              checked={active}
                              onChange={() => setSourceMode(option.value)}
                              disabled={recordingStartedAt !== null || transcribing}
                              className="sr-only"
                            />
                            <span
                              className={[
                                "mt-0.5 inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-full",
                                active ? "bg-accent/10 text-accent-text" : "bg-subtle text-secondary",
                              ].join(" ")}
                            >
                              <Icon size={15} />
                            </span>
                            <span className="min-w-0 flex-1">
                              <span className="block text-[12.5px] font-medium text-primary">
                                {option.label}
                              </span>
                              <span className="mt-0.5 block text-[11px] leading-relaxed text-secondary">
                                {option.hint}
                              </span>
                            </span>
                            <span
                              aria-hidden="true"
                              className={[
                                "mt-0.5 inline-flex h-4 w-4 shrink-0 items-center justify-center rounded-full border",
                                active ? "border-accent bg-accent" : "border-line-strong bg-canvas",
                              ].join(" ")}
                            >
                              {active ? <span className="h-1.5 w-1.5 rounded-full bg-accent-contrast" /> : null}
                            </span>
                          </label>
                        );
                      })}
                    </div>
                  </fieldset>

                  {sourceMode === "file" ? (
                    <div className="mt-4">
                      <button
                        type="button"
                        onClick={() => void pickAndImport()}
                        disabled={transcribing}
                        className={[
                          "flex min-h-[84px] w-full items-center justify-center rounded-[8px] border border-dashed px-4 py-4 text-center text-[12px] transition-colors disabled:cursor-not-allowed disabled:opacity-50",
                          draggingAudio
                            ? "border-accent bg-accent/[0.06] text-primary"
                            : "border-line text-secondary hover:border-line-strong hover:bg-hover hover:text-primary",
                        ].join(" ")}
                      >
                        {draggingAudio ? t("speech.capture.dropActive") : t("speech.capture.dropIdle")}
                      </button>
                      <div className="mt-2 flex flex-wrap items-center justify-between gap-2">
                        <p className="text-[10.5px] text-tertiary">{t("speech.capture.formats")}</p>
                        <button
                          type="button"
                          onClick={() => void revealStorageFolder()}
                          className={ghostButtonClass}
                        >
                          <FolderIcon size={12} />
                          {t("speech.capture.openFolder")}
                        </button>
                      </div>
                    </div>
                  ) : (
                    <div className="mt-4 space-y-3">
                      <div className="rounded-[8px] bg-subtle px-3 py-2.5">
                        <div className="flex items-end gap-2">
                          <label className="flex min-w-0 flex-1 flex-col gap-1">
                            <span className={fieldLabelClass}>{t("speech.recording.device")}</span>
                            <select
                              value={selectedDeviceId}
                              onChange={(event) => setSelectedDeviceId(event.target.value)}
                              disabled={recordingStartedAt !== null || devicesLoading}
                              className={fieldClass}
                            >
                              {devices.length === 0 ? (
                                <option value="">{t("speech.recording.noDevices")}</option>
                              ) : (
                                devices.map((device) => (
                                  <option key={device.id} value={device.id}>
                                    {device.name}
                                    {device.isDefault ? ` · ${t("speech.recording.defaultTag")}` : ""}
                                  </option>
                                ))
                              )}
                            </select>
                          </label>
                          <IconButton
                            label={t("speech.recording.refresh")}
                            onClick={() => void refreshDevices()}
                            disabled={devicesLoading || recordingStartedAt !== null}
                          >
                            <RefreshIcon size={13} className={devicesLoading ? "animate-spin" : ""} />
                          </IconButton>
                        </div>
                      </div>

                      {permissionBlocked ? (
                        <div className="rounded-[8px] bg-subtle px-3 py-2.5 text-[11.5px] text-secondary">
                          <p>
                            {permission === "denied"
                              ? t("speech.recording.permissionDenied")
                              : t("speech.recording.permissionRestricted")}
                          </p>
                          <button
                            type="button"
                            onClick={() => void revealMicrophoneSettings()}
                            className={`${secondaryButtonClass} mt-2`}
                          >
                            {t("speech.actions.openMicrophoneSettings")}
                          </button>
                        </div>
                      ) : permission === "undetermined" ? (
                        <p className="text-[11px] leading-relaxed text-secondary">
                          {t("speech.recording.permissionUndetermined")}
                        </p>
                      ) : permission === "unknown" ? (
                        <p className="text-[11px] leading-relaxed text-secondary">
                          {t("speech.recording.permissionUnknown")}
                        </p>
                      ) : null}

                      {recordingStartedAt !== null ? (
                        <div className="rounded-[8px] border border-line bg-canvas px-3 py-3">
                          <div className="flex items-center justify-between gap-3">
                            <span className="flex items-center gap-2 text-[12px] font-medium text-primary">
                              <span className="h-2 w-2 animate-pulse rounded-full bg-danger" />
                              {t("speech.recording.active")}
                            </span>
                            <span className="tabular-nums text-[13px] font-semibold text-primary">
                              {formatDuration(elapsedMs)}
                            </span>
                          </div>
                          <div
                            role="progressbar"
                            aria-label={t("speech.recording.active")}
                            aria-valuemin={0}
                            aria-valuemax={maxRecordingMs}
                            aria-valuenow={Math.min(elapsedMs, maxRecordingMs)}
                            className="mt-2 h-1.5 overflow-hidden rounded-full bg-subtle"
                          >
                            <div
                              className="h-full rounded-full bg-accent"
                              style={{ width: `${Math.min(100, (elapsedMs / maxRecordingMs) * 100)}%` }}
                            />
                          </div>
                          <div className="mt-2 flex items-center gap-2">
                            <span className="shrink-0 text-[10.5px] text-tertiary">
                              {t("speech.recording.level")}
                            </span>
                            <div className="h-1 flex-1 overflow-hidden rounded-full bg-subtle" aria-hidden="true">
                              <div
                                className="h-full rounded-full bg-success transition-[width] duration-75"
                                style={{ width: `${Math.max(2, Math.round(recordingLevelValue * 100))}%` }}
                              />
                            </div>
                          </div>
                          <div className="mt-3 flex flex-wrap justify-end gap-2">
                            <button
                              type="button"
                              onClick={() => void cancelRecordingSession()}
                              className={ghostButtonClass}
                            >
                              {t("speech.recording.discard")}
                            </button>
                            <button
                              type="button"
                              onClick={() => void finishRecording()}
                              className={primaryButtonClass}
                            >
                              <StopIcon size={11} />
                              {t("speech.recording.stop")}
                            </button>
                          </div>
                        </div>
                      ) : (
                        <div className="flex flex-wrap items-center gap-2.5">
                          <button
                            type="button"
                            onClick={() => void startRecordingSession()}
                            disabled={permissionBlocked}
                            className={primaryButtonClass}
                          >
                            <MicIcon size={13} />
                            {t("speech.recording.start")}
                          </button>
                          <p className="min-w-0 flex-1 text-[11px] leading-relaxed text-secondary">
                            {stagedAudio?.origin === "microphone"
                              ? t("speech.recording.recorded")
                              : t("speech.recording.idle")}
                          </p>
                        </div>
                      )}
                    </div>
                  )}

                  {stagedAudio && recordingStartedAt === null ? (
                    <div className="mt-4 rounded-[8px] bg-subtle px-3 py-3">
                      <div className="flex flex-wrap items-start justify-between gap-2">
                        <div className="min-w-0">
                          <p className="text-[10.5px] font-medium uppercase tracking-[0.055em] text-tertiary">
                            {t("speech.capture.selected")}
                          </p>
                          <p className="mt-1 truncate text-[12.5px] font-medium text-primary">
                            {stagedAudio.name || fileName(stagedAudio.path)}
                          </p>
                          <p className="mt-0.5 text-[10.5px] text-secondary">
                            {stagedAudio.origin === "file"
                              ? t("speech.capture.originFile")
                              : t("speech.capture.originRecording")}
                            {" · "}
                            {t("speech.capture.duration", {
                              duration: formatDuration(stagedAudio.durationMs),
                            })}
                          </p>
                        </div>
                        <button
                          type="button"
                          onClick={() => void discardStagedAudio()}
                          className={ghostButtonClass}
                        >
                          <CloseIcon size={11} />
                          {t("speech.capture.discard")}
                        </button>
                      </div>
                      {audioLoading ? (
                        <div className="mt-2 flex justify-center py-2">
                          <Spinner size={12} className="text-tertiary" />
                        </div>
                      ) : audioUrl ? (
                        <audio controls src={audioUrl} className="mt-2 w-full max-w-full" />
                      ) : null}
                    </div>
                  ) : null}

                  <div className="mt-4 flex flex-wrap items-center justify-between gap-2 border-t border-line pt-3">
                    <button
                      type="button"
                      onClick={() => goToFlowStep(1)}
                      disabled={recordingStartedAt !== null || transcribing}
                      className={`${secondaryButtonClass} h-8`}
                    >
                      {t("speech.flow.back")}
                    </button>
                    <button
                      type="button"
                      onClick={() => goToFlowStep(3)}
                      disabled={!canGoStep3}
                      className={primaryButtonClass}
                    >
                      {t("speech.flow.nextToTranscribe")}
                    </button>
                  </div>
                </section>
              ) : null}

              {step === 3 ? (
                <>
                  <section className={`${panelClass} ${panelMotion}`}>
                    <TranscriptionSummary
                      title={t("speech.summary.title")}
                      fields={flowSummary}
                    />
                  </section>

                  <section className={`${panelClass} ${panelMotion} flex min-h-[320px] flex-col`}>
                    <div className="flex flex-wrap items-start justify-between gap-3">
                      <div className="min-w-0">
                        <h2 className={sectionTitleClass}>{t("speech.transcribe.title")}</h2>
                        <p className={hintClass}>{t("speech.flow.step3Hint")}</p>
                      </div>
                      <button
                        type="button"
                        onClick={() => void runTranscription()}
                        disabled={!canGoStep3 || transcribing}
                        className={primaryButtonClass}
                      >
                        {transcribing ? <Spinner size={12} /> : <PlayIcon size={12} />}
                        {transcribing
                          ? t("speech.actions.transcribing")
                          : t("speech.actions.transcribe")}
                      </button>
                    </div>

                    {hasFlowResult ? (
                      <>
                        <textarea
                          aria-label={t("speech.editor.placeholder")}
                          value={draftText}
                          onChange={(event) => setDraftText(event.target.value)}
                          placeholder={t("speech.editor.placeholder")}
                          className="mt-3 min-h-[240px] w-full flex-1 resize-y rounded-[8px] border border-line bg-canvas px-3 py-2.5 text-[13px] leading-[1.65] text-primary outline-none transition-colors focus:border-line-strong"
                        />
                        <div className="mt-2 flex flex-wrap items-center justify-between gap-2">
                          <div className="flex flex-wrap gap-1.5">
                            <button
                              type="button"
                              onClick={() => void openSendToChatDialog()}
                              disabled={sendToChatBusy || !draftText.trim()}
                              className={secondaryButtonClass}
                            >
                              {sendToChatBusy ? <Spinner size={11} /> : <ChatIcon size={12} />}
                              {t("speech.sendToChat.send")}
                            </button>
                            <button
                              type="button"
                              onClick={() => void copyTranscriptionText()}
                              disabled={!draftText.trim()}
                              className={secondaryButtonClass}
                            >
                              <CopyIcon size={12} />
                              {t("speech.editor.copy")}
                            </button>
                            <button
                              type="button"
                              onClick={() => void saveTextFile()}
                              disabled={savingText}
                              className={secondaryButtonClass}
                            >
                              {savingText ? <Spinner size={11} /> : <DownloadIcon size={12} />}
                              {t("speech.editor.saveFile")}
                            </button>
                            {lastSentConversationId !== null ? (
                              <button
                                type="button"
                                onClick={openLastSentConversation}
                                className={secondaryButtonClass}
                              >
                                {t("speech.sendToChat.openConversation")}
                              </button>
                            ) : null}
                          </div>
                          {dirty ? (
                            <button
                              type="button"
                              onClick={() => void saveEditedText()}
                              disabled={savingEdit}
                              className={secondaryButtonClass}
                            >
                              {savingEdit ? t("speech.editor.saving") : t("speech.editor.save")}
                            </button>
                          ) : null}
                        </div>
                      </>
                    ) : (
                      <p className="mt-4 text-[11.5px] leading-relaxed text-tertiary">
                        {transcribing
                          ? t("speech.actions.transcribing")
                          : t("speech.transcribe.placeholder")}
                      </p>
                    )}

                    <div className="mt-4 flex flex-wrap items-center justify-between gap-2 border-t border-line pt-3">
                      <button
                        type="button"
                        onClick={() => goToFlowStep(2)}
                        disabled={transcribing}
                        className={`${secondaryButtonClass} h-8`}
                      >
                        {t("speech.flow.back")}
                      </button>
                      {hasFlowResult ? (
                        <button
                          type="button"
                          onClick={startNewTranscription}
                          className={`${secondaryButtonClass} h-8`}
                        >
                          {t("speech.transcribe.new")}
                        </button>
                      ) : null}
                    </div>
                  </section>
                </>
              ) : null}
            </>
          ) : null}

          {showingResult && selected ? (
            <>
              <section className={panelClass}>
                <TranscriptionSummary
                  title={t("speech.summary.title")}
                  fields={viewSummary}
                  {...(!selected.audioExists
                    ? { note: t("speech.capture.audioMissingDetail") }
                    : {})}
                />
              </section>

              <section className={`${panelClass} flex min-h-[360px] flex-1 flex-col`}>
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <h2 className={sectionTitleClass}>{t("speech.transcribe.title")}</h2>
                  <div className="flex flex-wrap gap-1.5">
                    <button
                      type="button"
                      onClick={() => void openSendToChatDialog()}
                      disabled={sendToChatBusy || !draftText.trim()}
                      className={secondaryButtonClass}
                    >
                      {sendToChatBusy ? <Spinner size={11} /> : <ChatIcon size={12} />}
                      {t("speech.sendToChat.send")}
                    </button>
                    <button
                      type="button"
                      onClick={() => void copyTranscriptionText()}
                      disabled={!draftText.trim()}
                      className={secondaryButtonClass}
                    >
                      <CopyIcon size={12} />
                      {t("speech.editor.copy")}
                    </button>
                    <button
                      type="button"
                      onClick={() => void saveTextFile()}
                      disabled={savingText}
                      className={secondaryButtonClass}
                    >
                      {savingText ? <Spinner size={11} /> : <DownloadIcon size={12} />}
                      {t("speech.editor.saveFile")}
                    </button>
                    {lastSentConversationId !== null ? (
                      <button
                        type="button"
                        onClick={openLastSentConversation}
                        className={secondaryButtonClass}
                      >
                        {t("speech.sendToChat.openConversation")}
                      </button>
                    ) : null}
                  </div>
                </div>
                <textarea
                  aria-label={t("speech.editor.placeholder")}
                  value={draftText}
                  onChange={(event) => setDraftText(event.target.value)}
                  placeholder={t("speech.editor.placeholder")}
                  className="mt-3 min-h-[260px] w-full flex-1 resize-y rounded-[8px] border border-line bg-canvas px-3 py-2.5 text-[13px] leading-[1.65] text-primary outline-none transition-colors focus:border-line-strong"
                />
                {dirty ? (
                  <div className="mt-2 flex justify-end">
                    <button
                      type="button"
                      onClick={() => void saveEditedText()}
                      disabled={savingEdit}
                      className={secondaryButtonClass}
                    >
                      {savingEdit ? t("speech.editor.saving") : t("speech.editor.save")}
                    </button>
                  </div>
                ) : null}
              </section>
            </>
          ) : null}
        </div>
      </DetailPane>

      <Modal
        open={sendToChatOpen}
        title={t("speech.sendToChat.title")}
        size="md"
        onClose={() => {
          if (!sendToChatBusy) {
            setSendToChatOpen(false);
          }
        }}
      >
        <div className="space-y-3">
          <p className="text-[12px] leading-relaxed text-secondary">{t("speech.sendToChat.body")}</p>

          {canSendToExistingConversation && canSendToNewConversation ? (
            <SegmentedControl
              label={t("speech.sendToChat.target")}
              value={chatTargetMode}
              options={[
                { value: "existing", label: t("speech.sendToChat.targetExisting") },
                { value: "new", label: t("speech.sendToChat.targetNew") },
              ]}
              onChange={setChatTargetMode}
            />
          ) : null}

          {chatTargetMode === "existing" ? (
            canSendToExistingConversation ? (
              <label className="flex min-w-0 flex-col gap-1">
                <span className="text-[11px] text-secondary">{t("speech.sendToChat.conversation")}</span>
                <select
                  value={chatDestinationId ?? ""}
                  onChange={(event) => setChatDestinationId(Number(event.target.value) || null)}
                  className={fieldClass}
                >
                  {chatConversations.map((conversation) => (
                    <option key={conversation.id} value={conversation.id}>
                      {(conversation.title || t("chat.untitled")) + " · " + conversation.modelAlias}
                    </option>
                  ))}
                </select>
              </label>
            ) : (
              <p className="text-[11.5px] text-secondary">{t("speech.sendToChat.noConversation")}</p>
            )
          ) : canSendToNewConversation ? (
            <label className="flex min-w-0 flex-col gap-1">
              <span className="text-[11px] text-secondary">{t("speech.sendToChat.model")}</span>
              <select
                value={chatModelAlias}
                onChange={(event) => setChatModelAlias(event.target.value)}
                className={fieldClass}
              >
                {preferredChatModelAliases.map((alias) => (
                  <option key={alias} value={alias}>
                    {alias}
                  </option>
                ))}
              </select>
            </label>
          ) : (
            <p className="text-[11.5px] text-secondary">{t("speech.sendToChat.noModel")}</p>
          )}

          <div className="flex flex-wrap justify-end gap-2 pt-1">
            <button
              type="button"
              onClick={() => setSendToChatOpen(false)}
              disabled={sendToChatBusy}
              className="inline-flex h-8 items-center justify-center rounded-md border border-line px-3 text-[11.5px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary disabled:opacity-60"
            >
              {t("common.cancel")}
            </button>
            <button
              type="button"
              onClick={() => void confirmSendToChat()}
              disabled={
                sendToChatBusy ||
                !draftText.trim() ||
                !sendToChatTargetsAvailable ||
                (chatTargetMode === "existing" ? chatDestinationId === null : !chatModelAlias)
              }
              className={primaryButtonClass}
            >
              {sendToChatBusy ? <Spinner size={11} /> : <ChatIcon size={12} />}
              {sendToChatBusy ? t("speech.sendToChat.sending") : t("speech.sendToChat.confirm")}
            </button>
          </div>
        </div>
      </Modal>

      <ConfirmDialog
        open={deleteTarget !== null}
        title={t("speech.delete.title")}
        body={t("speech.delete.body")}
        confirmLabel={
          deleteMode === "audio"
            ? t("speech.delete.confirmAudio")
            : deleteMode === "transcription"
              ? t("speech.delete.confirmTranscription")
              : t("speech.delete.confirmAll")
        }
        tone="danger"
        busy={deleting}
        onConfirm={() => void confirmDelete()}
        onClose={() => setDeleteTarget(null)}
      >
        <fieldset className="mt-3 space-y-1 rounded-[8px] border border-line bg-subtle p-2">
          <label className="flex items-center gap-2 text-[11.5px] text-secondary">
            <input
              type="radio"
              name="speech-delete-mode"
              checked={deleteMode === "all"}
              onChange={() => setDeleteMode("all")}
            />
            {t("speech.delete.optionAll")}
          </label>
          <label className="flex items-center gap-2 text-[11.5px] text-secondary">
            <input
              type="radio"
              name="speech-delete-mode"
              checked={deleteMode === "transcription"}
              onChange={() => setDeleteMode("transcription")}
            />
            {t("speech.delete.optionTranscription")}
          </label>
          <label className="flex items-center gap-2 text-[11.5px] text-secondary">
            <input
              type="radio"
              name="speech-delete-mode"
              checked={deleteMode === "audio"}
              onChange={() => setDeleteMode("audio")}
            />
            {t("speech.delete.optionAudio")}
          </label>
        </fieldset>
      </ConfirmDialog>
    </>
  );
}

function mergeTranscriptions(current: Transcription[], incoming: Transcription[]): Transcription[] {
  const byId = new Map(current.map((item) => [item.id, item]));

  for (const item of incoming) {
    byId.set(item.id, item);
  }

  return Array.from(byId.values()).sort((a, b) => {
    if (a.favorite !== b.favorite) {
      return a.favorite ? -1 : 1;
    }

    if (b.createdAt !== a.createdAt) {
      return b.createdAt - a.createdAt;
    }

    return b.id - a.id;
  });
}

function fileName(path: string): string {
  const segments = path.split(/[\\/]/);

  return segments[segments.length - 1] ?? path;
}

function excerpt(text: string): string {
  const compact = text.replace(/\s+/g, " ").trim();

  if (!compact) {
    return "—";
  }

  return compact.length > 96 ? `${compact.slice(0, 96)}…` : compact;
}

function formatDuration(durationMs: number): string {
  const totalSeconds = Math.max(0, Math.round(durationMs / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;

  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

function formatWhen(value: number, language: string): string {
  const date = new Date(value);

  return new Intl.DateTimeFormat(language, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

function formatTemperature(value: number): string {
  if (!Number.isFinite(value)) {
    return "0";
  }

  return value.toFixed(1).replace(/\.0$/, "");
}

function languageName(code: string, locale: string, fallback: string): string {
  if (!code) {
    return fallback;
  }

  try {
    return new Intl.DisplayNames([locale], { type: "language" }).of(code) ?? code;
  } catch {
    return code;
  }
}

function downloadLabel(job: ModelJob | undefined, t: any): string {
  if (!job || job.status === "succeeded") {
    return String(t("speech.actions.download"));
  }

  if (job.status === "running") {
    return String(t("speech.actions.downloadingUnknown"));
  }

  if (job.status === "failed") {
    return String(t("speech.actions.retryDownload"));
  }

  return String(t("speech.actions.download"));
}

function isLoadedChatModel(model: LoadedModel): boolean {
  const type = model.modelType?.toLowerCase();

  return type === "chat" || type === "multimodal";
}
