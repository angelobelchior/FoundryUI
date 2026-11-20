import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ContextList } from "../components/ContextList";
import { EmptyState, ListEmpty } from "../components/EmptyState";
import {
  AlertIcon,
  ArchiveIcon,
  ChatIcon,
  ChevronDownIcon,
  ChevronLeftIcon,
  CloseIcon,
  DownloadIcon,
  PencilIcon,
  PlusIcon,
  SearchIcon,
  TrashIcon,
} from "../components/Icon";
import { IconButton } from "../components/IconButton";
import { Modal } from "../components/Modal";
import { Spinner } from "../components/Spinner";
import { useToast } from "../components/Toast";
import {
  cancelGeneration,
  conversationUsage,
  deleteConversation,
  editChatMessage,
  exportConversation,
  listMessages,
  mergeConversation,
  mergeMessages,
  regenerateMessage,
  renameConversation,
  searchMessages,
  sendChatMessage,
  setConversationArchived,
  setConversationFavorite,
  setConversationModel,
  setConversationVariant,
  type ChatMessage,
  type Conversation,
  type ExportFormat,
  type SearchHit,
  type SendReceipt,
  type StreamEvent,
  type TokenUsage,
} from "../lib/chat";
import {
  discardAttachments,
  fileToDataUrl,
  imageRejection,
  maxImageBytes,
  maxImagesPerMessage,
  stageAttachment,
  type StagedImage,
} from "../lib/attachments";
import {
  catalogModel,
  downloadModel,
  FoundryCommandError,
  loadModel,
  startServer,
  toFoundryCommandError,
  unloadModel,
  workspaceSnapshot,
  type CatalogModel,
  type Declared,
  type JobStatus,
  type LoadedModel,
  type ModelJob,
  type ModelVariant,
} from "../lib/foundry";
import { minimumSearchQuery, searchResultLimit } from "../lib/search";
import { dragWindowFromMouseDown } from "../lib/drag";
import { ModelPicker } from "../components/ModelPicker";
import { useModelJobStream } from "../lib/useModelJobStream";
import { ConfirmDialog } from "./models/ConfirmDialog";
import { errorLabel } from "./models/format";
import { Composer } from "./chat/Composer";
import { ConversationList } from "./chat/ConversationList";
import { ExportMenu } from "./chat/ExportMenu";
import { ProjectFilesPanel } from "./chat/ProjectFilesPanel";
import { EditorPanel } from "./chat/editor/EditorPanel";
import { useEditorTabs } from "./chat/editor/useEditorTabs";
import { MessageList } from "./chat/MessageList";
import { ModelMenu, type ChatModelOption } from "./chat/ModelMenu";
import { ParamsPopover } from "./chat/ParamsPopover";
import { SearchPanel, type SearchScope } from "./chat/SearchPanel";
import { TokenUsageBadge } from "./chat/TokenUsageBadge";
import { projectApi, type ChatProject, type ProjectConversation, type ProjectFolderSelection } from "../lib/projects";
import { agentApi, type AgentTurn, type AgentToolCard } from "../lib/agent";
import { ProjectNavigation, ProjectsPanel } from "./chat/ProjectsPanel";
import { ProjectContext } from "./chat/ProjectContext";
import { ProjectMoveDialog } from "./chat/ProjectMoveDialog";
import { projectErrorLabel } from "./chat/projectErrors";
import { ProjectCreateDialog } from "./chat/ProjectCreateDialog";
import { projectControlFocus } from "./chat/projectFocus";
import { cx } from "../lib/cx";
import { ExecutionPopover } from "./chat/ExecutionPopover";
import { useAgentExecution } from "./chat/useAgentExecution";

const THREAD_PAGE_SIZE = 80;

type ActiveGeneration = {
  requestId: string;
  conversationId: number;
};

type BannerAction = {
  label: string;
  busy: boolean;
  onClick: () => void;
};

const chatModelTypes = new Set(["chat", "multimodal"]);

type ChatWorkspaceProps = {
  openConversationId?: number | null;
  onOpenConversationHandled?: () => void;
  onOpenModels?: () => void;
};

