import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ContextList } from "../components/ContextList";
import { EmptyState, ListEmpty } from "../components/EmptyState";
import { ModelAccessPanel } from "../components/ModelAccessPanel";
import {
  AlertIcon,
  ArchiveIcon,
  ChatIcon,
  ChevronDownIcon,
  CloseIcon,
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
  createConversation,
  deleteConversation,
  editChatMessage,
  exportConversation,
  listConversations,
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
  catalogList,
  catalogModel,
  FoundryCommandError,
  loadModel,
  loadedModels,
  startServer,
  toFoundryCommandError,
  unloadModel,
  type CatalogModel,
  type Declared,
  type LoadedModel,
} from "../lib/foundry";
import { minimumSearchQuery, searchResultLimit } from "../lib/search";
import { dragWindowFromMouseDown } from "../lib/drag";
import { ConfirmDialog } from "./models/ConfirmDialog";
import { errorLabel } from "./models/format";
import { Composer } from "./chat/Composer";
import { ConversationList } from "./chat/ConversationList";
import { ExportMenu } from "./chat/ExportMenu";
import { MessageList } from "./chat/MessageList";
import { ModelMenu, type ChatModelOption } from "./chat/ModelMenu";
import { ParamsPopover } from "./chat/ParamsPopover";
import { SearchPanel, type SearchScope } from "./chat/SearchPanel";
import { TokenUsageBadge } from "./chat/TokenUsageBadge";

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
  const [listFilterOpen, setListFilterOpen] = useState(false);
  const [listQuery, setListQuery] = useState("");
  const activeRef = useRef<ActiveGeneration | null>(null);
  const selectedIdRef = useRef<number | null>(null);
  const finishedRef = useRef(new Set<string>());
  const pendingJump = useRef<{ conversationId: number; messageId: number } | null>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const [jumpNonce, setJumpNonce] = useState(0);
  const selected = conversations?.find((conversation) => conversation.id === selectedId) ?? null;
  const chatModels = (loaded ?? []).filter(isChatModel);
  const cachedChatModels = (catalog ?? []).filter(
    (model) => (model.kind === "chat" || model.kind === "multimodal") && model.cached,
  );
  const loadableChatAliases = cachedChatModels
    .filter((model) => !chatModels.some((loadedModel) => loadedModel.alias === model.alias))
    .map((model) => model.alias);
  const streamingConversationId = active?.conversationId ?? null;
  const draft = selectedId === null ? "" : (drafts[selectedId] ?? "");
  const attachments = selectedId === null ? [] : (staged[selectedId] ?? []);
  const selectedModelAlias = selected?.modelAlias ?? null;
  const unloadCandidate = useMemo(
    () => pickUnloadCandidate(loaded ?? [], selectedModelAlias),
    [loaded, selectedModelAlias],
  );
  const notDeclared = inputModalities === null || inputModalities.origin === "notDeclared";
  const attachBlocked =
    inputModalities !== null &&
    inputModalities.origin !== "notDeclared" &&
    !(inputModalities.value ?? []).includes("image");
  const attachEnabled = !attachBlocked && active === null && !sending;
  const attachHint = attachBlocked
    ? t("chat.attachDisabled")
    : notDeclared
      ? t("chat.attachNotDeclared")
      : t("chat.attach");

  useEffect(() => {
    activeRef.current = active;
  }, [active]);

  useEffect(() => {
    selectedIdRef.current = selectedId;
  }, [selectedId]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "f") {
        if (document.querySelector('[role="dialog"]')) {
          return;
        }

        event.preventDefault();
        setSearchOpen(true);
        window.setTimeout(() => searchInputRef.current?.focus(), 0);
      }
    }

    window.addEventListener("keydown", onKeyDown, true);

    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, []);

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
    const [loadedResult, catalogResult] = await Promise.allSettled([loadedModels(), catalogList()]);

    if (loadedResult.status === "fulfilled") {
      setLoaded(loadedResult.value.models);
      setLoadedError(null);
    } else {
      setLoaded([]);
      setLoadedError(toFoundryCommandError(loadedResult.reason));
    }

    if (catalogResult.status === "fulfilled") {
      setCatalog(catalogResult.value);
    } else {
      setCatalog([]);
    }
  }, []);

  const isModelLoaded = useCallback(
    (alias: string) => (loaded ?? []).some((model) => matchesModelAlias(model, alias)),
    [loaded],
  );

  useEffect(() => {
    let live = true;

    listConversations()
      .then((next) => {
        if (!live) {
          return;
        }

        setConversations(next);
        setSelectedId((current) => current ?? next[0]?.id ?? null);
      })
      .catch((error) => {
        if (live) {
          setConversations([]);
          setBanner(toFoundryCommandError(error));
        }
      });

    void refreshModels();

    return () => {
      live = false;
    };
  }, [refreshModels]);

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
        current ? mergeConversation(current, event.conversation) : current,
      );
      setMessages((current) => mergeMessages(current, [event.userMessage, event.assistantMessage]));

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
      current ? mergeConversation(current, event.conversation) : current,
    );

    const conversationId = event.conversation.id;

    conversationUsage(conversationId)
      .then((next) => {
        if (selectedIdRef.current === conversationId) {
          setUsage(next);
        }
      })
      .catch(() => undefined);

    if (event.type === "done") {
      setMessages((current) => mergeMessages(current, [event.message]));
    } else {
      setMessages((current) => mergeMessages(current, [event.message]));
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

  async function refreshConversations() {
    try {
      const next = await listConversations(archivedView);

      setConversations(next);
      setSelectedId((current) =>
        current !== null && next.some((conversation) => conversation.id === current)
          ? current
          : (next[0]?.id ?? null),
      );
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    }
  }

  async function switchArchivedView() {
    const next = !archivedView;

    setArchivedView(next);
    setConversations(null);
    setSelectedId(null);
    setListQuery("");
    setDeleteTarget(null);
    pendingJump.current = null;
    setHighlightId(null);

    try {
      const list = await listConversations(next);

      setConversations(list);
      setSelectedId(list[0]?.id ?? null);
    } catch (error) {
      setConversations([]);
      setBanner(toFoundryCommandError(error));
    }
  }

  async function toggleFavorite(conversation: Conversation) {
    if (active) {
      return;
    }

    try {
      const updated = await setConversationFavorite(conversation.id, !conversation.favorite);

      setConversations((current) => (current ? mergeConversation(current, updated) : current));
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    }
  }

  async function toggleArchived(conversation: Conversation) {
    if (active) {
      return;
    }

    try {
      const updated = await setConversationArchived(conversation.id, !conversation.archived);
      const remaining = (conversations ?? []).filter((entry) => entry.id !== updated.id);

      setConversations(remaining);

      if (selectedId === updated.id) {
        pendingJump.current = null;
        setHighlightId(null);
        setSelectedId(remaining[0]?.id ?? null);
      }
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    }
  }

  useEffect(() => {
    if (openConversationId === null) {
      return;
    }

    if (conversations?.some((conversation) => conversation.id === openConversationId)) {
      pendingJump.current = null;
      setHighlightId(null);
      setSelectedId(openConversationId);
      onOpenConversationHandled?.();

      return;
    }

    let live = true;

    refreshConversations()
      .then(() => {
        if (live) {
          setSelectedId(openConversationId);
          onOpenConversationHandled?.();
        }
      })
      .catch(() => {
        if (live) {
          onOpenConversationHandled?.();
        }
      });

    return () => {
      live = false;
    };
  }, [openConversationId, conversations, onOpenConversationHandled]);

  async function startConversation() {
    const modelAlias = chatModels[0]?.alias;

    if (!modelAlias || active) {
      return;
    }

    try {
      const conversation = await createConversation(modelAlias);

      if (archivedView) {
        setArchivedView(false);

        try {
          setConversations(await listConversations(false));
        } catch {
          setConversations([conversation]);
        }
      } else {
        setConversations((current) => mergeConversation(current ?? [], conversation));
      }

      setSelectedId(conversation.id);
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
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

  async function ensureConversationModelLoaded(conversation: Conversation) {
    if (isModelLoaded(conversation.modelAlias)) {
      return;
    }

    await runModelLoad(conversation.modelAlias, true);
  }

  async function recoverServerConnection(refreshList: boolean): Promise<boolean> {
    if (reconnecting) {
      return false;
    }

    setReconnecting(true);

    try {
      await startServer();
      await refreshModels();

      if (refreshList) {
        await refreshConversations();
      }

      setBanner(null);
      return true;
    } catch (error) {
      setBanner(toFoundryCommandError(error));
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
        await runModelLoad(conversation.modelAlias, true);
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

    if (!selected || (!content && tokens.length === 0) || active || sending || loadingModelAlias !== null) {
      return;
    }

    const conversationId = selected.id;

    setSending(true);
    setBanner(null);
    pendingJump.current = null;

    try {
      await ensureConversationModelLoaded(selected);

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
      current ? mergeConversation(current, receipt.conversation) : current,
    );
    setMessages((current) =>
      mergeMessages(
        current.filter((message) => !removed.has(message.id)),
        [receipt.userMessage, receipt.assistantMessage],
      ),
    );

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
    pendingJump.current = null;
    setHighlightId(null);
    setSelectedId(id);
  }

  function selectSearchHit(hit: SearchHit) {
    if (hit.conversationId !== selectedIdRef.current) {
      setSelectedId(hit.conversationId);
    }

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

    try {
      const change = await setConversationModel(selected.id, modelAlias);

      setConversations((current) =>
        current ? mergeConversation(current, change.conversation) : current,
      );

      const divider = change.divider;

      if (divider) {
        setMessages((current) => mergeMessages(current, [divider]));
      }
    } catch (error) {
      setBanner(toFoundryCommandError(error));
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

    try {
      const conversation = await renameConversation(selected.id, title);

      setConversations((current) =>
        current ? mergeConversation(current, conversation) : current,
      );
      setRenameOpen(false);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
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

    try {
      await deleteConversation(target.id);

      const remaining = (conversations ?? []).filter(
        (conversation) => conversation.id !== target.id,
      );

      setConversations(remaining);

      if (selectedId === target.id) {
        pendingJump.current = null;
        setHighlightId(null);
        setMessages([]);
        setSelectedId(remaining[0]?.id ?? null);
      }

      setDrafts((current) => {
        const next = { ...current };

        delete next[target.id];

        return next;
      });
      clearAttachments(target.id);
      setDeleteTarget(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
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
      return {
        label: t("models.actions.load"),
        busy: loadingModelAlias === selected.modelAlias,
        onClick: () => void loadChatModelAlias(selected.modelAlias),
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

  const modelOptions: ChatModelOption[] = chatModels.map((model) => ({
    value: model.alias,
    label: model.alias,
    hint: model.displayName ?? model.id,
  }));

  if (selected && !modelOptions.some((option) => option.value === selected.modelAlias)) {
    modelOptions.unshift({
      value: selected.modelAlias,
      label: selected.modelAlias,
      hint: t("chat.notLoaded"),
    });
  }

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
  const missingLoadedChatModels = catalog !== null && chatModels.length === 0 && cachedChatModels.length > 0;
  const actionsDisabled = active !== null || sending;
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
  }, [filteredConversations, selectedId]);

  return (
    <>
      <ContextList
        title={archivedView ? t("chat.archivedTitle") : t("chat.listTitle")}
        action={
          <div className="flex items-center gap-0.5">
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
              disabled={!chatModels.length || active !== null}
            >
              <PlusIcon size={15} />
            </IconButton>
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
          ) : null
        }
      >
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
      </ContextList>

      {selected ? (
        <section className="flex h-full min-h-0 flex-col bg-canvas">
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
                    <ExportMenu
                      disabled={active !== null}
                      busy={exporting}
                      onExport={(format) => void runExport(format)}
                    />
                    <ParamsPopover
                      conversationId={selected.id}
                      modelAlias={selected.modelAlias}
                      disabled={active !== null}
                    />
                    <ModelMenu
                      label={t("chat.model")}
                      value={selected.modelAlias}
                      options={modelOptions}
                      disabled={active !== null}
                      onChange={(modelAlias) => void changeModel(modelAlias)}
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

          {banner ? (
            <ErrorBanner
              error={banner}
              action={bannerAction}
              onDismiss={() => setBanner(null)}
            />
          ) : null}

          {!loadedError && (missingDownloadedChatModels || missingLoadedChatModels) ? (
            <div className="shrink-0 border-b border-line bg-canvas px-4 py-3 sm:px-5">
              <div className="mx-auto max-w-[760px]">
                <ModelAccessPanel
                  title={
                    missingDownloadedChatModels
                      ? t("common.noDownloadedModelTitle", { kind: t("models.kinds.chat") })
                      : t("common.noLoadedModelTitle", { kind: t("models.kinds.chat") })
                  }
                  body={
                    missingDownloadedChatModels
                      ? t("common.noDownloadedModelBody", { kind: t("models.kinds.chat") })
                      : t("common.noLoadedModelBody", { kind: t("models.kinds.chat") })
                  }
                  listTitle={t("common.downloadedModels")}
                  models={loadableChatAliases}
                  loadLabel={t("models.actions.load")}
                  loadingLabel={t("common.loading")}
                  openModelsLabel={t("common.openModels")}
                  loadingAlias={loadingModelAlias}
                  onLoad={(alias) => void loadChatModelAlias(alias)}
                  onOpenModels={() => onOpenModels?.()}
                />
              </div>
            </div>
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
              key={selected.id}
              messages={messages}
              hasMore={hasMore}
              loadingOlder={loadingOlder}
              highlightId={highlightId}
              actionsDisabled={actionsDisabled}
              onLoadOlder={() => void loadOlder()}
              onEdit={startEdit}
              onRegenerate={() => setRegenerateOpen(true)}
            />
          )}

          <Composer
            value={draft}
            sending={sending}
            active={active !== null}
            stopping={stopping}
            modelLabel={selected.modelAlias}
            attachments={attachments}
            attachEnabled={attachEnabled}
            attachHint={attachHint}
            onChange={updateDraft}
            onSend={() => void send()}
            onStop={() => void stop()}
            onAttach={(files) => void attachFiles(files)}
            onRemoveAttachment={removeAttachment}
          />
        </section>
      ) : (
        <section className="flex h-full min-h-0 flex-col bg-canvas">
          <header
            className="flex h-[var(--titlebar-inset)] shrink-0 items-center border-b border-line px-5"
            data-tauri-drag-region
            onMouseDown={dragWindowFromMouseDown}
          >
            <h1 className="text-[15px] font-semibold tracking-[-0.01em] text-primary">
              {t("chat.title")}
            </h1>
          </header>
          {banner ? (
            <ErrorBanner
              error={banner}
              action={bannerAction}
              onDismiss={() => setBanner(null)}
            />
          ) : null}
          {!loadedError && (missingDownloadedChatModels || missingLoadedChatModels) ? (
            <div className="mx-auto w-full max-w-[760px] px-4 py-5 sm:px-6 sm:py-6">
              <ModelAccessPanel
                title={
                  missingDownloadedChatModels
                    ? t("common.noDownloadedModelTitle", { kind: t("models.kinds.chat") })
                    : t("common.noLoadedModelTitle", { kind: t("models.kinds.chat") })
                }
                body={
                  missingDownloadedChatModels
                    ? t("common.noDownloadedModelBody", { kind: t("models.kinds.chat") })
                    : t("common.noLoadedModelBody", { kind: t("models.kinds.chat") })
                }
                listTitle={t("common.downloadedModels")}
                models={loadableChatAliases}
                loadLabel={t("models.actions.load")}
                loadingLabel={t("common.loading")}
                openModelsLabel={t("common.openModels")}
                loadingAlias={loadingModelAlias}
                onLoad={(alias) => void loadChatModelAlias(alias)}
                onOpenModels={() => onOpenModels?.()}
              />
            </div>
          ) : (
            <EmptyState icon={ChatIcon} title={emptyTitle} body={emptyBody} />
          )}
        </section>
      )}

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

function isChatModel(model: LoadedModel): boolean {
  const type = model.modelType?.toLowerCase();

  return type !== undefined && chatModelTypes.has(type);
}

function matchesModelAlias(model: LoadedModel, alias: string): boolean {
  return model.alias === alias || model.id === alias;
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
