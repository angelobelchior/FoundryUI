import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ContextList } from "../components/ContextList";
import { DetailPane } from "../components/DetailPane";
import { EmptyState, ListEmpty } from "../components/EmptyState";
import { ModelAccessPanel } from "../components/ModelAccessPanel";
import {
  AlertIcon,
  ArchiveIcon,
  CloseIcon,
  CopyIcon,
  DownloadIcon,
  EmbeddingsIcon,
  PaperclipIcon,
  PlayIcon,
  PlusIcon,
  SearchIcon,
  StarIcon,
  StopIcon,
  TrashIcon,
} from "../components/Icon";
import { ContextMenu, type ContextMenuItem } from "../components/ContextMenu";
import { IconButton } from "../components/IconButton";
import { Spinner } from "../components/Spinner";
import { useToast } from "../components/Toast";
import {
  answerCorpus,
  cancelVectorize,
  createCorpus,
  deleteCorpus,
  getCorpus,
  importCorpusLines,
  listCorpora,
  pickCorpusFiles,
  revealCorpusSource,
  renameCorpus,
  replaceCorpusDocuments,
  searchCorpus,
  setCorpusArchived,
  setCorpusFavorite,
  setCorpusModel,
  vectorizeCorpus,
  type EmbeddingCorpus,
  type CorpusDocument,
  type EmbeddingAnswerResult,
  type EmbeddingSearchHit,
  type EmbeddingSearchResult,
  type EmbeddingUsage,
  type VectorizeReceipt,
  type VectorizeEvent,
} from "../lib/embeddings";
import { copyText } from "../lib/copy";
import {
  downloadModel,
  loadModel,
  toFoundryCommandError,
  workspaceSnapshot,
  type CatalogModel,
  type FoundryCommandError,
  type JobStatus,
  type ModelJob,
} from "../lib/foundry";
import { useModelJobStream } from "../lib/useModelJobStream";
import { ConfirmDialog } from "./models/ConfirmDialog";
import { errorLabel } from "./models/format";

type FlowStep = 1 | 2 | 3 | 4;
type FlowMode = "none" | "create" | "append";
type ProcessingStage = "idle" | "extracting" | "saving" | "vectorizing";

type MenuState = {
  corpus: EmbeddingCorpus;
  x: number;
  y: number;
};

type EmbeddingsWorkspaceProps = {
  onOpenModels?: () => void;
  onLeaveGuardChange?: (guard: { message: string; onConfirmLeave?: () => Promise<void> | void } | null) => void;
};