export function ChatWorkspace({
  openConversationId = null,
  onOpenConversationHandled,
  onOpenModels,
}: ChatWorkspaceProps) {
  const { t } = useTranslation();
  const { push } = useToast();

  const [projects, setProjects] = useState<ChatProject[]>([]);
  const [projectsView, setProjectsView] = useState(false);
  const [projectId, setProjectId] = useState<number | null>(null);
  const [projectBusy, setProjectBusy] = useState(false);
  const [projectError, setProjectError] = useState<string | null>(null);
  const [projectsLoading, setProjectsLoading] = useState(true);
  const [projectsLoadError, setProjectsLoadError] = useState<string | null>(null);
  const [projectNotice, setProjectNotice] = useState<string | null>(null);
  const [projectMoveOpen, setProjectMoveOpen] = useState(false);
  const [projectFolderSelection, setProjectFolderSelection] = useState<ProjectFolderSelection | null>(null);
  const [projectSelectionExpired, setProjectSelectionExpired] = useState(false);
  const pendingFolderSelection = useRef<ProjectFolderSelection | null>(null);
  const projectWorkspaceMounted = useRef(true);
  const moveReturnFocus = useRef<HTMLElement | null>(null);
  const selectedProject = projects.find((project) => project.id === projectId) ?? null;
  const [conversations, setConversations] = useState<Conversation[] | null>(null);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [usage, setUsage] = useState<TokenUsage | null>(null);
  const [hasMore, setHasMore] = useState(false);
  const [loadingThread, setLoadingThread] = useState(false);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [loaded, setLoaded] = useState<LoadedModel[] | null>(null);
  const [catalog, setCatalog] = useState<CatalogModel[] | null>(null);
  const [loadedError, setLoadedError] = useState<FoundryCommandError | null>(null);
  const [banner, setBanner] = useState<FoundryCommandError | null>(null);
  const [drafts, setDrafts] = useState<Record<number, string>>({});
  const [staged, setStaged] = useState<Record<number, StagedImage[]>>({});
  const [inputModalities, setInputModalities] = useState<Declared<string[]> | null>(null);
  const [active, setActive] = useState<ActiveGeneration | null>(null);
  const [stopping, setStopping] = useState(false);
  const [sending, setSending] = useState(false);
  const [reconnecting, setReconnecting] = useState(false);
  const [renameOpen, setRenameOpen] = useState(false);
  const [renameValue, setRenameValue] = useState("");
  const [renaming, setRenaming] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<Conversation | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [archivedView, setArchivedView] = useState(false);
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchScope, setSearchScope] = useState<SearchScope>("conversation");
  const [searchResults, setSearchResults] = useState<SearchHit[]>([]);
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState<FoundryCommandError | null>(null);
  const [highlightId, setHighlightId] = useState<number | null>(null);
  const [editTarget, setEditTarget] = useState<ChatMessage | null>(null);
  const [editValue, setEditValue] = useState("");
  const [editBusy, setEditBusy] = useState(false);
  const [regenerateOpen, setRegenerateOpen] = useState(false);
  const [regenerateBusy, setRegenerateBusy] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [loadingModelAlias, setLoadingModelAlias] = useState<string | null>(null);
  const [freeingMemoryAlias, setFreeingMemoryAlias] = useState<string | null>(null);
  const [jobs, setJobs] = useState<Record<string, ModelJob>>({});
  const jobStatusRef = useRef<Record<string, JobStatus>>({});
  const [listFilterOpen, setListFilterOpen] = useState(false);
  const [listQuery, setListQuery] = useState("");
  const activeRef = useRef<ActiveGeneration | null>(null);
  const selectedIdRef = useRef<number | null>(null);
  const scopeNavigation = useRef(0);
  const scopeRequest = useRef(0);
  const selectionIntent = useRef<{ navigation: number; id: number } | null>(null);
  const [scopeRevision, setScopeRevision] = useState(0);
  const finishedRef = useRef(new Set<string>());
  const pendingJump = useRef<{ conversationId: number; messageId: number } | null>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const [jumpNonce, setJumpNonce] = useState(0);
  const selected = conversations?.find((conversation) => conversation.id === selectedId) ?? null;
  const [agentTurns, setAgentTurns] = useState<AgentTurn[]>([]);
  const refreshAgentMessages = useCallback((id: number) => {
    void listMessages(id, null, THREAD_PAGE_SIZE).then((page) => {
      if (selectedIdRef.current === id) setMessages((current) => mergeMessages(current, page.messages));
    }).catch(() => {});
    void agentApi.turns(id).then((turns) => {
      if (selectedIdRef.current === id) setAgentTurns(turns);
    }).catch(() => {});
  }, []);
  const agent = useAgentExecution(selectedId, refreshAgentMessages);
  const editor = useEditorTabs();
  useEffect(() => {
    if (selectedId === null) {
      setAgentTurns([]);
      return;
    }
    let live = true;
    void agentApi.turns(selectedId).then((turns) => {
      if (live && selectedIdRef.current === selectedId) setAgentTurns(turns);
    }).catch(() => {});
    return () => { live = false; };
  }, [selectedId]);
  const toolsByMessage = useMemo(() => {
    const map: Record<number, AgentToolCard[]> = {};
    for (const turn of agentTurns) map[turn.messageId] = turn.tools;
    return map;
  }, [agentTurns]);
  const agentCompatible = (catalog ?? []).some((model) =>
    (model.alias === selected?.modelAlias || model.id === selected?.modelAlias) && model.supportsToolCalling,
  );
  const chatModels = (loaded ?? []).filter(isChatModel);
  const cachedChatModels = (catalog ?? []).filter(
    (model) => (model.kind === "chat" || model.kind === "multimodal") && model.cached,
  );
  const allChatModels = (catalog ?? []).filter(
    (model) => model.kind === "chat" || model.kind === "multimodal",
  );
  const streamingConversationId = active?.conversationId ?? null;
  const draft = selectedId === null ? "" : (drafts[selectedId] ?? "");
  const attachments = selectedId === null ? [] : (staged[selectedId] ?? []);
  const selectedModelAlias = selected?.modelAlias ?? null;
  const selectedVariantId = selected?.modelVariantId ?? null;
  const selectedCatalogModel =
    selectedModelAlias === null
      ? null
      : (catalog ?? []).find(
          (model) => model.alias === selectedModelAlias || model.id === selectedModelAlias,
        ) ?? null;
  const unloadCandidate = useMemo(
    () => pickUnloadCandidate(loaded ?? [], selectedModelAlias),
    [loaded, selectedModelAlias],
  );
  const notDeclared = inputModalities === null || inputModalities.origin === "notDeclared";
  const attachBlocked =
    inputModalities !== null &&
    inputModalities.origin !== "notDeclared" &&
    !(inputModalities.value ?? []).includes("image");
  const attachEnabled = !attachBlocked && active === null && !sending && !agentCompatible && !agent.busy;
  const attachHint = attachBlocked
    ? t("chat.attachDisabled")
    : notDeclared
      ? t("chat.attachNotDeclared")
      : t("chat.attach");

  useEffect(() => {
    activeRef.current = active;
  }, [active]);

  useEffect(() => {
    if (!projectMoveOpen) moveReturnFocus.current?.focus();
  }, [projectMoveOpen]);

  useEffect(() => {
    selectedIdRef.current = selectedId;
  }, [selectedId]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "f") {
        if (document.querySelector('[role="dialog"]')) {
          return;
        }
        if (selectedProject) {
          return;
        }

        event.preventDefault();
        setSearchOpen(true);
        window.setTimeout(() => searchInputRef.current?.focus(), 0);
      }
    }

    window.addEventListener("keydown", onKeyDown, true);

    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [selectedProject]);

  useEffect(() => {
    if (!searchOpen) {
      return;
    }

    function onEscape(event: KeyboardEvent) {
      if (event.key === "Escape" && !document.querySelector('[role="dialog"]')) {
        setSearchOpen(false);
      }
    }

    window.addEventListener("keydown", onEscape, true);

    return () => window.removeEventListener("keydown", onEscape, true);
  }, [searchOpen]);

  useEffect(() => {
    if (!searchOpen) {
      setSearchResults([]);
      setSearching(false);
      setSearchError(null);

      return;
    }

    const query = searchQuery.trim();

    if (query.length < minimumSearchQuery) {
      setSearchResults([]);
      setSearching(false);
      setSearchError(null);

      return;
    }

    let live = true;
    const conversationId = searchScope === "conversation" ? selectedId : null;

    setSearching(true);

    const timer = window.setTimeout(() => {
      searchMessages({ query, conversationId, limit: searchResultLimit })
        .then((hits) => {
          if (live) {
            setSearchResults(hits);
            setSearchError(null);
          }
        })
        .catch((error) => {
          if (live) {
            setSearchResults([]);
            setSearchError(toFoundryCommandError(error));
          }
        })
        .finally(() => {
          if (live) {
            setSearching(false);
          }
        });
    }, 140);

    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [searchOpen, searchQuery, searchScope, selectedId]);

  useEffect(() => {
    if (highlightId === null) {
      return;
    }

    const timer = window.setTimeout(() => setHighlightId(null), 2400);

    return () => window.clearTimeout(timer);
  }, [highlightId]);

  useEffect(() => {
    const target = pendingJump.current;

    if (!target || target.conversationId !== selectedId || loadingThread) {
      return;
    }

    if (messages.some((message) => message.id === target.messageId)) {
      pendingJump.current = null;
      setHighlightId(target.messageId);

      return;
    }

    if (hasMore && !loadingOlder) {
      void loadOlder();
    }
  }, [jumpNonce, messages, selectedId, hasMore, loadingOlder, loadingThread]);

  const refreshModels = useCallback(async () => {
    try {
      const snapshot = await workspaceSnapshot();

      setLoaded(snapshot.loaded.models);
      setLoadedError(null);
      setCatalog(snapshot.catalog);
    } catch (error) {
      setLoaded([]);
      setLoadedError(toFoundryCommandError(error));
      setCatalog([]);
    }
  }, []);

  const reloadProjects = useCallback(async () => {
    setProjectsLoading(true);
    setProjectsLoadError(null);
    try {
      setProjects(await projectApi.list());
    } catch (error) {
      setProjectsLoadError(projectErrorLabel(error, t));
    } finally {
      setProjectsLoading(false);
    }
  }, [t]);

  useEffect(() => { void reloadProjects(); }, [reloadProjects]);

  useEffect(() => {
    projectWorkspaceMounted.current = true;
    return () => {
      projectWorkspaceMounted.current = false;
      const selection = pendingFolderSelection.current;
      if (selection) void projectApi.discardFolder(selection.selectionId).catch(() => undefined);
    };
  }, []);

  function beginScopeNavigation() {
    scopeRequest.current += 1;
    return ++scopeNavigation.current;
  }

  function commitScopeNavigation(
    nextProjectId: number | null,
    nextArchived: boolean,
    nextProjectsView: boolean,
    preferredId: number | null,
    navigation = beginScopeNavigation(),
  ) {
    selectionIntent.current = preferredId === null ? null : { navigation, id: preferredId };
    setProjectId(nextProjectId);
    setArchivedView(nextArchived);
    setProjectsView(nextProjectsView);
    selectedIdRef.current = preferredId;
    setSelectedId(preferredId);
    setConversations(null);
    setScopeRevision(navigation);
  }

  const installScopeEntries = useCallback((entries: ProjectConversation[]) => {
    const next = entries.map((entry) => entry.conversation);
    const intent = selectionIntent.current;
    const preferredId = intent?.navigation === scopeNavigation.current ? intent.id : null;
    setConversations(next);
    setSelectedId((current) => {
      if (preferredId !== null && next.some((entry) => entry.id === preferredId)) return preferredId;
      return next.some((entry) => entry.id === current) ? current : next[0]?.id ?? null;
    });
  }, []);

  const loadScopeConversations = useCallback(async (nextProjectId: number | null, archived: boolean, navigation: number) => {
    if (navigation !== scopeNavigation.current) return;
    const request = ++scopeRequest.current;
    setConversations(null);
    try {
      const entries = await projectApi.listConversations(nextProjectId, archived);
      if (navigation !== scopeNavigation.current || request !== scopeRequest.current) return;
      installScopeEntries(entries);
    } catch (error) {
      if (navigation !== scopeNavigation.current || request !== scopeRequest.current) return;
      setConversations([]);
      setProjectError(projectErrorLabel(error, t));
    }
  }, [installScopeEntries, t]);

  async function pickProject() {
    if (projectBusy || active || sending || pendingFolderSelection.current) return;
    setProjectBusy(true);
    setProjectError(null);
    try {
      const selection = await projectApi.pickFolder();
      if (!selection) return;
      if (!projectWorkspaceMounted.current) {
        await projectApi.discardFolder(selection.selectionId);
        return;
      }
      pendingFolderSelection.current = selection;
      setProjectSelectionExpired(false);
      setProjectFolderSelection(selection);
    } catch (error) {
      setProjectError(projectErrorLabel(error, t));
    } finally {
      setProjectBusy(false);
    }
  }

  async function cancelProjectCreation() {
    const selection = pendingFolderSelection.current;
    if (!selection || projectBusy) return;
    setProjectBusy(true);
    try {
      await projectApi.discardFolder(selection.selectionId);
      pendingFolderSelection.current = null;
      setProjectFolderSelection(null);
      setProjectSelectionExpired(false);
      setProjectError(null);
    } catch (error) {
      setProjectError(projectErrorLabel(error, t));
    } finally {
      setProjectBusy(false);
    }
  }

  async function confirmProjectCreation(name: string) {
    const selection = pendingFolderSelection.current;
    if (!selection || projectBusy || active || sending || projectSelectionExpired) return;
    setProjectBusy(true);
    setProjectError(null);
    const navigation = scopeRevision;
    try {
      const project = await projectApi.confirmFolder(selection.selectionId, name);
      pendingFolderSelection.current = null;
      if (!projectWorkspaceMounted.current) return;
      setProjectFolderSelection(null);
      setProjects((current) => [...current.filter((entry) => entry.id !== project.id), project]);
      if (navigation !== scopeNavigation.current) return;
      commitScopeNavigation(project.id, false, true, null);
      setListQuery("");
      setProjectNotice(project.created ? null : t("projects.duplicate"));
    } catch (error) {
      const code = typeof error === "object" && error !== null && "code" in error ? error.code : null;
      const expired = code === "ProjectStaleContext" || code === "ProjectInvalidSelection";
      setProjectSelectionExpired(expired);
      setProjectError(expired ? t("projects.selectionExpired") : projectErrorLabel(error, t));
    } finally {
      setProjectBusy(false);
    }
  }

  function switchOrganization(value: boolean) {
    commitScopeNavigation(null, false, value, null);
    setListQuery("");
    setProjectNotice(null);
    setProjectError(null);
  }

  async function selectProject(project: ChatProject) {
    const navigation = beginScopeNavigation();
    setProjectError(null);
    try {
      const currentProject = await projectApi.get(project.id);
      if (navigation !== scopeNavigation.current) return;
      setProjects((current) => current.map((entry) => entry.id === currentProject.id ? currentProject : entry));
      commitScopeNavigation(currentProject.id, false, true, null, navigation);
      setListQuery("");
      setProjectNotice(null);
    } catch (error) {
      if (navigation !== scopeNavigation.current) return;
      setProjectError(projectErrorLabel(error, t));
      setScopeRevision(navigation);
    }
  }

  function openProjectMove(trigger: HTMLButtonElement) {
    const details = trigger.closest("details");
    moveReturnFocus.current = details?.querySelector("summary") ?? null;
    details?.removeAttribute("open");
    setProjectError(null);
    setProjectMoveOpen(true);
  }

  async function renameProject(name: string) {
    if (!selectedProject || projectBusy || active || sending) return false;
    setProjectBusy(true);
    setProjectError(null);
    try {
      const updated = await projectApi.rename(selectedProject.id, name);
      setProjects((current) => current.map((project) => project.id === updated.id ? updated : project));
      return true;
    } catch (error) {
      setProjectError(projectErrorLabel(error, t));
      return false;
    } finally {
      setProjectBusy(false);
    }
  }

  async function moveProjectConversation(destination: number | null) {
    if (!selected || projectBusy || active || sending) return;
    setProjectBusy(true);
    setProjectError(null);
    const navigation = scopeRevision;
    try {
      const receipt = await projectApi.moveConversation(selected.id, destination);
      if (!receipt) return;
      const context = receipt.contexts.find((entry) => entry.conversationId === selected.id);
      if (!context) throw new Error("Missing committed conversation context");
      setProjectMoveOpen(false);
      if (navigation !== scopeNavigation.current) return;
      commitScopeNavigation(context.projectId, archivedView, context.projectId !== null, selected.id);
      setListQuery("");
    } catch (error) {
      if (navigation === scopeNavigation.current) setProjectError(projectErrorLabel(error, t));
    } finally {
      setProjectBusy(false);
    }
  }

  async function removeProject() {
    if (!selectedProject || projectBusy || active || sending) return;
    setProjectBusy(true);
    setProjectError(null);
    const navigation = scopeRevision;
    try {
      const receipt = await projectApi.remove(selectedProject.id);
      if (!receipt) return;
      setProjects((current) => current.filter((project) => project.id !== receipt.removedProjectId));
      for (const id of receipt.deletedConversationIds) clearAttachments(id);
      if (navigation !== scopeNavigation.current) return;
      commitScopeNavigation(null, archivedView, false, selectedId);
      setListQuery("");
      setProjectNotice(t("projects.removed"));
    } catch (error) {
      if (navigation === scopeNavigation.current) setProjectError(projectErrorLabel(error, t));
    } finally {
      setProjectBusy(false);
    }
  }

  async function relocateProject() {
    if (!selectedProject || projectBusy || active || sending) return;
    setProjectBusy(true);
    setProjectError(null);
    const navigation = scopeRevision;
    try {
      const updated = await projectApi.pickRelocate(selectedProject.id);
      if (!updated) return;
      setProjects((current) => current.map((project) => project.id === updated.id ? updated : project));
      await refreshConversations(navigation);
    } catch (error) {
      if (navigation === scopeNavigation.current) setProjectError(projectErrorLabel(error, t));
    } finally {
      setProjectBusy(false);
    }
  }

  const isModelLoaded = useCallback(
    (alias: string) => (loaded ?? []).some((model) => matchesModelAlias(model, alias)),
    [loaded],
  );

  useEffect(() => {
    void loadScopeConversations(projectId, archivedView, scopeRevision);
    return () => { scopeRequest.current += 1; };
  }, [projectId, archivedView, scopeRevision, loadScopeConversations]);

  useEffect(() => { void refreshModels(); }, [refreshModels]);

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
      push(t("models.actions.downloaded", { alias: job.alias }), "neutral");
      void refreshModels();
    }
  });

  useEffect(() => {
    if (!selectedModelAlias) {
      setInputModalities(null);

      return;
    }

    let live = true;

    catalogModel(selectedModelAlias)
      .then((model) => {
        if (live) {
          setInputModalities(model.inputModalities);
        }
      })
      .catch(() => {
        if (live) {
          setInputModalities(null);
        }
      });

    return () => {
      live = false;
    };
  }, [selectedModelAlias]);

  useEffect(() => {
    if (selectedId === null) {
      setMessages([]);
      setHasMore(false);
      setLoadingThread(false);

      return;
    }

    let live = true;

    setLoadingThread(true);

    listMessages(selectedId, null, THREAD_PAGE_SIZE)
      .then((page) => {
        if (!live) {
          return;
        }

        setMessages(page.messages);
        setHasMore(page.hasMore);
      })
      .catch((error) => {
        if (live) {
          setBanner(toFoundryCommandError(error));
        }
      })
      .finally(() => {
        if (live) {
          setLoadingThread(false);
        }
      });

    return () => {
      live = false;
    };
  }, [selectedId]);

  useEffect(() => {
    if (selectedId === null) {
      setUsage(null);

      return;
    }

    let live = true;

    setUsage(null);

    conversationUsage(selectedId)
      .then((next) => {
        if (live) {
          setUsage(next);
        }
      })
      .catch(() => {
        if (live) {
          setUsage(null);
        }
      });

    return () => {
      live = false;
    };
  }, [selectedId]);

  const handleStreamEvent = useCallback((event: StreamEvent) => {
    if (event.type === "started") {
      setConversations((current) =>
        mergeVisibleConversation(current, event.conversation),
      );
      if (selectedIdRef.current === event.conversation.id) {
        setMessages((current) => mergeMessages(current, [event.userMessage, event.assistantMessage]));
      }

      return;
    }

    if (event.type === "delta") {
      setMessages((current) =>
        current.map((message) =>
          message.id === event.messageId
            ? {
                ...message,
                content: message.content + event.content,
                reasoning: message.reasoning + event.reasoning,
              }
            : message,
        ),
      );

      return;
    }

    if (event.type === "usage") {
      setMessages((current) =>
        current.map((message) =>
          message.id === event.messageId ? { ...message, usage: event.usage } : message,
        ),
      );

      return;
    }

    finishedRef.current.add(event.requestId);
    setConversations((current) =>
      mergeVisibleConversation(current, event.conversation),
    );

    const conversationId = event.conversation.id;

    conversationUsage(conversationId)
      .then((next) => {
        if (selectedIdRef.current === conversationId) {
          setUsage(next);
        }
      })
      .catch(() => undefined);

    if (selectedIdRef.current === event.conversation.id) {
      setMessages((current) => mergeMessages(current, [event.message]));
    }
    if (event.type !== "done") {
      setBanner(new FoundryCommandError(event.error));
    }

    if (activeRef.current?.requestId === event.requestId) {
      activeRef.current = null;
      setActive(null);
      setStopping(false);
    }
  }, []);

  function updateDraft(value: string) {
    if (selectedId === null) {
      return;
    }

    setDrafts((current) => ({ ...current, [selectedId]: value }));
  }

  function mentionProjectFile(path: string) {
    if (selectedId === null) {
      return;
    }

    const reference = `@${path}`;
    const trimmed = draft.trimEnd();
    updateDraft(trimmed ? `${trimmed} ${reference}` : reference);
  }

  function openProjectFile(path: string) {
    if (selectedProject) {
      void editor.openFile(selectedProject.id, path);
    }
  }

  async function refreshConversations(navigation: number) {
    await loadScopeConversations(projectId, archivedView, navigation);
  }

  async function switchArchivedView() {
    const next = !archivedView;

    commitScopeNavigation(projectId, next, projectsView, null);
    setListQuery("");
    setDeleteTarget(null);
    pendingJump.current = null;
    setHighlightId(null);

  }

  async function toggleFavorite(conversation: Conversation) {
    if (active) {
      return;
    }

    const navigation = scopeNavigation.current;
    try {
      const updated = await setConversationFavorite(conversation.id, !conversation.favorite);
      if (navigation !== scopeNavigation.current) return;
      setConversations((current) => mergeVisibleConversation(current, updated));
    } catch (error) {
      if (navigation === scopeNavigation.current) setBanner(toFoundryCommandError(error));
    }
  }

  function removeDisplayedConversation(id: number) {
    setConversations((current) => current?.filter((entry) => entry.id !== id) ?? current);
    if (selectedIdRef.current !== id) return;
    pendingJump.current = null;
    setHighlightId(null);
    setMessages([]);
    if (selectionIntent.current?.id === id) selectionIntent.current = null;
    selectedIdRef.current = null;
    setSelectedId((current) => current === id ? null : current);
  }

  async function toggleArchived(conversation: Conversation) {
    if (active) {
      return;
    }

    const navigation = scopeNavigation.current;
    try {
      const updated = await setConversationArchived(conversation.id, !conversation.archived);
      if (navigation !== scopeNavigation.current) return;
      removeDisplayedConversation(updated.id);
    } catch (error) {
      if (navigation === scopeNavigation.current) setBanner(toFoundryCommandError(error));
    }
  }

  useEffect(() => {
    if (openConversationId === null) {
      return;
    }

    if (conversations?.some((conversation) => conversation.id === openConversationId)) {
      pendingJump.current = null;
      setHighlightId(null);
      selectionIntent.current = { navigation: scopeNavigation.current, id: openConversationId };
      setSelectedId(openConversationId);
      onOpenConversationHandled?.();

      return;
    }

    let live = true;

    openScopedConversation(openConversationId).finally(() => {
      if (live) onOpenConversationHandled?.();
    });

    return () => {
      live = false;
    };
  }, [openConversationId, onOpenConversationHandled]);

  async function openScopedConversation(id: number): Promise<boolean> {
    const navigation = beginScopeNavigation();
    try {
      const context = await projectApi.conversationContext(id);
      let nextArchived = false;
      let entries = await projectApi.listConversations(context.projectId, false);
      if (!entries.some((entry) => entry.conversation.id === id)) {
        nextArchived = true;
        entries = await projectApi.listConversations(context.projectId, true);
      }
      if (!entries.some((entry) => entry.conversation.id === id)) {
        if (navigation !== scopeNavigation.current) return false;
        setProjectError(t("projects.errors.ProjectNotFound"));
        setScopeRevision(navigation);
        return false;
      }
      if (context.projectId !== null) {
        const project = await projectApi.get(context.projectId);
        if (navigation !== scopeNavigation.current) return false;
        setProjects((current) => [...current.filter((entry) => entry.id !== project.id), project]);
      }
      if (navigation !== scopeNavigation.current) return false;
      commitScopeNavigation(context.projectId, nextArchived, context.projectId !== null, id, navigation);
      installScopeEntries(entries);
      setListQuery("");
      setProjectNotice(null);
      setSelectedId(id);
      return true;
    } catch (error) {
      if (navigation !== scopeNavigation.current) return false;
      setProjectError(projectErrorLabel(error, t));
      setScopeRevision(navigation);
      return false;
    }
  }

  async function startConversation() {
    const modelAlias = chatModels[0]?.alias ?? cachedChatModels[0]?.alias;

    if (!modelAlias || active || sending || projectBusy) {
      return;
    }

    const navigation = scopeNavigation.current;
    setProjectError(null);
    setProjectBusy(true);
    try {
      const entry = await projectApi.createConversation(projectId, modelAlias);
      if (navigation !== scopeNavigation.current) return;
      const conversation = entry.conversation;
      selectionIntent.current = { navigation, id: conversation.id };

      if (archivedView) {
        commitScopeNavigation(projectId, false, projectsView, conversation.id, navigation);
        setConversations([conversation]);
      } else {
        setConversations((current) => mergeConversation(current ?? [], conversation));
      }

      setSelectedId(conversation.id);
      setBanner(null);
    } catch (error) {
      if (navigation !== scopeNavigation.current) return;
      setProjectError(projectErrorLabel(error, t));
    } finally {
      setProjectBusy(false);
    }
  }

  async function runModelLoad(alias: string, announce = true): Promise<boolean> {
    setLoadingModelAlias(alias);

    try {
      await loadModel(alias);
      await refreshModels();
      setBanner(null);

      if (announce) {
        push(t("models.actions.loaded", { alias }), "neutral");
      }

      return true;
    } finally {
      setLoadingModelAlias(null);
    }
  }

  async function loadChatModelAlias(alias: string) {
    if (loadingModelAlias || active !== null || sending || regenerateBusy || editBusy) {
      return;
    }

    try {
      await runModelLoad(alias, true);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    }
  }

  async function downloadChatModelAlias(alias: string) {
    try {
      await downloadModel(alias);
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    }
  }

  async function ensureConversationModelLoaded(conversation: Conversation) {
    const target = conversationLoadTarget(conversation);

    if (isModelLoaded(target)) {
      return;
    }

    await runModelLoad(target, true);
  }

  async function recoverServerConnection(refreshList: boolean): Promise<boolean> {
    if (reconnecting) {
      return false;
    }

    setReconnecting(true);
    const navigation = scopeRevision;

    try {
      await startServer();
      await refreshModels();

      if (refreshList) {
        await refreshConversations(navigation);
      }

      if (navigation === scopeNavigation.current) setBanner(null);
      return true;
    } catch (error) {
      if (navigation === scopeNavigation.current) setBanner(toFoundryCommandError(error));
      return false;
    } finally {
      setReconnecting(false);
    }
  }

  async function recoverFromGenerationFailure(
    failure: FoundryCommandError,
    conversation: Conversation,
  ): Promise<boolean> {
    if (failure.code === "ServerUnreachable") {
      return recoverServerConnection(false);
    }

    if (failure.code === "ModelNotLoaded") {
      try {
        await runModelLoad(conversationLoadTarget(conversation), true);
        return true;
      } catch (error) {
        setBanner(toFoundryCommandError(error));
        return false;
      }
    }

    return false;
  }

  async function runWithGenerationRecovery(
    conversation: Conversation,
    operation: () => Promise<SendReceipt>,
  ): Promise<SendReceipt> {
    try {
      return await operation();
    } catch (error) {
      const failure = toFoundryCommandError(error);
      const recovered = await recoverFromGenerationFailure(failure, conversation);

      if (!recovered) {
        throw failure;
      }

      return operation();
    }
  }

  async function send() {
    const content = draft.trim();
    const tokens = attachments.map((attachment) => attachment.token);

    if (!selected || (!content && tokens.length === 0) || active || agent.busy || sending || loadingModelAlias !== null) {
      return;
    }

    const conversationId = selected.id;

    setSending(true);
    setBanner(null);
    pendingJump.current = null;

    try {
      await ensureConversationModelLoaded(selected);

      if (agentCompatible) {
        if (tokens.length > 0) return;
        const receipt = await agent.start(content);
        setDrafts((current) => ({ ...current, [conversationId]: "" }));
        setConversations((current) => mergeVisibleConversation(current, receipt.conversation));
        if (selectedIdRef.current === conversationId) {
          setMessages((current) => mergeMessages(current, [receipt.userMessage, receipt.assistantMessage]));
          refreshAgentMessages(conversationId);
        }
        return;
      }

      const receipt = await runWithGenerationRecovery(selected, () =>
        sendChatMessage({
          conversationId,
          content,
          attachments: tokens,
          onEvent: handleStreamEvent,
        }),
      );

      setDrafts((current) => ({ ...current, [conversationId]: "" }));
      clearAttachments(conversationId);
      applyReceipt(receipt, conversationId);
    } catch (error) {
      const failure = toFoundryCommandError(error);

      setBanner(failure);

      if (failure.code === "UnsupportedImage") {
        clearAttachments(conversationId);
      }
    } finally {
      setSending(false);
    }
  }

  function applyReceipt(receipt: SendReceipt, conversationId: number) {
    const removed = new Set(receipt.removed);

    setConversations((current) =>
      mergeVisibleConversation(current, receipt.conversation),
    );
    if (selectedIdRef.current === conversationId) {
      setMessages((current) =>
        mergeMessages(
          current.filter((message) => !removed.has(message.id)),
          [receipt.userMessage, receipt.assistantMessage],
        ),
      );
    }

    if (!finishedRef.current.has(receipt.requestId)) {
      const next = {
        requestId: receipt.requestId,
        conversationId,
      };

      activeRef.current = next;
      setActive(next);
      setStopping(false);
    }
  }

  async function confirmRegenerate() {
    if (!selected || regenerateBusy || active || loadingModelAlias !== null) {
      return;
    }

    const conversationId = selected.id;

    setRegenerateBusy(true);
    setBanner(null);
    pendingJump.current = null;

    try {
      await ensureConversationModelLoaded(selected);

      const receipt = await runWithGenerationRecovery(selected, () =>
        regenerateMessage(conversationId, handleStreamEvent),
      );

      applyReceipt(receipt, conversationId);
      setRegenerateOpen(false);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setRegenerateBusy(false);
    }
  }

  function startEdit(message: ChatMessage) {
    setEditTarget(message);
    setEditValue(message.content);
  }

  async function confirmEdit() {
    const content = editValue.trim();

    if (!editTarget || !content || editBusy || active || loadingModelAlias !== null) {
      return;
    }

    setEditBusy(true);
    setBanner(null);
    pendingJump.current = null;

    const conversation = conversations?.find((entry) => entry.id === editTarget.conversationId) ?? null;

    if (!conversation) {
      setEditBusy(false);
      return;
    }

    try {
      await ensureConversationModelLoaded(conversation);

      const receipt = await runWithGenerationRecovery(conversation, () =>
        editChatMessage(editTarget.id, content, handleStreamEvent),
      );

      applyReceipt(receipt, receipt.conversation.id);
      setEditTarget(null);
      setEditValue("");
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setEditBusy(false);
    }
  }

  async function runExport(format: ExportFormat) {
    if (!selected || exporting) {
      return;
    }

    setExporting(true);

    try {
      const labels = {
        untitled: t("chat.untitled"),
        user: t("chat.export.document.user"),
        assistant: t("chat.export.document.assistant"),
        thinking: t("chat.reasoning"),
        model: t("chat.export.document.model"),
        messages: t("chat.export.document.messages"),
        modelChanged: t("chat.export.document.modelChanged"),
        tokens: t("chat.export.document.tokens"),
        imageAttachmentsOne: t("chat.export.document.imageAttachmentsOne"),
        imageAttachmentsOther: t("chat.export.document.imageAttachmentsOther"),
      };
      const receipt = await exportConversation(selected.id, format, labels);
      const file = receipt.path.split(/[\\/]/).pop() ?? receipt.path;

      push(t("chat.export.done", { file }), "neutral");
    } catch {
      push(t("chat.export.failed"), "danger");
    } finally {
      setExporting(false);
    }
  }

  function selectConversation(id: number) {
    selectionIntent.current = { navigation: scopeNavigation.current, id };
    selectedIdRef.current = id;
    pendingJump.current = null;
    setHighlightId(null);
    setSelectedId(id);
  }

  async function selectSearchHit(hit: SearchHit) {
    if (!conversations?.some((conversation) => conversation.id === hit.conversationId)) {
      if (!(await openScopedConversation(hit.conversationId))) return;
    }
    selectionIntent.current = { navigation: scopeNavigation.current, id: hit.conversationId };
    if (hit.conversationId !== selectedIdRef.current) {
      setSelectedId(hit.conversationId);
    }
    selectedIdRef.current = hit.conversationId;

    pendingJump.current = {
      conversationId: hit.conversationId,
      messageId: hit.messageId,
    };
    setJumpNonce((current) => current + 1);
  }

  async function attachFiles(files: File[]) {
    if (!selected || !attachEnabled) {
      return;
    }

    const conversationId = selected.id;
    let count = (staged[conversationId] ?? []).length;

    for (const file of files) {
      if (count >= maxImagesPerMessage) {
        push(t("chat.attachTooMany", { count: maxImagesPerMessage }), "danger");
        break;
      }

      const rejection = imageRejection(file);

      if (rejection === "unsupported") {
        push(t("chat.attachUnsupported"), "danger");
        continue;
      }

      if (rejection === "tooLarge") {
        push(t("chat.attachTooLarge", { size: maxImageBytes / (1024 * 1024) }), "danger");
        continue;
      }

      try {
        const previewUrl = await fileToDataUrl(file);
        const attachment = await stageAttachment(previewUrl);

        count += 1;
        setStaged((current) => ({
          ...current,
          [conversationId]: [
            ...(current[conversationId] ?? []),
            { ...attachment, previewUrl },
          ],
        }));
      } catch (error) {
        push(errorLabel(toFoundryCommandError(error), t), "danger");
      }
    }
  }

  function clearAttachments(conversationId: number) {
    const dropped = staged[conversationId] ?? [];

    if (dropped.length) {
      void discardAttachments(dropped.map((attachment) => attachment.token));
    }

    setStaged((current) => {
      const next = { ...current };

      delete next[conversationId];

      return next;
    });
  }

  function removeAttachment(token: string) {
    if (selectedId === null) {
      return;
    }

    const conversationId = selectedId;

    void discardAttachments([token]);
    setStaged((current) => ({
      ...current,
      [conversationId]: (current[conversationId] ?? []).filter(
        (attachment) => attachment.token !== token,
      ),
    }));
  }

  async function stop() {
    if (agent.busy) {
      await agent.cancel();
      return;
    }
    const current = activeRef.current;

    if (!current) {
      return;
    }

    setStopping(true);

    try {
      const cancelled = await cancelGeneration(current.requestId);

      if (!cancelled) {
        activeRef.current = null;
        setActive(null);
        setStopping(false);
      }
    } catch (error) {
      setBanner(toFoundryCommandError(error));
      setStopping(false);
    }
  }

  async function changeModel(modelAlias: string) {
    if (!selected || selected.modelAlias === modelAlias || active) {
      return;
    }

    const navigation = scopeNavigation.current;
    try {
      const change = await setConversationModel(selected.id, modelAlias);
      if (navigation !== scopeNavigation.current) return;
      setConversations((current) => mergeVisibleConversation(current, change.conversation));

      const divider = change.divider;

      if (divider && selectedIdRef.current === change.conversation.id) {
        setMessages((current) => mergeMessages(current, [divider]));
      }
    } catch (error) {
      if (navigation === scopeNavigation.current) setBanner(toFoundryCommandError(error));
    }
  }

  async function changeVariant(modelVariantId: string) {
    if (!selected || active) {
      return;
    }

    const next = modelVariantId.trim() ? modelVariantId.trim() : null;

    if ((selected.modelVariantId ?? null) === next) {
      return;
    }

    const navigation = scopeNavigation.current;
    try {
      const change = await setConversationVariant(selected.id, next);
      if (navigation !== scopeNavigation.current) return;
      setConversations((current) => mergeVisibleConversation(current, change.conversation));

      const divider = change.divider;

      if (divider && selectedIdRef.current === change.conversation.id) {
        setMessages((current) => mergeMessages(current, [divider]));
      }
    } catch (error) {
      if (navigation === scopeNavigation.current) setBanner(toFoundryCommandError(error));
    }
  }

  function openRename() {
    if (!selected) {
      return;
    }

    setRenameValue(selected.title || t("chat.untitled"));
    setRenameOpen(true);
  }

  async function submitRename() {
    const title = renameValue.trim();

    if (!selected || !title || renaming) {
      return;
    }

    setRenaming(true);
    const navigation = scopeNavigation.current;
    try {
      const conversation = await renameConversation(selected.id, title);
      if (navigation !== scopeNavigation.current) return;
      setConversations((current) => mergeVisibleConversation(current, conversation));
      setRenameOpen(false);
    } catch (error) {
      if (navigation === scopeNavigation.current) setBanner(toFoundryCommandError(error));
    } finally {
      setRenaming(false);
    }
  }

  async function confirmDelete() {
    const target = deleteTarget;

    if (!target || deleting) {
      return;
    }

    setDeleting(true);
    const navigation = scopeNavigation.current;
    try {
      await deleteConversation(target.id);
      if (navigation === scopeNavigation.current) removeDisplayedConversation(target.id);

      setDrafts((current) => {
        const next = { ...current };

        delete next[target.id];

        return next;
      });
      clearAttachments(target.id);
      setDeleteTarget((current) => current?.id === target.id ? null : current);
    } catch (error) {
      if (navigation === scopeNavigation.current) setBanner(toFoundryCommandError(error));
    } finally {
      setDeleting(false);
    }
  }

  async function loadOlder() {
    const oldest = messages[0];

    if (!selected || !oldest || loadingOlder) {
      return;
    }

    const conversationId = selected.id;

    setLoadingOlder(true);

    try {
      const page = await listMessages(conversationId, oldest.seq, THREAD_PAGE_SIZE);

      if (selectedIdRef.current !== conversationId) {
        return;
      }

      setMessages((current) => mergeMessages(current, page.messages));
      setHasMore(page.hasMore);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setLoadingOlder(false);
    }
  }

  async function reconnect() {
    await recoverServerConnection(true);
  }

  async function freeMemory() {
    const candidate = unloadCandidate;

    if (!candidate || freeingMemoryAlias) {
      return;
    }

    setFreeingMemoryAlias(candidate.alias);

    try {
      await unloadModel(candidate.alias);
      await refreshModels();
      setBanner(null);
      push(t("models.actions.unloaded", { alias: candidate.alias }), "neutral");
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setFreeingMemoryAlias(null);
    }
  }

  const bannerAction = useMemo<BannerAction | null>(() => {
    if (!banner) {
      return null;
    }

    if (banner.code === "ServerUnreachable") {
      return {
        label: reconnecting ? t("chat.reconnecting") : t("chat.reconnect"),
        busy: reconnecting,
        onClick: () => void reconnect(),
      };
    }

    if (banner.code === "ModelNotLoaded" && selected) {
      const target = conversationLoadTarget(selected);

      return {
        label: t("models.actions.load"),
        busy: loadingModelAlias === target,
        onClick: () => void loadChatModelAlias(target),
      };
    }

    if (banner.code === "ModelNotCached") {
      return {
        label: t("common.openModels"),
        busy: false,
        onClick: () => onOpenModels?.(),
      };
    }

    if (banner.code === "OutOfMemory") {
      if (unloadCandidate) {
        return {
          label: t("models.actions.unload"),
          busy: freeingMemoryAlias === unloadCandidate.alias,
          onClick: () => void freeMemory(),
        };
      }

      return {
        label: t("common.openModels"),
        busy: false,
        onClick: () => onOpenModels?.(),
      };
    }

    return null;
  }, [
    banner,
    reconnecting,
    selected,
    loadingModelAlias,
    onOpenModels,
    unloadCandidate,
    freeingMemoryAlias,
    reconnect,
    loadChatModelAlias,
    freeMemory,
    t,
  ]);

  const variantOptions: ChatModelOption[] = [
    {
      value: "",
      label: t("chat.variantAuto"),
      hint: t("chat.variantAutoHint"),
    },
    ...(selectedCatalogModel?.variants ?? []).map((variant) => ({
      value: variant.id,
      label: variant.displayName ?? variant.id,
      hint: variantLabel(variant),
    })),
  ];

  if (
    selectedVariantId &&
    !variantOptions.some((option) => option.value === selectedVariantId)
  ) {
    variantOptions.unshift({
      value: selectedVariantId,
      label: selectedVariantId,
      hint: t("chat.variantNotInCatalog"),
    });
  }

  const hasVariantMenu = Boolean(selectedVariantId) || (selectedCatalogModel?.variants.length ?? 0) > 0;

  const composerControls = selected ? (
    <>
      <span className="inline-flex h-7 items-center rounded-[7px] bg-subtle px-2 text-[10px] font-semibold uppercase tracking-[0.04em] text-tertiary">
        {t("params.origin.conversation")}
      </span>
      <ModelPicker
        label={t("chat.model")}
        models={allChatModels}
        selectedAlias={selected.modelAlias}
        disabled={active !== null}
        loadingAlias={loadingModelAlias}
        jobs={jobs}
        placement="up"
        onOpenModels={onOpenModels}
        onUse={(modelAlias) => void changeModel(modelAlias)}
        onLoad={(alias) => void loadChatModelAlias(alias)}
        onDownload={(alias) => void downloadChatModelAlias(alias)}
      />
      {hasVariantMenu ? (
        <ModelMenu
          label={t("chat.variant")}
          value={selected.modelVariantId ?? ""}
          options={variantOptions}
          disabled={active !== null}
          emphasized={Boolean(selected.modelVariantId)}
          onChange={(value) => void changeVariant(value)}
        />
      ) : null}
      <ParamsPopover
        conversationId={selected.id}
        modelAlias={selected.modelAlias}
        supportedFields={selectedCatalogModel?.supportedGenerationParams}
        disabled={active !== null}
      />
      <ExecutionPopover
        agent={agent}
        compatible={agentCompatible}
        hasAttachments={attachments.length > 0}
      />
    </>
  ) : null;

  const emptyTitle = loadedError
    ? t("chat.noModelTitle")
    : chatModels.length === 0
      ? t("chat.noModelTitle")
      : t("chat.emptyTitle");
  const emptyBody = loadedError
    ? errorLabel(loadedError, t)
    : chatModels.length === 0
      ? t("chat.noModelBody")
      : t("chat.emptyBody");
  const missingDownloadedChatModels = catalog !== null && cachedChatModels.length === 0;
  const actionsDisabled = active !== null || sending || agent.busy;
  const filteredConversations = useMemo(() => {
    const query = listQuery.trim().toLocaleLowerCase();

    if (!query) {
      return conversations ?? [];
    }

    return (conversations ?? []).filter((conversation) => {
      const title = (conversation.title || t("chat.untitled")).toLocaleLowerCase();
      const model = conversation.modelAlias.toLocaleLowerCase();

      return title.includes(query) || model.includes(query);
    });
  }, [conversations, listQuery, t]);

  useEffect(() => {
    if (conversations === null) return;
    if (filteredConversations.length === 0) {
      setSelectedId(null);
      return;
    }

    const first = filteredConversations[0];

    if (!first) {
      return;
    }

    if (selectedId === null || !filteredConversations.some((conversation) => conversation.id === selectedId)) {
      setSelectedId(first.id);
    }
  }, [conversations, filteredConversations, selectedId]);

  const projectContext = selectedProject ? (
    <ProjectContext
      key={selectedProject.id}
      project={selectedProject}
      disabled={actionsDisabled}
      busy={projectBusy}
      error={projectError}
      onRename={renameProject}
      onRemove={() => void removeProject()}
      onRelocate={() => void relocateProject()}
    />
  ) : null;

  const listTitle = selectedProject
    ? selectedProject.name
    : archivedView
      ? t("chat.archivedTitle")
      : projectsView
        ? t("projects.title")
        : t("chat.listTitle");

  return (
    <>
      <ContextList
        title={listTitle}
        action={
          <div className="flex items-center gap-0.5">
            {projectsView ? (
              <IconButton
                label={t("projects.new")}
                onClick={() => void pickProject()}
                disabled={projectBusy || actionsDisabled}
                className={projectControlFocus}
              >
                <PlusIcon size={15} />
              </IconButton>
            ) : null}
            {!projectsView || selectedProject ? <>
            <IconButton
              label={archivedView ? t("chat.archivedBack") : t("chat.archivedTitle")}
              onClick={() => void switchArchivedView()}
              className={archivedView ? "border-line-strong bg-active text-primary" : ""}
            >
              <ArchiveIcon size={14} />
            </IconButton>
            <IconButton
              label={t("chat.listFilterPlaceholder")}
              onClick={() => setListFilterOpen((current) => !current)}
            >
              <SearchIcon size={14} />
            </IconButton>
            <IconButton
              label={t("chat.new")}
              onClick={() => void startConversation()}
              disabled={!cachedChatModels.length || actionsDisabled || projectBusy}
            >
              <PlusIcon size={15} />
            </IconButton>
            </> : null}
          </div>
        }
        toolbar={
          <div className="space-y-2">
          {selectedProject ? (
            <button
              type="button"
              onClick={() => switchOrganization(true)}
              className={cx("inline-flex h-7 items-center gap-1 rounded-[7px] px-2 text-[12px] font-medium text-primary transition-colors hover:bg-hover", projectControlFocus)}
            >
              <ChevronLeftIcon size={14} />
              {t("projects.all")}
            </button>
          ) : (
            <ProjectNavigation projectsView={projectsView} onChange={switchOrganization} />
          )}
          {projectError ? <p role="alert" className="text-[12px] leading-relaxed text-danger">{projectError}</p> : null}
          {projectNotice ? <p role="status" className="text-[12px] leading-relaxed text-primary">{projectNotice}</p> : null}
          {(!projectsView || selectedProject) && (listFilterOpen || listQuery.trim()) ? (
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
                placeholder={t("chat.listFilterPlaceholder")}
                aria-label={t("chat.listFilterPlaceholder")}
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
                  className="absolute right-1 top-1/2 inline-flex h-5 w-5 -translate-y-1/2 items-center justify-center rounded text-tertiary transition-colors hover:bg-hover hover:text-primary"
                >
                  <CloseIcon size={12} />
                </button>
              ) : null}
            </div>
          ) : null}
          </div>
        }
      >
        {projectsView && !selectedProject ? (
          <ProjectsPanel
            projects={projects}
            selectedId={projectId}
            loading={projectsLoading}
            error={projectsLoadError}
            onSelect={selectProject}
            onRetry={() => void reloadProjects()}
          />
        ) : (
          <>
            {selectedProject ? (
              <ProjectFilesPanel
                projectId={selectedProject.id}
                onOpen={openProjectFile}
                onAttachText={mentionProjectFile}
              />
            ) : null}
            {conversations === null ? (
              <div className="flex justify-center py-6">
                <Spinner size={14} className="text-tertiary" />
              </div>
            ) : conversations.length === 0 ? (
              <ListEmpty
                icon={archivedView ? ArchiveIcon : ChatIcon}
                title={
                  archivedView ? t("chat.archivedEmptyTitle") : t("chat.listEmptyTitle")
                }
                body={archivedView ? t("chat.archivedEmptyBody") : t("chat.listEmptyBody")}
              />
            ) : filteredConversations.length === 0 ? (
              <ListEmpty
                icon={SearchIcon}
                title={t("chat.listNoMatchesTitle")}
                body={t("chat.listNoMatchesBody")}
              />
            ) : (
              <ConversationList
                conversations={filteredConversations}
                selectedId={selectedId}
                streamingConversationId={streamingConversationId}
                disabled={active !== null}
                archivedView={archivedView}
                onSelect={selectConversation}
                onToggleFavorite={(conversation) => void toggleFavorite(conversation)}
                onToggleArchive={(conversation) => void toggleArchived(conversation)}
                onRequestDelete={setDeleteTarget}
              />
            )}
          </>
        )}
      </ContextList>

      <div className="flex min-h-0 min-w-0 flex-1">
        <EditorPanel editor={editor} />
        {selected ? (
          <section className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-canvas">
          <header
            className="flex h-[var(--titlebar-inset)] shrink-0 items-center justify-between gap-3 border-b border-line px-5"
            data-tauri-drag-region
            onMouseDown={dragWindowFromMouseDown}
          >
            <div className="flex min-w-0 items-center gap-2" data-tauri-drag-region>
              <h1 className="truncate text-[15px] font-semibold tracking-[-0.01em] text-primary">
                {selected.title || t("chat.untitled")}
              </h1>
            </div>
            <div className="flex shrink-0 items-center gap-1">
              <IconButton label={t("chat.search.label")} onClick={() => setSearchOpen(true)}>
                <SearchIcon size={15} />
              </IconButton>
              <details className="group relative">
                <summary className="inline-flex h-7 list-none items-center gap-1.5 rounded-[7px] border border-line px-2.5 text-[11.5px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary">
                  {t("chat.conversationActions")}
                  <ChevronDownIcon size={12} className="text-tertiary transition-transform group-open:rotate-180" />
                </summary>
                <div className="absolute right-0 top-full z-30 mt-1 w-[min(94vw,460px)] rounded-lg border border-line bg-elevated p-3 shadow-[var(--shadow-popover)]">
                  <div className="flex flex-wrap items-center gap-2 border-b border-line pb-2.5">
                    <span className="min-w-0 flex-1 truncate text-[11.5px] text-secondary">{selected.modelAlias}</span>
                    {usage ? <TokenUsageBadge usage={usage} /> : null}
                  </div>
                  <div className="mt-2.5 flex flex-wrap items-center gap-2">
                    <button
                      type="button"
                      disabled={actionsDisabled || projectBusy}
                      onClick={(event) => openProjectMove(event.currentTarget)}
                      className={cx("rounded-[7px] border border-line px-2.5 py-1.5 text-[12px] text-primary hover:bg-hover disabled:opacity-50", projectControlFocus)}
                    >
                      {t("projects.move")}
                    </button>
                    <ExportMenu
                      disabled={active !== null}
                      busy={exporting}
                      onExport={(format) => void runExport(format)}
                    />
                  </div>
                  <div className="mt-2.5 flex items-center justify-end gap-1 border-t border-line pt-2.5">
                    <IconButton
                      label={t("chat.rename")}
                      onClick={openRename}
                      disabled={active !== null}
                    >
                      <PencilIcon size={15} />
                    </IconButton>
                    <IconButton
                      label={t("chat.delete")}
                      onClick={() => setDeleteTarget(selected)}
                      disabled={active !== null}
                    >
                      <TrashIcon size={15} />
                    </IconButton>
                  </div>
                </div>
              </details>
            </div>
          </header>

          {projectContext}

          {banner ? (
            <ErrorBanner
              error={banner}
              action={bannerAction}
              onDismiss={() => setBanner(null)}
            />
          ) : null}

          {searchOpen ? (
            <SearchPanel
              inputRef={searchInputRef}
              query={searchQuery}
              scope={searchScope}
              results={searchResults}
              searching={searching}
              error={searchError}
              onQueryChange={setSearchQuery}
              onScopeChange={setSearchScope}
              onSelect={selectSearchHit}
              onClose={() => setSearchOpen(false)}
            />
          ) : null}

          {loadingThread ? (
            <div className="flex min-h-0 flex-1 items-center justify-center">
              <Spinner size={16} className="text-tertiary" />
            </div>
          ) : (
            <MessageList
              key={`messages-${selected.id}`}
              messages={messages}
              hasMore={hasMore}
              loadingOlder={loadingOlder}
              highlightId={highlightId}
              actionsDisabled={actionsDisabled}
              onLoadOlder={() => void loadOlder()}
              onEdit={startEdit}
              onRegenerate={() => setRegenerateOpen(true)}
              toolsByMessage={toolsByMessage}
            />
          )}

          <Composer
            value={draft}
            sending={sending}
            active={active !== null || agent.busy}
            stopping={stopping || agent.stopping}
            modelLabel={selected.modelAlias}
            projectId={projectId}
            attachments={attachments}
            attachEnabled={attachEnabled}
            attachHint={attachHint}
            onChange={updateDraft}
            onSend={() => void send()}
            onStop={() => void stop()}
            onAttach={(files) => void attachFiles(files)}
            onRemoveAttachment={removeAttachment}
            controls={composerControls}
          />
          </section>
        ) : (
          <section className="flex h-full min-h-0 min-w-0 flex-1 flex-col bg-canvas">
          <header
            className="flex h-[var(--titlebar-inset)] shrink-0 items-center border-b border-line px-5"
            data-tauri-drag-region
            onMouseDown={dragWindowFromMouseDown}
          >
            <h1 className="min-w-0 truncate text-[15px] font-semibold tracking-[-0.01em] text-primary" title={selectedProject?.name}>
              {selectedProject?.name ?? t("chat.title")}
            </h1>
          </header>
          {projectContext}
          {banner ? (
            <ErrorBanner
              error={banner}
              action={bannerAction}
              onDismiss={() => setBanner(null)}
            />
          ) : null}
          <EmptyState
            icon={ChatIcon}
            title={emptyTitle}
            body={emptyBody}
            {...(missingDownloadedChatModels
              ? {
                  action: (
                    <button
                      type="button"
                      onClick={() => onOpenModels?.()}
                      className="inline-flex h-8 items-center gap-1.5 rounded-md border border-line px-3 text-[12px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary"
                    >
                      <DownloadIcon size={12} />
                      {t("models.actions.download")}
                    </button>
                  ),
                }
              : {})}
          />
        </section>
        )}
      </div>

      {projectFolderSelection ? (
        <ProjectCreateDialog
          key={projectFolderSelection.selectionId}
          selection={projectFolderSelection}
          busy={projectBusy}
          error={projectError}
          expired={projectSelectionExpired}
          onCancel={() => void cancelProjectCreation()}
          onConfirm={confirmProjectCreation}
        />
      ) : null}
      {projectMoveOpen ? (
        <ProjectMoveDialog
          projects={projects}
          projectId={projectId}
          busy={projectBusy}
          error={projectError}
          onClose={() => setProjectMoveOpen(false)}
          onMove={moveProjectConversation}
        />
      ) : null}
      <Modal open={renameOpen} title={t("chat.renameTitle")} onClose={() => setRenameOpen(false)}>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void submitRename();
          }}
        >
          <label
            className="mb-1 block text-[11px] font-medium text-secondary"
            htmlFor="chat-rename"
          >
            {t("chat.renameLabel")}
          </label>
          <input
            id="chat-rename"
            autoFocus
            value={renameValue}
            onChange={(event) => setRenameValue(event.target.value)}
            className="h-8 w-full rounded-[7px] border border-line bg-canvas px-2 text-[13px] text-primary outline-none focus:border-line-strong"
          />
          <div className="mt-4 flex flex-wrap justify-end gap-2">
            <button
              type="button"
              onClick={() => setRenameOpen(false)}
              disabled={renaming}
              className="inline-flex h-8 w-full items-center justify-center rounded-md border border-line px-3 text-[11.5px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary disabled:opacity-60 sm:w-auto"
            >
              {t("common.cancel")}
            </button>
            <button
              type="submit"
              disabled={renaming || !renameValue.trim()}
              className="inline-flex h-8 w-full items-center justify-center rounded-md bg-accent px-3 text-[11.5px] font-medium text-accent-contrast transition-colors hover:bg-accent-hover disabled:opacity-50 sm:w-auto"
            >
              {renaming ? t("chat.renaming") : t("chat.renameSave")}
            </button>
          </div>
        </form>
      </Modal>

      <ConfirmDialog
        open={deleteTarget !== null}
        title={t("chat.deleteTitle", {
          title: deleteTarget?.title || t("chat.untitled"),
        })}
        body={t("chat.deleteBody")}
        confirmLabel={t("chat.deleteConfirm")}
        tone="danger"
        busy={deleting}
        onConfirm={() => void confirmDelete()}
        onClose={() => setDeleteTarget(null)}
      />

      <ConfirmDialog
        open={regenerateOpen}
        title={t("chat.regenerate.title")}
        body={t("chat.regenerate.body")}
        confirmLabel={t("chat.regenerate.confirm")}
        busy={regenerateBusy}
        onConfirm={() => void confirmRegenerate()}
        onClose={() => setRegenerateOpen(false)}
      />

      <ConfirmDialog
        open={editTarget !== null}
        title={t("chat.edit.title")}
        body={t("chat.edit.warning")}
        confirmLabel={editBusy ? t("chat.edit.busy") : t("chat.edit.confirm")}
        busy={editBusy}
        confirmDisabled={!editValue.trim()}
        onConfirm={() => void confirmEdit()}
        onClose={() => {
          if (!editBusy) {
            setEditTarget(null);
          }
        }}
      >
        <label
          className="mt-3 mb-1 block text-[11px] font-medium text-secondary"
          htmlFor="chat-edit"
        >
          {t("chat.edit.field")}
        </label>
        <textarea
          id="chat-edit"
          autoFocus
          rows={6}
          value={editValue}
          onChange={(event) => setEditValue(event.target.value)}
          className="w-full resize-y rounded-[7px] border border-line bg-canvas px-2 py-1.5 text-[13px] leading-[1.5] text-primary outline-none focus:border-line-strong"
        />
        {!editValue.trim() ? (
          <p className="mt-1 text-[11px] text-danger">{t("chat.edit.empty")}</p>
        ) : null}
      </ConfirmDialog>
    </>
  );
}

function ErrorBanner({
  error,
  action,
  onDismiss,
}: {
  error: FoundryCommandError;
  action: BannerAction | null;
  onDismiss: () => void;
}) {
  const { t } = useTranslation();

  return (
    <div className="shrink-0 border-b border-line bg-subtle px-4 py-2">
      <div className="mx-auto flex max-w-[760px] items-center gap-2 text-[11.5px] text-danger">
        <AlertIcon size={14} className="shrink-0" />
        <span className="min-w-0 flex-1 truncate">{errorLabel(error, t)}</span>
        {action ? (
          <button
            type="button"
            onClick={action.onClick}
            disabled={action.busy}
            className="inline-flex h-6 shrink-0 items-center gap-1.5 rounded-[6px] border border-line px-2 text-[11px] text-secondary transition-colors hover:border-line-strong hover:text-primary disabled:opacity-60"
          >
            {action.busy ? <Spinner size={10} /> : null}
            {action.label}
          </button>
        ) : null}
        <IconButton label={t("common.close")} onClick={onDismiss} className="h-6 w-6">
          <CloseIcon size={13} />
        </IconButton>
      </div>
    </div>
  );
}

function mergeVisibleConversation(current: Conversation[] | null, updated: Conversation): Conversation[] | null {
  return current?.some((entry) => entry.id === updated.id) ? mergeConversation(current, updated) : current;
}