export function EmbeddingsWorkspace({ onOpenModels, onLeaveGuardChange }: EmbeddingsWorkspaceProps) {
  const { t } = useTranslation();
  const { push } = useToast();
  const [corpora, setCorpora] = useState<EmbeddingCorpus[] | null>(null);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [selected, setSelected] = useState<EmbeddingCorpus | null>(null);
  const [documents, setDocuments] = useState<CorpusDocument[]>([]);
  const [models, setModels] = useState<CatalogModel[]>([]);
  const [loadedAliases, setLoadedAliases] = useState<string[]>([]);
  const [banner, setBanner] = useState<FoundryCommandError | null>(null);
  const [nameDraft, setNameDraft] = useState("");
  const [renameBusy, setRenameBusy] = useState(false);
  const [flowMode, setFlowMode] = useState<FlowMode>("none");
  const [flowCorpusId, setFlowCorpusId] = useState<number | null>(null);
  const [activeFlowStep, setActiveFlowStep] = useState<FlowStep>(4);
  const [flowTransition, setFlowTransition] = useState<"forward" | "backward">("forward");
  const [stagedFiles, setStagedFiles] = useState<string[]>([]);
  const [processingStage, setProcessingStage] = useState<ProcessingStage>("idle");
  const [extractProgress, setExtractProgress] = useState({ completed: 0, total: 0 });
  const [replaceBusy, setReplaceBusy] = useState(false);
  const [creatingCorpus, setCreatingCorpus] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<EmbeddingCorpus | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [archivedView, setArchivedView] = useState(false);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [vectorizing, setVectorizing] = useState(false);
  const [vectorizeRequestId, setVectorizeRequestId] = useState<string | null>(null);
  const [vectorizeProgress, setVectorizeProgress] = useState({ completed: 0, total: 0 });
  const [vectorizeUsage, setVectorizeUsage] = useState<EmbeddingUsage>({ promptTokens: 0, totalTokens: 0 });
  const [searchBusy, setSearchBusy] = useState(false);
  const [answerBusy, setAnswerBusy] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [topK, setTopK] = useState("5");
  const [searchExistingSourcesOnly, setSearchExistingSourcesOnly] = useState(false);
  const [searchResult, setSearchResult] = useState<EmbeddingSearchResult | null>(null);
  const [answerResult, setAnswerResult] = useState<EmbeddingAnswerResult | null>(null);
  const [selectedHit, setSelectedHit] = useState<EmbeddingSearchHit | null>(null);
  const [showFullHitText, setShowFullHitText] = useState(false);
  const [pendingModelAlias, setPendingModelAlias] = useState<string | null>(null);
  const [listFilterOpen, setListFilterOpen] = useState(false);
  const [listQuery, setListQuery] = useState("");
  const [loadingModelAlias, setLoadingModelAlias] = useState<string | null>(null);
  const [jobs, setJobs] = useState<Record<string, ModelJob>>({});
  const [openingSourcePath, setOpeningSourcePath] = useState<string | null>(null);
  const jobStatusRef = useRef<Record<string, JobStatus>>({});
  const isCreateFlow = flowMode === "create";
  const isAppendFlow = flowMode === "append";
  const canGoStep2 = Boolean(selected?.modelAlias.trim());
  const canGoStep3 = canGoStep2 && (stagedFiles.length > 0 || (selected?.documentCount ?? 0) > 0);
  const displayedVectorProgress = selected
    ? {
        completed: vectorizing ? vectorizeProgress.completed : selected.vectorCount,
        total: vectorizing ? vectorizeProgress.total : selected.documentCount,
      }
    : { completed: 0, total: 0 };
  const hasVectorizedDocuments = (selected?.vectorCount ?? 0) > 0;
  const vectorizeActionLabel = vectorizing
    ? t("embeddings.actions.cancelVectorize")
    : hasVectorizedDocuments
      ? t("embeddings.actions.revectorize")
      : t("embeddings.actions.vectorize");
  const searchReady = selected !== null && selected.documentCount > 0 && selected.vectorCount === selected.documentCount && !vectorizing && !replaceBusy;
  const canGoStep4 = searchReady && stagedFiles.length === 0;
  const furthestStep: FlowStep = !canGoStep2 ? 1 : !canGoStep3 ? 2 : !canGoStep4 ? 3 : 4;
  const showingStep4 = (isCreateFlow && activeFlowStep === 4) || (!isCreateFlow && !isAppendFlow);
  const modelBusy = loadingModelAlias !== null || (selected ? jobs[selected.modelAlias]?.status === "running" : false);
  const searchInteractionBusy = searchBusy || answerBusy;
  const topStatusMessage = processingStage === "extracting"
    ? t("embeddings.flow.extracting")
    : processingStage === "saving"
      ? t("embeddings.flow.saving")
      : vectorizing
    ? t("embeddings.flow.topVectorizing")
    : selected?.documentCount === 0
      ? t("embeddings.flow.topLoadFiles")
      : !searchReady
        ? t("embeddings.flow.topStartVectorization")
        : searchReady
          ? t("embeddings.flow.topReady")
          : hasVectorizedDocuments
            ? t("embeddings.flow.topPartialVectors", {
                done: selected?.vectorCount ?? 0,
                total: selected?.documentCount ?? 0,
              })
            : t("embeddings.flow.topStartVectorization");
  const progressNumbers = processingStage === "extracting"
    ? extractProgress
    : {
        completed: displayedVectorProgress.completed,
        total: displayedVectorProgress.total,
      };
  const progressLabel = processingStage === "extracting"
    ? t("embeddings.vectorize.extractProgress", {
        completed: progressNumbers.completed,
        total: progressNumbers.total,
      })
    : t("embeddings.vectorize.progress", {
        completed: progressNumbers.completed,
        total: progressNumbers.total,
      });
  const filteredSearchHits = useMemo(() => {
    const hits = searchResult?.hits ?? [];

    if (!searchExistingSourcesOnly) {
      return hits;
    }

    return hits.filter((hit) => Boolean(hit.sourcePath) && hit.sourceExists);
  }, [searchExistingSourcesOnly, searchResult]);
  const selectedHitContent = useMemo(() => {
    if (!selectedHit) {
      return "";
    }

    if (showFullHitText) {
      return selectedHit.text;
    }

    return extractSearchContext(selectedHit.text, searchQuery);
  }, [searchQuery, selectedHit, showFullHitText]);
  const selectedHitHasExcerpt = selectedHit !== null && selectedHitContent !== selectedHit.text;

  const embeddingModels = useMemo(
    () => models.filter((model) => model.kind === "embedding"),
    [models],
  );
  const cachedEmbeddingModels = useMemo(
    () => embeddingModels.filter((model) => model.cached),
    [embeddingModels],
  );
  const loadedEmbeddingAliasSet = useMemo(() => new Set(loadedAliases), [loadedAliases]);
  const loadedEmbeddingModels = useMemo(
    () => cachedEmbeddingModels.filter((model) => loadedEmbeddingAliasSet.has(model.alias)),
    [cachedEmbeddingModels, loadedEmbeddingAliasSet],
  );
  const loadableEmbeddingAliases = useMemo(
    () => cachedEmbeddingModels.filter((model) => !loadedEmbeddingAliasSet.has(model.alias)).map((model) => model.alias),
    [cachedEmbeddingModels, loadedEmbeddingAliasSet],
  );
  const selectedModel = useMemo(
    () => embeddingModels.find((model) => model.alias === selected?.modelAlias),
    [embeddingModels, selected?.modelAlias],
  );
  const selectedModelIsLoaded = selected ? loadedEmbeddingAliasSet.has(selected.modelAlias) : false;
  const selectedModelJob = selected ? jobs[selected.modelAlias] : undefined;
  const selectedModelNeedsDownload = Boolean(selected && selectedModel && !selectedModel.cached);
  const selectedModelUnavailable = Boolean(selected && !selectedModel);
  const selectedModelNeedsLoad = Boolean(selected && selectedModel?.cached && !selectedModelIsLoaded);
  const sourceFileCount = useMemo(() => uniqueDocumentSources(documents).length, [documents]);
  const filteredCorpora = useMemo(() => {
    const query = listQuery.trim().toLocaleLowerCase();

    if (!query) {
      return corpora ?? [];
    }

    return (corpora ?? []).filter((corpus) => {
      const name = corpus.name.toLocaleLowerCase();
      const model = corpus.modelAlias.toLocaleLowerCase();

      return name.includes(query) || model.includes(query);
    });
  }, [corpora, listQuery]);

  const refreshCorpora = useCallback(async (archived = false) => {
    const items = await listCorpora(archived);

    setCorpora(items);
    setSelectedId((current) =>
      current !== null && items.some((entry) => entry.id === current) ? current : (items[0]?.id ?? null),
    );
  }, []);

  const refreshModels = useCallback(async () => {
    const snapshot = await workspaceSnapshot();

    setModels(snapshot.catalog);
    setLoadedAliases(snapshot.loaded.models.map((model) => model.alias));
  }, []);

  const goToFlowStep = useCallback(
    (next: FlowStep) => {
      if (!isCreateFlow) {
        return;
      }

      setFlowTransition(next >= activeFlowStep ? "forward" : "backward");
      setActiveFlowStep(next);
    },
    [activeFlowStep, isCreateFlow],
  );

  useEffect(() => {
    Promise.all([refreshCorpora(), refreshModels()]).catch((error) => {
      setBanner(toFoundryCommandError(error));
    });
  }, [refreshCorpora, refreshModels]);

  useEffect(() => {
    if (selectedId === null) {
      setSelected(null);
      setDocuments([]);
      setNameDraft("");
      setSearchResult(null);
      setAnswerResult(null);
      setSelectedHit(null);
      setFlowMode("none");
      setFlowCorpusId(null);
      setActiveFlowStep(4);
      setStagedFiles([]);

      return;
    }

    let live = true;
    setSelected(null);
    setSearchResult(null);
    setAnswerResult(null);
    setSelectedHit(null);
    getCorpus(selectedId)
      .then((details) => {
        if (!live) return;
        const inCreateFlow = flowMode === "create" && flowCorpusId === details.corpus.id;

        setSelected(details.corpus);
        setDocuments(details.documents);
        setNameDraft(details.corpus.name);

        if (inCreateFlow) {
          if (details.corpus.documentCount === 0) {
            setActiveFlowStep(1);
          }
        } else {
          setFlowMode("none");
          setFlowCorpusId(null);
          setActiveFlowStep(4);
          setStagedFiles([]);
        }

      })
      .catch((error) => {
        if (live) setBanner(toFoundryCommandError(error));
      });
    return () => { live = false; };
  }, [flowCorpusId, flowMode, selectedId]);

  useEffect(() => {
    if (!isCreateFlow) {
      return;
    }

    if (activeFlowStep > furthestStep) {
      setFlowTransition("backward");
      setActiveFlowStep(furthestStep);
    }
  }, [activeFlowStep, furthestStep, isCreateFlow]);

  useEffect(() => {
    setSelectedHit((current) => {
      if (filteredSearchHits.length === 0) {
        return null;
      }

      if (current && filteredSearchHits.some((hit) => hit.documentId === current.documentId)) {
        return current;
      }

      return filteredSearchHits[0] ?? null;
    });
  }, [filteredSearchHits]);

  useEffect(() => {
    setShowFullHitText(false);
  }, [searchQuery, selectedHit?.documentId]);

  useEffect(() => {
    setAnswerResult(null);
  }, [searchQuery, topK]);

  useEffect(() => {
    if (filteredCorpora.length === 0) {
      setSelectedId(null);
      return;
    }

    const first = filteredCorpora[0];

    if (!first) {
      return;
    }

    if (selectedId === null || !filteredCorpora.some((entry) => entry.id === selectedId)) {
      setSelectedId(first.id);
    }
  }, [filteredCorpora, selectedId]);

  useEffect(() => {
    if (!onLeaveGuardChange) {
      return;
    }

    const pendingDraft = flowMode === "create" && activeFlowStep < 4;

    if (!pendingDraft) {
      onLeaveGuardChange(null);
      return;
    }

    const draftCorpusId = flowCorpusId ?? selected?.id ?? null;

    onLeaveGuardChange({
      message: t("embeddings.flow.leaveConfirm"),
      onConfirmLeave: async () => {
        if (draftCorpusId === null) {
          return;
        }

        try {
          await deleteCorpus(draftCorpusId);
        } catch (error) {
          setBanner(toFoundryCommandError(error));
          throw error;
        }
      },
    });

    return () => {
      onLeaveGuardChange(null);
    };
  }, [activeFlowStep, flowCorpusId, flowMode, onLeaveGuardChange, selected?.id, t]);

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
      void refreshModels();
      push(t("models.actions.downloaded", { alias: job.alias }), "neutral");

      return;
    }

    if (job.status === "failed" && job.error) {
      push(errorLabel(toFoundryCommandError(job.error), t), "danger");
    }
  });

  async function startNewCorpusFlow() {
    if (creatingCorpus) {
      return;
    }

    const model = cachedEmbeddingModels[0];

    if (!model) {
      push(t("embeddings.create.noModels"), "danger");
      return;
    }

    setCreatingCorpus(true);

    try {
      const corpus = await createCorpus({
        name: suggestCorpusName(t("embeddings.actions.newCorpus"), corpora ?? []),
        modelAlias: model.alias,
        dimensions: model?.embeddingDimensions.value ?? 0,
      });

      setBanner(null);

      if (archivedView) {
        setArchivedView(false);
      }

      await refreshCorpora(false);
      setFlowCorpusId(corpus.id);
      setFlowMode("create");
      setFlowTransition("forward");
      setActiveFlowStep(1);
      setStagedFiles([]);
      setSelectedId(corpus.id);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setCreatingCorpus(false);
    }
  }

  async function saveCorpusName() {
    if (!selected || !nameDraft.trim() || renameBusy || nameDraft.trim() === selected.name) {
      return;
    }

    setRenameBusy(true);

    try {
      const updated = await renameCorpus(selected.id, nameDraft.trim());

      setSelected(updated);
      setCorpora((current) => mergeCorpora(current ?? [], updated));
      setBanner(null);
      push(t("embeddings.corpus.renamed"), "neutral");
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setRenameBusy(false);
    }
  }

  async function addStagedFiles() {
    try {
      const paths = await pickCorpusFiles();

      if (!paths.length) {
        return;
      }

      setStagedFiles((current) => {
        const next = new Set(current);
        const previousSize = next.size;

        for (const path of paths) {
          next.add(path);
        }

        const added = next.size - previousSize;

        if (added > 0) {
          push(t("embeddings.documents.filesAdded", { count: added }), "neutral");
        }

        return Array.from(next);
      });
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    }
  }

  function removeStagedFile(path: string) {
    if (replaceBusy || vectorizing) {
      return;
    }

    setStagedFiles((current) => current.filter((entry) => entry !== path));
  }

  async function applyModelChange(alias: string) {
    if (!selected) {
      return;
    }

    const model = embeddingModels.find((entry) => entry.alias === alias);

    try {
      const updated = await setCorpusModel({
        corpusId: selected.id,
        modelAlias: alias,
        dimensions: model?.embeddingDimensions.value ?? 0,
      });

      setSelected(updated);
      setCorpora((current) => mergeCorpora(current ?? [], updated));
      setSearchResult(null);
      setAnswerResult(null);
      setSelectedHit(null);
      setBanner(null);
      push(t("embeddings.model.changed"), "neutral");
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    }
  }

  async function changeModel(alias: string) {
    if (!selected) {
      return;
    }

    if (selected.modelAlias === alias) {
      return;
    }

    if (selected.vectorCount > 0) {
      setPendingModelAlias(alias);

      return;
    }

    await applyModelChange(alias);
  }

  async function runVectorization(corpusId: number, totalDocuments: number): Promise<VectorizeReceipt> {
    setVectorizing(true);
    setVectorizeProgress({ completed: 0, total: totalDocuments });
    setVectorizeUsage({ promptTokens: 0, totalTokens: 0 });

    try {
      const receipt = await vectorizeCorpus(corpusId, (event) => {
        handleVectorizeEvent(event);
      });

      setVectorizeRequestId(null);
      setVectorizeProgress({ completed: receipt.completed, total: receipt.totalDocuments });
      setVectorizeUsage(receipt.usage);
      setSelected(receipt.corpus);
      setCorpora((current) => mergeCorpora(current ?? [], receipt.corpus));
      setBanner(null);
      push(
        receipt.cancelled ? t("embeddings.vectorize.cancelled") : t("embeddings.vectorize.done"),
        "neutral",
      );

      await refreshModels();

      return receipt;
    } catch (error) {
      setBanner(toFoundryCommandError(error));
      throw error;
    } finally {
      setVectorizing(false);
      setVectorizeRequestId(null);
    }
  }

  async function runFlowVectorization(mode: "create" | "append") {
    if (!selected || replaceBusy || vectorizing || searchInteractionBusy || modelBusy) {
      return;
    }

    const files = stagedFiles;

    if (files.length === 0) {
      if (!selected.documentCount) {
        return;
      }

      setProcessingStage("vectorizing");

      try {
        await runVectorization(selected.id, selected.documentCount);
      } catch {
        return;
      } finally {
        setProcessingStage("idle");
      }

      return;
    }

    setReplaceBusy(true);
    setProcessingStage("extracting");
    setExtractProgress({ completed: 0, total: files.length });

    try {
      const extracted: Array<{ path: string; lines: string[] }> = [];

      for (let index = 0; index < files.length; index += 1) {
        const path = files[index];

        if (!path) {
          continue;
        }

        const lines = await importCorpusLines(path);

        extracted.push({ path, lines });
        setExtractProgress({ completed: index + 1, total: files.length });
      }

      const importedLines = extracted.flatMap((entry) => entry.lines);
      const importedSources = extracted.flatMap((entry) => entry.lines.map(() => entry.path));

      if (!importedLines.length) {
        push(t("embeddings.documents.importedEmpty"), "neutral");
        setProcessingStage("idle");
        return;
      }

      const baseLines = documents.map((document) => document.text);
      const baseSources = documents.map((document) => document.sourcePath ?? null);
      const nextLines = [...baseLines, ...importedLines];
      const nextSources = [...baseSources, ...importedSources];

      setProcessingStage("saving");

      const updated = await replaceCorpusDocuments(selected.id, nextLines, nextSources);

      setSelected(updated);
      setCorpora((current) => mergeCorpora(current ?? [], updated));

      const details = await getCorpus(selected.id);

      setDocuments(details.documents);
      setSearchResult(null);
      setAnswerResult(null);
      setSelectedHit(null);

      setProcessingStage("vectorizing");

      const receipt = await runVectorization(updated.id, updated.documentCount);

      if (!receipt.cancelled && receipt.completed === receipt.totalDocuments && receipt.totalDocuments > 0) {
        if (mode === "create") {
          setFlowTransition("forward");
        }

        if (mode === "append") {
          setFlowMode("none");
          setActiveFlowStep(4);
        }
      }

      setStagedFiles([]);
      setExtractProgress({ completed: 0, total: 0 });
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setReplaceBusy(false);
      setProcessingStage("idle");
    }
  }

  async function runRevectorizeCurrent() {
    if (!selected || !selected.documentCount || replaceBusy || vectorizing || searchInteractionBusy || modelBusy) {
      return;
    }

    setProcessingStage("vectorizing");

    try {
      await runVectorization(selected.id, selected.documentCount);
    } finally {
      setProcessingStage("idle");
    }
  }

  async function runReindexFromSources() {
    if (!selected || replaceBusy || vectorizing || searchInteractionBusy || modelBusy) {
      return;
    }

    const sourcePaths = uniqueDocumentSources(documents);

    if (sourcePaths.length === 0) {
      push(t("embeddings.reindex.noSources"), "neutral");
      return;
    }

    setReplaceBusy(true);
    setProcessingStage("extracting");
    setExtractProgress({ completed: 0, total: sourcePaths.length });

    try {
      const extracted: Array<{ path: string; lines: string[] }> = [];
      const failedPaths: string[] = [];

      for (let index = 0; index < sourcePaths.length; index += 1) {
        const path = sourcePaths[index];

        if (!path) {
          setExtractProgress({ completed: index + 1, total: sourcePaths.length });
          continue;
        }

        try {
          const lines = await importCorpusLines(path);

          extracted.push({ path, lines });
        } catch {
          failedPaths.push(path);
        } finally {
          setExtractProgress({ completed: index + 1, total: sourcePaths.length });
        }
      }

      const lines = extracted.flatMap((entry) => entry.lines);
      const sources = extracted.flatMap((entry) => entry.lines.map(() => entry.path));
      const unsourcedLines = documents
        .filter((document) => !document.sourcePath)
        .map((document) => document.text);
      const nextLines = [...lines, ...unsourcedLines];
      const nextSources = [...sources, ...unsourcedLines.map(() => null)];

      if (!lines.length) {
        push(
          failedPaths.length > 0
            ? t("embeddings.reindex.allFailed", { count: failedPaths.length })
            : t("embeddings.documents.importedEmpty"),
          "danger",
        );
        return;
      }

      setProcessingStage("saving");

      const updated = await replaceCorpusDocuments(selected.id, nextLines, nextSources);

      setSelected(updated);
      setCorpora((current) => mergeCorpora(current ?? [], updated));
      const details = await getCorpus(selected.id);

      setDocuments(details.documents);
      setSearchResult(null);
      setAnswerResult(null);
      setSelectedHit(null);

      setProcessingStage("vectorizing");

      await runVectorization(updated.id, updated.documentCount);

      if (failedPaths.length > 0) {
        push(t("embeddings.reindex.partial", { count: failedPaths.length }), "danger");
      }
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setReplaceBusy(false);
      setProcessingStage("idle");
      setExtractProgress({ completed: 0, total: 0 });
    }
  }

  function handleVectorizeEvent(event: VectorizeEvent) {
    if (event.type === "started") {
      setVectorizeRequestId(event.requestId);
      setVectorizeProgress({ completed: 0, total: event.totalDocuments });

      return;
    }

    setVectorizeProgress({ completed: event.completed, total: event.totalDocuments });
    setVectorizeUsage((current) => ({
      promptTokens: current.promptTokens + event.usage.promptTokens,
      totalTokens: current.totalTokens + event.usage.totalTokens,
    }));
  }

  async function cancelVectorization() {
    if (!vectorizeRequestId) {
      return;
    }

    await cancelVectorize(vectorizeRequestId);
  }

  async function runSearch(queryOverride?: string) {
    if (!selected || !searchReady || searchInteractionBusy) {
      return;
    }

    const activeQuery = (queryOverride ?? searchQuery).trim();

    if (!activeQuery) {
      return;
    }

    const parsed = Number(topK);

    setSearchBusy(true);

    try {
      const result = await searchCorpus({
        corpusId: selected.id,
        query: activeQuery,
        topK: Number.isFinite(parsed) ? parsed : 5,
      });

      setAnswerResult(null);
      setSearchResult(result);
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setSearchBusy(false);
    }
  }

  async function runSemanticAnswer() {
    if (!selected || !searchReady || searchInteractionBusy) {
      return;
    }

    const activeQuery = searchQuery.trim();

    if (!activeQuery) {
      return;
    }

    const parsed = Number(topK);

    setAnswerResult(null);
    setAnswerBusy(true);

    try {
      const result = await answerCorpus({
        corpusId: selected.id,
        query: activeQuery,
        topK: Number.isFinite(parsed) ? parsed : 5,
      });

      setSearchResult(result.search);
      setAnswerResult(result);
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setAnswerBusy(false);
    }
  }

  function refineSearchByTerm(term: string) {
    const nextQuery = term.trim();

    if (!nextQuery) {
      return;
    }

    if (!selected || !searchReady || searchInteractionBusy) {
      return;
    }

    setSearchQuery(nextQuery);
    void runSearch(nextQuery);
  }

  async function loadEmbeddingModelAlias(alias: string) {
    if (loadingModelAlias) {
      return;
    }

    setLoadingModelAlias(alias);

    try {
      await loadModel(alias);
      await refreshModels();
      setBanner(null);
      push(t("models.actions.loaded", { alias }), "neutral");
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setLoadingModelAlias(null);
    }
  }

  async function downloadSelectedModel() {
    if (!selected || selectedModelJob?.status === "running") {
      return;
    }

    try {
      await downloadModel(selected.modelAlias);
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    }
  }

  async function copyHitText() {
    if (!selectedHit) {
      return;
    }

    const copied = await copyText(selectedHit.text);

    push(t(copied ? "embeddings.search.copied" : "embeddings.search.copyFailed"), copied ? "neutral" : "danger");
  }

  async function copySemanticAnswer() {
    if (!answerResult) {
      return;
    }

    const copied = await copyText(answerResult.answer);

    push(
      t(copied ? "embeddings.search.answerCopied" : "embeddings.search.answerCopyFailed"),
      copied ? "neutral" : "danger",
    );
  }

  async function copyHitSourcePath() {
    const sourcePath = selectedHit?.sourcePath;

    if (!sourcePath) {
      return;
    }

    const copied = await copyText(sourcePath);

    push(
      t(copied ? "embeddings.search.sourcePathCopied" : "embeddings.search.sourcePathCopyFailed"),
      copied ? "neutral" : "danger",
    );
  }

  async function openHitSource() {
    const sourcePath = selectedHit?.sourcePath;

    if (!sourcePath || openingSourcePath) {
      return;
    }

    setOpeningSourcePath(sourcePath);

    try {
      const revealed = await revealCorpusSource(sourcePath);

      if (!revealed) {
        push(t("embeddings.search.sourceMissing"), "danger");
      }
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setOpeningSourcePath(null);
    }
  }

  async function removeCorpus() {
    const target = deleteTarget;

    if (!target || deleting) {
      return;
    }

    setDeleting(true);

    try {
      await deleteCorpus(target.id);
      setDeleteTarget(null);
      setBanner(null);
      await refreshCorpora(archivedView);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    } finally {
      setDeleting(false);
    }
  }

  async function switchArchivedView() {
    const next = !archivedView;

    setArchivedView(next);
    setCorpora(null);
    setSelectedId(null);
    setListQuery("");
    setFlowMode("none");
    setFlowCorpusId(null);
    setActiveFlowStep(4);
    setStagedFiles([]);

    try {
      const items = await listCorpora(next);

      setCorpora(items);
      setSelectedId(items[0]?.id ?? null);
    } catch (error) {
      setCorpora([]);
      setBanner(toFoundryCommandError(error));
    }
  }

  async function toggleFavorite(corpus: EmbeddingCorpus) {
    try {
      const updated = await setCorpusFavorite(corpus.id, !corpus.favorite);

      setCorpora((current) => mergeCorpora(current ?? [], updated));
      setSelected((current) => (current?.id === updated.id ? updated : current));
      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    }
  }

  async function toggleArchived(corpus: EmbeddingCorpus) {
    try {
      const updated = await setCorpusArchived(corpus.id, !corpus.archived);
      const remaining = (corpora ?? []).filter((entry) => entry.id !== updated.id);

      setCorpora(remaining);

      if (selectedId === updated.id) {
        setSelectedId(remaining[0]?.id ?? null);
      }

      setBanner(null);
    } catch (error) {
      setBanner(toFoundryCommandError(error));
    }
  }

  const menuItems: ContextMenuItem[] = menu
    ? [
        {
          label: menu.corpus.favorite ? t("embeddings.unfavorite") : t("embeddings.favorite"),
          icon: StarIcon,
          onSelect: () => void toggleFavorite(menu.corpus),
        },
        {
          label: archivedView ? t("embeddings.unarchive") : t("embeddings.archive"),
          icon: ArchiveIcon,
          onSelect: () => void toggleArchived(menu.corpus),
        },
        {
          label: t("embeddings.deleteForever"),
          icon: TrashIcon,
          tone: "danger",
          onSelect: () => setDeleteTarget(menu.corpus),
        },
      ]
    : [];

  const missingDownloadedEmbeddingModels = cachedEmbeddingModels.length === 0;
  const missingLoadedEmbeddingModels = loadedEmbeddingModels.length === 0 && cachedEmbeddingModels.length > 0;
  const panelClass = "min-w-0 rounded-[9px] border border-line bg-canvas p-3 sm:p-3.5";
  const sectionTitleClass = "text-[11px] font-semibold uppercase tracking-[0.055em] text-tertiary";
  const fieldClass =
    "h-8 w-full min-w-0 rounded-md border border-line bg-canvas px-2.5 text-[12px] text-primary outline-none transition-colors focus:border-line-strong";
  const secondaryButtonClass =
    "inline-flex h-7 min-w-0 items-center justify-center gap-1.5 rounded-md border border-line bg-canvas px-2.5 text-[11.5px] text-secondary transition-colors hover:bg-hover hover:text-primary disabled:opacity-50";
  const primaryButtonClass =
    "inline-flex h-8 min-w-0 items-center justify-center gap-1.5 rounded-md bg-accent px-3 text-[12px] font-medium text-accent-contrast transition-colors hover:bg-accent-hover disabled:opacity-50";
  const detailWidthClass = showingStep4 ? "max-w-[1180px]" : "max-w-[820px]";

  return (
    <>
      <ContextList
        title={archivedView ? t("embeddings.archivedTitle") : t("embeddings.listTitle")}
        action={
          <div className="flex items-center gap-1">
            <IconButton
              label={archivedView ? t("embeddings.archivedBack") : t("embeddings.archivedTitle")}
              onClick={() => void switchArchivedView()}
              className={archivedView ? "border-line-strong bg-active text-primary" : ""}
            >
              <ArchiveIcon size={14} />
            </IconButton>
            <IconButton
              label={t("embeddings.listFilterPlaceholder")}
              onClick={() => setListFilterOpen((current) => !current)}
            >
              <SearchIcon size={14} />
            </IconButton>
            <IconButton
              label={t("embeddings.actions.newCorpus")}
              disabled={creatingCorpus || vectorizing || replaceBusy || searchInteractionBusy || deleting}
              onClick={() => void startNewCorpusFlow()}
            >
              <PlusIcon size={14} />
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
                placeholder={t("embeddings.listFilterPlaceholder")}
                aria-label={t("embeddings.listFilterPlaceholder")}
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
        {corpora === null ? (
          <div className="flex justify-center py-6">
            <Spinner size={14} className="text-tertiary" />
          </div>
        ) : corpora.length === 0 ? (
          <ListEmpty
            icon={archivedView ? ArchiveIcon : EmbeddingsIcon}
            title={archivedView ? t("embeddings.archivedEmptyTitle") : t("embeddings.listEmptyTitle")}
            body={archivedView ? t("embeddings.archivedEmptyBody") : t("embeddings.listEmptyBody")}
          />
        ) : filteredCorpora.length === 0 ? (
          <ListEmpty
            icon={SearchIcon}
            title={t("embeddings.listNoMatchesTitle")}
            body={t("embeddings.listNoMatchesBody")}
          />
        ) : (
          <ul className="flex flex-col gap-1 pb-2">
            {filteredCorpora.map((corpus) => {
              const active = corpus.id === selectedId;

              return (
                <li
                  key={corpus.id}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    setMenu({ corpus, x: event.clientX, y: event.clientY });
                  }}
                >
                  <button
                    type="button"
                    disabled={vectorizing || replaceBusy || searchInteractionBusy}
                    aria-current={active ? "true" : undefined}
                    onClick={() => {
                      setFlowMode("none");
                      setFlowCorpusId(null);
                      setActiveFlowStep(4);
                      setStagedFiles([]);
                      setSelectedId(corpus.id);
                    }}
                    onKeyDown={(event) => {
                      if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
                        event.preventDefault();

                        const rect = event.currentTarget.getBoundingClientRect();

                        setMenu({ corpus, x: rect.right - 12, y: rect.top + 26 });
                      }
                    }}
                    className={[
                      "flex w-full flex-col gap-1 rounded-[var(--item-radius)] border px-[var(--item-pad-x)] py-[var(--item-pad-y)] text-left transition-colors",
                      active
                        ? "border-line-strong bg-elevated shadow-[var(--shadow-thumb)]"
                        : "border-transparent hover:border-line hover:bg-hover",
                    ].join(" ")}
                  >
                    <span className="flex w-full min-w-0 items-center gap-1.5">
                      {corpus.favorite ? (
                        <StarIcon size={11} className="shrink-0 fill-current text-accent" />
                      ) : null}
                      <span className="min-w-0 flex-1 truncate text-[12px] leading-[1.4] text-primary">
                        {corpus.name}
                      </span>
                    </span>
                    <span className="flex items-center justify-between gap-2 text-[10.5px]">
                      <span className="min-w-0 truncate text-secondary">{corpus.modelAlias}</span>
                      <span className="shrink-0 tabular-nums text-tertiary">{corpus.vectorCount}/{corpus.documentCount}</span>
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
          label={t("embeddings.corpusActions")}
          items={menuItems}
          onClose={() => setMenu(null)}
        />
      ) : null}

      <DetailPane
        title={selected ? selected.name : t("embeddings.title")}
        {...(selected ? { subtitle: selected.modelAlias } : {})}
        actions={
          selected ? (
            <>
              <IconButton
                label={t("embeddings.actions.deleteCorpus")}
                onClick={() => setDeleteTarget(selected)}
              >
                <TrashIcon size={14} />
              </IconButton>
            </>
          ) : undefined
        }
      >
        {!selected ? (
          <EmptyState icon={EmbeddingsIcon} title={t("embeddings.emptyTitle")} body={t("embeddings.emptyBody")} />
        ) : (
          <div className={`mx-auto flex min-h-full w-full min-w-0 ${detailWidthClass} flex-col gap-4 px-4 py-5 sm:px-6 sm:py-6`}>
            <div className="flex flex-wrap items-center justify-between gap-3">
              <p role="status" className="text-[12.5px] text-secondary">
                {topStatusMessage}
              </p>
              {!isCreateFlow ? (
                <div className="flex flex-wrap gap-2">
                  <button
                    type="button"
                    onClick={() => {
                      setFlowMode(isAppendFlow ? "none" : "append");
                      if (!isAppendFlow) {
                        setStagedFiles([]);
                        setFlowTransition("forward");
                      }
                    }}
                    className={`${secondaryButtonClass} h-8 w-full sm:w-auto`}
                  >
                    {isAppendFlow ? t("embeddings.flow.closeAppend") : t("embeddings.flow.addFiles")}
                  </button>
                  <button
                    type="button"
                    onClick={() => void runRevectorizeCurrent()}
                    disabled={!selected.documentCount || vectorizing || replaceBusy || processingStage !== "idle" || modelBusy}
                    className={`${secondaryButtonClass} h-8 w-full sm:w-auto`}
                  >
                    {vectorizing && processingStage === "vectorizing" ? <Spinner size={11} /> : null}
                    {t("embeddings.actions.revectorize")}
                  </button>
                  <button
                    type="button"
                    onClick={() => void runReindexFromSources()}
                    disabled={
                      !selected.documentCount
                      || sourceFileCount === 0
                      || vectorizing
                      || replaceBusy
                      || processingStage !== "idle"
                      || modelBusy
                    }
                    className={`${secondaryButtonClass} h-8 w-full sm:w-auto`}
                  >
                    {processingStage === "extracting" || processingStage === "saving" ? <Spinner size={11} /> : null}
                    {t("embeddings.actions.reindex")}
                  </button>
                </div>
              ) : activeFlowStep === 4 ? (
                <button
                  type="button"
                  onClick={() => {
                    setFlowMode("none");
                    setFlowCorpusId(null);
                    setActiveFlowStep(4);
                    setStagedFiles([]);
                  }}
                  className={`${secondaryButtonClass} h-8 w-full sm:w-auto`}
                >
                  {t("embeddings.flow.finish")}
                </button>
              ) : null}
            </div>

            {banner ? (
              <div className="flex items-start gap-2 rounded-[8px] border border-line bg-subtle px-3 py-2 text-[12px] text-danger">
                <AlertIcon size={14} className="shrink-0" />
                <span className="min-w-0 flex-1">{errorLabel(banner, t)}</span>
              </div>
            ) : null}

            {missingDownloadedEmbeddingModels || missingLoadedEmbeddingModels ? (
              <ModelAccessPanel
                title={
                  missingDownloadedEmbeddingModels
                    ? t("common.noDownloadedModelTitle", { kind: t("models.kinds.embedding") })
                    : t("common.noLoadedModelTitle", { kind: t("models.kinds.embedding") })
                }
                body={
                  missingDownloadedEmbeddingModels
                    ? t("common.noDownloadedModelBody", { kind: t("models.kinds.embedding") })
                    : t("common.noLoadedModelBody", { kind: t("models.kinds.embedding") })
                }
                listTitle={t("common.downloadedModels")}
                models={loadableEmbeddingAliases}
                loadLabel={t("models.actions.load")}
                loadingLabel={t("common.loading")}
                openModelsLabel={t("common.openModels")}
                loadingAlias={loadingModelAlias}
                onLoad={(alias) => void loadEmbeddingModelAlias(alias)}
                onOpenModels={() => onOpenModels?.()}
              />
            ) : null}

            {selectedModelNeedsDownload || selectedModelNeedsLoad || selectedModelUnavailable ? (
              <section className="rounded-[9px] border border-line bg-subtle px-3 py-3" role="status" aria-live="polite">
                <div className="flex items-start gap-2">
                  <AlertIcon size={14} className="mt-0.5 shrink-0 text-secondary" />
                  <div className="min-w-0 flex-1">
                    <p className="text-[12px] font-medium text-primary">
                      {selectedModelUnavailable
                        ? t("embeddings.model.unavailableTitle")
                        : selectedModelNeedsDownload
                          ? t("embeddings.model.downloadTitle", { alias: selected.modelAlias })
                          : t("embeddings.model.loadTitle", { alias: selected.modelAlias })}
                    </p>
                    <p className="mt-1 text-[11.5px] leading-relaxed text-secondary">
                      {selectedModelUnavailable
                        ? t("embeddings.model.unavailableBody")
                        : selectedModelNeedsDownload
                          ? t("embeddings.model.downloadBody")
                          : t("embeddings.model.loadBody")}
                    </p>
                  </div>
                </div>
                <div className="mt-3 flex flex-wrap justify-end gap-2">
                  {selectedModelNeedsLoad ? (
                    <button
                      type="button"
                      onClick={() => void loadEmbeddingModelAlias(selected.modelAlias)}
                      disabled={loadingModelAlias !== null}
                      className={secondaryButtonClass}
                    >
                      {loadingModelAlias === selected.modelAlias ? <Spinner size={11} /> : null}
                      {t("models.actions.load")}
                    </button>
                  ) : null}
                  {selectedModelNeedsDownload ? (
                    <button
                      type="button"
                      onClick={() => void downloadSelectedModel()}
                      disabled={selectedModelJob?.status === "running"}
                      className={secondaryButtonClass}
                    >
                      {selectedModelJob?.status === "running" ? <Spinner size={11} /> : <DownloadIcon size={12} />}
                      {selectedModelJob?.status === "running" ? t("embeddings.model.downloading") : t("models.actions.download")}
                    </button>
                  ) : null}
                  <button
                    type="button"
                    onClick={() => onOpenModels?.()}
                    className={secondaryButtonClass}
                  >
                    {t("common.openModels")}
                  </button>
                </div>
              </section>
            ) : null}

            {isCreateFlow ? (
              <>
                <ol className="grid gap-2 sm:grid-cols-4" aria-label={t("embeddings.flow.stepsLabel")}>
                  {[1, 2, 3, 4].map((value) => {
                    const step = value as FlowStep;
                    const active = activeFlowStep === step;
                    const done = step < activeFlowStep;
                    const locked = step > furthestStep;

                    return (
                      <li
                        key={step}
                        className={[
                          "rounded-[8px] border",
                          active
                            ? "border-line-strong bg-elevated"
                            : done
                              ? "border-line bg-canvas"
                              : "border-line bg-subtle",
                        ].join(" ")}
                      >
                        <button
                          type="button"
                          onClick={() => goToFlowStep(step)}
                          disabled={locked}
                          className="flex w-full items-center gap-2 px-2.5 py-2 text-left disabled:cursor-not-allowed disabled:opacity-60"
                        >
                          <span
                            className={[
                              "inline-flex h-5 w-5 items-center justify-center rounded-full text-[10.5px] font-semibold",
                              done || active ? "bg-accent text-accent-contrast" : "bg-subtle text-secondary",
                            ].join(" ")}
                          >
                            {step}
                          </span>
                          <span className="text-[11.5px] text-secondary">
                            {step === 1
                              ? t("embeddings.flow.step1")
                              : step === 2
                                ? t("embeddings.flow.step2")
                                : step === 3
                                  ? t("embeddings.flow.step3")
                                  : t("embeddings.flow.step4")}
                          </span>
                        </button>
                      </li>
                    );
                  })}
                </ol>

                {activeFlowStep === 1 ? (
                  <section className={`${panelClass} ${flowTransition === "forward" ? "speech-flow-panel speech-flow-panel-forward" : "speech-flow-panel speech-flow-panel-backward"}`}>
                    <div className="mb-3 space-y-1">
                      <h2 className={sectionTitleClass}>{t("embeddings.flow.step1PanelTitle")}</h2>
                      <p className="text-[11.5px] text-secondary">{t("embeddings.flow.step1Hint")}</p>
                    </div>
                    <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-[minmax(0,1fr)_240px_auto] lg:items-end">
                      <label className="flex min-w-0 flex-col gap-1">
                        <span className="text-[11px] text-secondary">{t("embeddings.corpus.name")}</span>
                        <input
                          value={nameDraft}
                          onChange={(event) => setNameDraft(event.target.value)}
                          className={fieldClass}
                        />
                      </label>

                      <label className="flex min-w-0 flex-col gap-1">
                        <span className="text-[11px] text-secondary">{t("embeddings.corpus.model")}</span>
                        <select
                          value={selected.modelAlias}
                          onChange={(event) => void changeModel(event.target.value)}
                          className={fieldClass}
                        >
                          {embeddingModels.map((model) => (
                            <option key={model.alias} value={model.alias} disabled={!model.cached}>
                              {model.alias}
                            </option>
                          ))}
                        </select>
                      </label>

                      <button
                        type="button"
                        onClick={() => void saveCorpusName()}
                        disabled={renameBusy || !nameDraft.trim()}
                        className={`${secondaryButtonClass} h-8 w-full sm:w-auto`}
                      >
                        {renameBusy ? t("embeddings.actions.saving") : t("embeddings.actions.saveName")}
                      </button>
                    </div>
                    <div className="mt-2.5 flex flex-wrap items-center gap-3 text-[10.5px] text-secondary">
                      <span>{t("embeddings.corpus.documentsCount", { count: selected.documentCount })}</span>
                      <span>{t("embeddings.corpus.vectorizedCount", { count: selected.vectorCount })}</span>
                      <span>{t("embeddings.corpus.dimensions", { count: selected.dimensions })}</span>
                    </div>
                    <div className="mt-3 flex flex-wrap justify-end gap-2">
                      <button
                        type="button"
                        onClick={() => goToFlowStep(2)}
                        disabled={!canGoStep2}
                        className={`${primaryButtonClass} w-full sm:w-auto`}
                      >
                        {t("embeddings.flow.nextToFiles")}
                      </button>
                    </div>
                  </section>
                ) : null}

                {activeFlowStep === 2 ? (
                  <section className={`${panelClass} ${flowTransition === "forward" ? "speech-flow-panel speech-flow-panel-forward" : "speech-flow-panel speech-flow-panel-backward"}`}>
                    <div className="mb-3 flex flex-wrap items-start justify-between gap-2">
                      <div className="space-y-1">
                        <h2 className={sectionTitleClass}>{t("embeddings.flow.step2PanelTitle")}</h2>
                        <p className="text-[11.5px] text-secondary">{t("embeddings.flow.step2Hint")}</p>
                      </div>
                      <button
                        type="button"
                        onClick={() => void addStagedFiles()}
                        disabled={replaceBusy || vectorizing}
                        className={secondaryButtonClass}
                      >
                        <PaperclipIcon size={12} />
                        {t("embeddings.documents.import")}
                      </button>
                    </div>

                    {stagedFiles.length ? (
                      <ul className="space-y-1.5 rounded-[8px] border border-line bg-canvas p-2">
                        {stagedFiles.map((path) => (
                          <li key={path} className="flex items-center justify-between gap-2 rounded-[7px] border border-line bg-canvas px-2 py-1.5">
                            <div className="min-w-0">
                              <p className="truncate text-[11.5px] text-primary">{fileName(path)}</p>
                              <p className="truncate text-[10.5px] text-tertiary">{path}</p>
                            </div>
                            <button
                              type="button"
                              onClick={() => removeStagedFile(path)}
                              disabled={replaceBusy || vectorizing}
                              className={secondaryButtonClass}
                            >
                              <TrashIcon size={11} />
                              {t("embeddings.documents.remove")}
                            </button>
                          </li>
                        ))}
                      </ul>
                    ) : (
                      <p className="rounded-[8px] border border-dashed border-line px-3 py-4 text-[11.5px] text-secondary">
                        {t("embeddings.documents.empty")}
                      </p>
                    )}

                    <p className="mt-2 text-[10.5px] text-secondary">
                      {t("embeddings.documents.selected", { count: stagedFiles.length })}
                    </p>

                    <div className="mt-3 flex flex-wrap justify-end gap-2">
                      <button
                        type="button"
                        onClick={() => goToFlowStep(1)}
                        className={`${secondaryButtonClass} h-8`}
                      >
                        {t("embeddings.flow.backToConfig")}
                      </button>
                      <button
                        type="button"
                        onClick={() => goToFlowStep(3)}
                        disabled={!canGoStep3}
                        className={`${primaryButtonClass} w-full sm:w-auto`}
                      >
                        {t("embeddings.flow.nextToVectorize")}
                      </button>
                    </div>
                  </section>
                ) : null}

                {activeFlowStep === 3 ? (
                  <section className={`${panelClass} ${flowTransition === "forward" ? "speech-flow-panel speech-flow-panel-forward" : "speech-flow-panel speech-flow-panel-backward"}`}>
                    <div className="mb-3 space-y-1">
                      <h2 className={sectionTitleClass}>{t("embeddings.flow.step3PanelTitle")}</h2>
                      <p className="text-[11.5px] text-secondary">{t("embeddings.flow.step3Hint")}</p>
                    </div>

                    <div className="grid gap-2 rounded-[8px] border border-line bg-subtle p-2.5 text-[11.5px] sm:grid-cols-2">
                      <div className="min-w-0">
                        <p className="text-tertiary">{t("embeddings.corpus.model")}</p>
                        <p className="truncate text-primary">{selected.modelAlias}</p>
                      </div>
                      <div className="min-w-0">
                        <p className="text-tertiary">{t("embeddings.documents.selected", { count: stagedFiles.length })}</p>
                        <p className="truncate text-primary">{stagedFiles.length}</p>
                      </div>
                      <div className="sm:col-span-2">
                        <p className="text-tertiary">{t("embeddings.flow.step3Files")}</p>
                        <p className="line-clamp-2 text-primary">
                          {stagedFiles.length ? stagedFiles.map((path) => fileName(path)).join(" · ") : "—"}
                        </p>
                      </div>
                    </div>

                    <div className="mt-3" role="progressbar" aria-label={t("embeddings.vectorize.title")} aria-valuemin={0} aria-valuemax={Math.max(1, progressNumbers.total)} aria-valuenow={progressNumbers.completed}>
                      <div className="h-2 overflow-hidden rounded-full bg-subtle">
                        <div
                          className="h-full rounded-full bg-accent"
                          style={{
                            width:
                              progressNumbers.total > 0
                                ? `${Math.round((progressNumbers.completed / progressNumbers.total) * 100)}%`
                                : "0%",
                          }}
                        />
                      </div>
                    </div>

                    <div className="mt-2 flex flex-wrap items-center gap-x-4 gap-y-1 text-[10.5px] text-secondary">
                      <span>{progressLabel}</span>
                      <span>{t("embeddings.tokens", { count: vectorizeUsage.totalTokens })}</span>
                    </div>

                    <div className="mt-3 flex flex-wrap justify-end gap-2">
                      <button
                        type="button"
                        onClick={() => goToFlowStep(2)}
                        disabled={vectorizing || replaceBusy}
                        className={`${secondaryButtonClass} h-8`}
                      >
                        {t("embeddings.flow.backToFiles")}
                      </button>
                      <button
                        type="button"
                        onClick={() => (vectorizing ? void cancelVectorization() : void runFlowVectorization("create"))}
                        disabled={vectorizing ? !vectorizeRequestId : !canGoStep3 || replaceBusy || modelBusy}
                        className={`${vectorizing ? secondaryButtonClass : primaryButtonClass} w-full sm:w-auto`}
                      >
                        {vectorizing ? <StopIcon size={11} /> : <PlayIcon size={12} />}
                        {vectorizeActionLabel}
                      </button>
                      <button
                        type="button"
                        onClick={() => goToFlowStep(4)}
                        disabled={!canGoStep4 || vectorizing || replaceBusy}
                        className={`${primaryButtonClass} w-full sm:w-auto`}
                      >
                        {t("embeddings.flow.nextToSearch")}
                      </button>
                    </div>
                  </section>
                ) : null}

                {activeFlowStep === 4 ? (
                  <section className={`${panelClass} ${flowTransition === "forward" ? "speech-flow-panel speech-flow-panel-forward" : "speech-flow-panel speech-flow-panel-backward"}`}>
                    <h2 className={sectionTitleClass}>{t("embeddings.flow.step4PanelTitle")}</h2>
                    <p className="mt-1 text-[11.5px] text-secondary">{t("embeddings.flow.step4Hint")}</p>
                  </section>
                ) : null}
              </>
            ) : null}

            {isAppendFlow ? (
              <section className={`${panelClass} ${flowTransition === "forward" ? "speech-flow-panel speech-flow-panel-forward" : "speech-flow-panel speech-flow-panel-backward"}`}>
                <div className="mb-3 flex flex-wrap items-start justify-between gap-2">
                  <div className="space-y-1">
                    <h2 className={sectionTitleClass}>{t("embeddings.flow.appendTitle")}</h2>
                    <p className="text-[11.5px] text-secondary">{t("embeddings.flow.appendHint")}</p>
                  </div>
                  <button
                    type="button"
                    onClick={() => void addStagedFiles()}
                    disabled={replaceBusy || vectorizing}
                    className={secondaryButtonClass}
                  >
                    <PaperclipIcon size={12} />
                    {t("embeddings.documents.import")}
                  </button>
                </div>

                {stagedFiles.length ? (
                  <ul className="space-y-1.5 rounded-[8px] border border-line bg-canvas p-2">
                    {stagedFiles.map((path) => (
                      <li key={path} className="flex items-center justify-between gap-2 rounded-[7px] border border-line bg-canvas px-2 py-1.5">
                        <div className="min-w-0">
                          <p className="truncate text-[11.5px] text-primary">{fileName(path)}</p>
                          <p className="truncate text-[10.5px] text-tertiary">{path}</p>
                        </div>
                        <button
                          type="button"
                          onClick={() => removeStagedFile(path)}
                          disabled={replaceBusy || vectorizing}
                          className={secondaryButtonClass}
                        >
                          <TrashIcon size={11} />
                          {t("embeddings.documents.remove")}
                        </button>
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p className="rounded-[8px] border border-dashed border-line px-3 py-4 text-[11.5px] text-secondary">
                    {t("embeddings.documents.empty")}
                  </p>
                )}

                <div className="mt-3" role="progressbar" aria-label={t("embeddings.vectorize.title")} aria-valuemin={0} aria-valuemax={Math.max(1, progressNumbers.total)} aria-valuenow={progressNumbers.completed}>
                  <div className="h-2 overflow-hidden rounded-full bg-subtle">
                    <div
                      className="h-full rounded-full bg-accent"
                      style={{
                        width:
                          progressNumbers.total > 0
                            ? `${Math.round((progressNumbers.completed / progressNumbers.total) * 100)}%`
                            : "0%",
                      }}
                    />
                  </div>
                </div>
                <div className="mt-2 flex flex-wrap items-center gap-x-4 gap-y-1 text-[10.5px] text-secondary">
                  <span>{progressLabel}</span>
                  <span>{t("embeddings.tokens", { count: vectorizeUsage.totalTokens })}</span>
                </div>

                <div className="mt-3 flex flex-wrap justify-end gap-2">
                  <button
                    type="button"
                    onClick={() => {
                      setFlowMode("none");
                      setStagedFiles([]);
                      setExtractProgress({ completed: 0, total: 0 });
                    }}
                    disabled={replaceBusy || vectorizing}
                    className={`${secondaryButtonClass} h-8`}
                  >
                    {t("common.cancel")}
                  </button>
                  <button
                    type="button"
                    onClick={() => (vectorizing ? void cancelVectorization() : void runFlowVectorization("append"))}
                    disabled={vectorizing ? !vectorizeRequestId : !stagedFiles.length || replaceBusy || modelBusy}
                    className={`${vectorizing ? secondaryButtonClass : primaryButtonClass} w-full sm:w-auto`}
                  >
                    {vectorizing ? <StopIcon size={11} /> : <PlayIcon size={12} />}
                    {vectorizeActionLabel}
                  </button>
                </div>
              </section>
            ) : null}

            {showingStep4 ? (
              <section className="min-h-0 flex-1 rounded-[9px] border border-line bg-canvas p-3 sm:p-3.5">
                <div className="mb-3 flex items-center justify-between gap-3">
                  <h2 className={sectionTitleClass}>{t("embeddings.tabs.search")}</h2>
                </div>

                {!searchReady ? (
                  <div className="mb-3 rounded-[8px] border border-line bg-subtle px-3 py-2 text-[11.5px] text-secondary">
                    {selected.documentCount === 0
                      ? t("embeddings.flow.step4NeedFiles")
                      : t("embeddings.flow.step4NeedVectorization")}
                  </div>
                ) : null}

                <div className="flex h-full min-h-[260px] flex-col gap-3">
                  <div className="grid gap-2 lg:grid-cols-[minmax(0,1fr)_auto_auto] lg:items-end">
                    <label className="flex flex-col gap-1">
                      <span className="text-[11px] text-secondary">{t("embeddings.search.query")}</span>
                      <input
                        value={searchQuery}
                        onChange={(event) => setSearchQuery(event.target.value)}
                        onKeyDown={(event) => {
                          if (event.key === "Enter") {
                            event.preventDefault();
                            void runSearch();
                          }
                        }}
                        disabled={!searchReady}
                        className={fieldClass}
                      />
                    </label>
                    <button
                      type="button"
                      onClick={() => void runSearch()}
                      disabled={searchInteractionBusy || !searchReady || !searchQuery.trim()}
                      className={`${primaryButtonClass} w-full lg:w-auto`}
                    >
                      {searchBusy ? t("embeddings.search.searching") : t("embeddings.search.search")}
                    </button>
                    <button
                      type="button"
                      onClick={() => void runSemanticAnswer()}
                      disabled={searchInteractionBusy || !searchReady || !searchQuery.trim()}
                      className={`${secondaryButtonClass} h-8 w-full lg:w-auto`}
                    >
                      {answerBusy ? <Spinner size={11} /> : null}
                      {answerBusy ? t("embeddings.search.answering") : t("embeddings.search.answer")}
                    </button>
                  </div>

                  <div className="grid gap-2 rounded-[8px] border border-line bg-subtle p-2.5 sm:grid-cols-[140px_minmax(0,1fr)] sm:items-end lg:grid-cols-[160px_minmax(0,1fr)]">
                    <label className="flex flex-col gap-1">
                      <span className="text-[11px] text-secondary">{t("embeddings.flow.resultCount")}</span>
                      <input
                        type="number"
                        min={1}
                        max={50}
                        value={topK}
                        onChange={(event) => setTopK(event.target.value)}
                        className={fieldClass}
                      />
                    </label>
                    <label className="flex h-8 items-center gap-2 rounded-md border border-line bg-canvas px-2.5 text-[11.5px] text-secondary">
                      <input
                        type="checkbox"
                        checked={searchExistingSourcesOnly}
                        onChange={(event) => setSearchExistingSourcesOnly(event.target.checked)}
                        className="h-3.5 w-3.5 rounded border-line bg-canvas"
                      />
                      <span>{t("embeddings.flow.onlyExistingSources")}</span>
                    </label>
                  </div>

                  {searchResult ? (
                    <div role="status" className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[10.5px] text-secondary">
                      <span>{t("embeddings.tokens", { count: searchResult.usage.totalTokens })}</span>
                      <span>{t("embeddings.search.rankedBy")}</span>
                      <span>{t("embeddings.search.resultsShown", { shown: filteredSearchHits.length, total: searchResult.hits.length })}</span>
                    </div>
                  ) : null}
                  {searchResult?.hits.length ? (
                    <p className="text-[10.5px] text-tertiary">{t("embeddings.search.refineHint")}</p>
                  ) : null}

                  {answerResult ? (
                    <section className="rounded-[8px] border border-line bg-subtle px-3 py-2.5">
                      <div className="flex flex-wrap items-center justify-between gap-2">
                        <h3 className="text-[11px] font-semibold uppercase tracking-[0.05em] text-tertiary">
                          {t("embeddings.search.semanticAnswer")}
                        </h3>
                        <button type="button" onClick={() => void copySemanticAnswer()} className={secondaryButtonClass}>
                          <CopyIcon size={12} />
                          {t("embeddings.search.copyAnswer")}
                        </button>
                      </div>
                      <p className="mt-2 whitespace-pre-wrap text-[12px] leading-relaxed text-primary">{answerResult.answer}</p>
                      <div className="mt-2 flex flex-wrap gap-x-3 gap-y-1 text-[10.5px] text-secondary">
                        <span>{t("embeddings.search.answerModel", { alias: answerResult.modelAlias })}</span>
                        <span>{t("embeddings.search.answerTokens", { count: answerResult.usage.totalTokens })}</span>
                        <span>{t("embeddings.search.answerSources", { count: answerResult.citations.length })}</span>
                      </div>
                      {answerResult.citations.length ? (
                        <div className="mt-2 flex flex-wrap gap-1.5">
                          {answerResult.citations.map((hit, index) => (
                            <button
                              key={`${hit.documentId}-${index}`}
                              type="button"
                              onClick={() => setSelectedHit(hit)}
                              className="inline-flex h-6 items-center rounded-md border border-line bg-canvas px-2 text-[10.5px] text-secondary transition-colors hover:border-line-strong hover:text-primary"
                            >
                              [{index + 1}] {hit.sourcePath ? fileName(hit.sourcePath) : t("embeddings.search.sourceUnknown")}
                            </button>
                          ))}
                        </div>
                      ) : null}
                    </section>
                  ) : null}

                  <div className="grid min-h-0 flex-1 gap-3 xl:grid-cols-[380px_minmax(0,1fr)]">
                    <div className="min-h-0 overflow-y-auto rounded-[8px] border border-line bg-canvas p-2.5">
                      {filteredSearchHits.length ? (
                        <ul className="flex flex-col gap-2">
                          {filteredSearchHits.map((hit, index) => {
                            const title = hit.sourcePath ? fileName(hit.sourcePath) : `#${hit.position + 1}`;

                            return (
                              <li key={hit.documentId}>
                                <button
                                  type="button"
                                  onClick={() => setSelectedHit(hit)}
                                  className={[
                                    "w-full rounded-[8px] border px-2.5 py-2 text-left transition-colors",
                                    selectedHit?.documentId === hit.documentId
                                      ? "border-line-strong bg-active"
                                      : "border-line bg-canvas hover:border-line-strong hover:bg-hover",
                                  ].join(" ")}
                                >
                                  <div className="flex items-start justify-between gap-2">
                                    <div className="min-w-0">
                                      <p className="truncate text-[12px] font-medium text-primary">{title}</p>
                                      {hit.sourcePath ? (
                                        <p className="truncate text-[10.5px] text-tertiary">{hit.sourcePath}</p>
                                      ) : (
                                        <p className="text-[10.5px] text-tertiary">{t("embeddings.search.sourceUnknown")}</p>
                                      )}
                                    </div>
                                    <span className="shrink-0 rounded-full border border-line bg-canvas px-2 py-0.5 text-[10px] font-medium tabular-nums text-secondary">
                                      {Math.round(hit.score * 100)}%
                                    </span>
                                  </div>
                                  <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-subtle">
                                    <div className="h-full rounded-full bg-accent" style={{ width: `${hit.score * 100}%` }} />
                                  </div>
                                  <p className="mt-2 line-clamp-3 text-[11px] leading-relaxed text-secondary">
                                    {renderHighlightedText(extractSearchContext(hit.text, searchQuery, 110, 260), searchQuery)}
                                  </p>
                                  <div className="mt-2 flex items-center justify-between text-[10px]">
                                    <span className="text-tertiary">#{index + 1}</span>
                                    <span className={hit.sourceExists ? "text-tertiary" : "text-danger"}>
                                      {hit.sourceExists ? t("embeddings.search.sourceAvailableShort") : t("embeddings.search.sourceMissingShort")}
                                    </span>
                                  </div>
                                </button>
                              </li>
                            );
                          })}
                        </ul>
                      ) : (
                        <p className="px-1 py-2 text-[11px] text-secondary">
                          {searchResult?.hits.length ? t("embeddings.search.emptyAfterFilter") : t("embeddings.search.empty")}
                        </p>
                      )}
                    </div>

                    <div className="min-h-0 rounded-[8px] border border-line bg-canvas p-3">
                      {selectedHit ? (
                        <div className="flex h-full min-h-0 flex-col">
                          <div className="rounded-[8px] border border-line bg-subtle px-2.5 py-2">
                            <p className="truncate text-[12px] font-medium text-primary">
                              {selectedHit.sourcePath ? fileName(selectedHit.sourcePath) : t("embeddings.search.sourceUnknown")}
                            </p>
                            {selectedHit.sourcePath ? (
                              <p className="mt-0.5 truncate text-[10.5px] text-tertiary">{selectedHit.sourcePath}</p>
                            ) : null}
                            <div className="mt-2 flex flex-wrap items-center justify-between gap-2 text-[11px] text-secondary">
                              <span>{t("embeddings.search.position", { position: selectedHit.position + 1 })}</span>
                              <span>{t("embeddings.search.score", { score: selectedHit.score.toFixed(1) })}</span>
                            </div>
                          </div>
                          <div className="mt-3 min-h-0 flex-1 overflow-y-auto rounded-[8px] border border-line bg-canvas p-3">
                            <p className="whitespace-pre-wrap text-[12px] leading-relaxed text-primary">{renderHighlightedText(selectedHitContent, searchQuery, refineSearchByTerm)}</p>
                          </div>
                          <div className="mt-3 flex flex-wrap justify-end gap-2">
                            {selectedHitHasExcerpt ? (
                              <button
                                type="button"
                                onClick={() => setShowFullHitText((current) => !current)}
                                className={secondaryButtonClass}
                              >
                                {showFullHitText ? t("embeddings.search.showExcerpt") : t("embeddings.search.showFull")}
                              </button>
                            ) : null}
                            {selectedHit.sourcePath && selectedHit.sourceExists ? (
                              <button
                                type="button"
                                onClick={() => void openHitSource()}
                                disabled={openingSourcePath === selectedHit.sourcePath}
                                className={secondaryButtonClass}
                              >
                                {openingSourcePath === selectedHit.sourcePath ? <Spinner size={11} /> : null}
                                {t("embeddings.search.openSource")}
                              </button>
                            ) : null}
                            {selectedHit.sourcePath ? (
                              <button
                                type="button"
                                onClick={() => void copyHitSourcePath()}
                                className={secondaryButtonClass}
                              >
                                <CopyIcon size={12} />
                                {t("embeddings.search.copySourcePath")}
                              </button>
                            ) : null}
                            <button
                              type="button"
                              onClick={() => void copyHitText()}
                              className={secondaryButtonClass}
                            >
                              <CopyIcon size={12} />
                              {t("embeddings.search.copy")}
                            </button>
                          </div>
                        </div>
                      ) : (
                        <p className="text-[11px] text-secondary">{t("embeddings.search.pickResult")}</p>
                      )}
                    </div>
                  </div>
                </div>
              </section>
            ) : null}
          </div>
        )}
      </DetailPane>

      <ConfirmDialog
        open={pendingModelAlias !== null}
        title={t("embeddings.model.confirmTitle")}
        body={t("embeddings.model.confirmBody")}
        confirmLabel={t("embeddings.model.confirmAction")}
        onClose={() => setPendingModelAlias(null)}
        onConfirm={() => {
          const alias = pendingModelAlias;

          setPendingModelAlias(null);

          if (alias) {
            void applyModelChange(alias);
          }
        }}
      />

      <ConfirmDialog
        open={deleteTarget !== null}
        title={t("embeddings.delete.title")}
        body={t("embeddings.delete.body")}
        confirmLabel={t("embeddings.delete.confirm")}
        tone="danger"
        busy={deleting}
        onClose={() => setDeleteTarget(null)}
        onConfirm={() => void removeCorpus()}
      />
    </>
  );
}

function mergeCorpora(current: EmbeddingCorpus[], incoming: EmbeddingCorpus): EmbeddingCorpus[] {
  const next = [incoming, ...current.filter((entry) => entry.id !== incoming.id)];

  return next.sort((left, right) => {
    if (left.favorite !== right.favorite) {
      return left.favorite ? -1 : 1;
    }

    return right.updatedAt - left.updatedAt || right.id - left.id;
  });
}

function suggestCorpusName(baseName: string, corpora: EmbeddingCorpus[]): string {
  const normalizedBase = baseName.trim();

  if (!normalizedBase) {
    return "Corpus";
  }

  const names = new Set(corpora.map((corpus) => corpus.name.trim().toLocaleLowerCase()));

  if (!names.has(normalizedBase.toLocaleLowerCase())) {
    return normalizedBase;
  }

  let suffix = 2;

  while (names.has(`${normalizedBase} ${suffix}`.toLocaleLowerCase())) {
    suffix += 1;
  }

  return `${normalizedBase} ${suffix}`;
}

function fileName(path: string): string {
  const segments = path.split(/[\\/]/);

  return segments[segments.length - 1] ?? path;
}

function uniqueDocumentSources(documents: CorpusDocument[]): string[] {
  const seen = new Set<string>();
  const paths: string[] = [];

  for (const document of documents) {
    const source = document.sourcePath?.trim();

    if (!source || seen.has(source)) {
      continue;
    }

    seen.add(source);
    paths.push(source);
  }

  return paths;
}

function extractSearchContext(text: string, query: string, radius = 180, fallbackLength = 420): string {
  if (text.length === 0) {
    return text;
  }

  const normalizedQuery = query.trim().toLocaleLowerCase();

  if (!normalizedQuery) {
    return text.length > fallbackLength ? `${text.slice(0, fallbackLength).trimEnd()}…` : text;
  }

  const terms = normalizedQuery
    .split(/\s+/)
    .filter((term) => term.length > 0)
    .slice(0, 8);

  if (terms.length === 0) {
    return text.length > fallbackLength ? `${text.slice(0, fallbackLength).trimEnd()}…` : text;
  }

  const lowerText = text.toLocaleLowerCase();
  let matchIndex = Number.POSITIVE_INFINITY;
  let matchLength = 0;

  for (const term of terms) {
    const index = lowerText.indexOf(term);

    if (index >= 0 && index < matchIndex) {
      matchIndex = index;
      matchLength = term.length;
    }
  }

  if (!Number.isFinite(matchIndex)) {
    return text.length > fallbackLength ? `${text.slice(0, fallbackLength).trimEnd()}…` : text;
  }

  const start = Math.max(0, Math.floor(matchIndex) - radius);
  const end = Math.min(text.length, Math.floor(matchIndex) + matchLength + radius);
  let snippet = text.slice(start, end).trim();

  if (start > 0) {
    snippet = `…${snippet}`;
  }

  if (end < text.length) {
    snippet = `${snippet}…`;
  }

  if (snippet.length > fallbackLength + 2) {
    snippet = `${snippet.slice(0, fallbackLength).trimEnd()}…`;
  }

  return snippet;
}

function renderHighlightedText(text: string, query: string, onTermClick?: (term: string) => void) {
  const terms = query
    .trim()
    .toLocaleLowerCase()
    .split(/\s+/)
    .filter((term) => term.length > 0)
    .slice(0, 8);

  if (terms.length === 0) {
    return text;
  }

  const matcher = new RegExp(`(${terms.map((term) => escapeRegExp(term)).join("|")})`, "gi");

  return text.split(matcher).map((part, index) => {
    if (!part) {
      return null;
    }

    const matched = terms.includes(part.toLocaleLowerCase());

    if (!matched) {
      return <span key={`${part}-${index}`}>{part}</span>;
    }

    if (onTermClick) {
      return (
        <button
          key={`${part}-${index}`}
          type="button"
          onClick={() => onTermClick(part)}
          className="rounded bg-accent/15 px-[1px] text-primary underline decoration-accent/40 decoration-dotted underline-offset-2 transition-colors hover:bg-accent/25"
        >
          {part}
        </button>
      );
    }

    return <mark key={`${part}-${index}`} className="rounded bg-accent/15 px-[1px] text-primary">{part}</mark>;
  });
}

function escapeRegExp(input: string): string {
  return input.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