function isChatModel(model: LoadedModel): boolean {
  const type = model.modelType?.toLowerCase();

  return type !== undefined && chatModelTypes.has(type);
}

function matchesModelAlias(model: LoadedModel, alias: string): boolean {
  return model.alias === alias || model.id === alias;
}

function conversationLoadTarget(conversation: Conversation): string {
  return conversation.modelVariantId ?? conversation.modelAlias;
}

function variantLabel(variant: ModelVariant): string {
  const device = variant.device?.toUpperCase() ?? null;
  const provider = variant.executionProvider?.replace(/ExecutionProvider$/, "") ?? null;
  const tags = [device, provider !== device ? provider : null, variant.cached ? "cached" : null].filter(
    (value): value is string => Boolean(value),
  );

  return tags.join(" · ") || variant.id;
}

function pickUnloadCandidate(
  models: LoadedModel[],
  selectedAlias: string | null,
): LoadedModel | null {
  if (!models.length) {
    return null;
  }

  const preferred = models
    .filter((model) => !selectedAlias || !matchesModelAlias(model, selectedAlias))
    .sort(compareLoadedModelSize)[0];

  return preferred ?? null;
}

function compareLoadedModelSize(left: LoadedModel, right: LoadedModel): number {
  const leftSize = left.fileSizeMb ?? 0;
  const rightSize = right.fileSizeMb ?? 0;

  if (leftSize !== rightSize) {
    return rightSize - leftSize;
  }

  return left.alias.localeCompare(right.alias);
}
