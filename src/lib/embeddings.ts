import { Channel, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

export type EmbeddingCorpus = {
  id: number;
  name: string;
  modelAlias: string;
  dimensions: number;
  documentCount: number;
  vectorCount: number;
  createdAt: number;
  updatedAt: number;
  favorite: boolean;
  archived: boolean;
};

export type CorpusDocument = {
  id: number;
  corpusId: number;
  position: number;
  text: string;
  sourcePath: string | null;
  createdAt: number;
};

export type EmbeddingCorpusDetails = {
  corpus: EmbeddingCorpus;
  documents: CorpusDocument[];
};

export type EmbeddingUsage = {
  promptTokens: number;
  totalTokens: number;
};

export type VectorizeEvent =
  | {
      type: "started";
      requestId: string;
      corpusId: number;
      totalDocuments: number;
    }
  | {
      type: "progress";
      requestId: string;
      corpusId: number;
      completed: number;
      totalDocuments: number;
      documentId: number;
      usage: EmbeddingUsage;
    };

export type VectorizeReceipt = {
  requestId: string;
  corpus: EmbeddingCorpus;
  usage: EmbeddingUsage;
  completed: number;
  totalDocuments: number;
  cancelled: boolean;
};

export type EmbeddingSearchHit = {
  documentId: number;
  position: number;
  text: string;
  sourcePath: string | null;
  sourceExists: boolean;
  score: number;
};

export type EmbeddingSearchResult = {
  usage: EmbeddingUsage;
  hits: EmbeddingSearchHit[];
};

export type EmbeddingCompareResult = {
  usage: EmbeddingUsage;
  score: number;
};

export type EmbeddingAnswerResult = {
  answer: string;
  modelAlias: string;
  modelId: string;
  autoLoadedModel: boolean;
  search: EmbeddingSearchResult;
  citations: EmbeddingSearchHit[];
  usage: {
    promptTokens: number;
    completionTokens: number;
    totalTokens: number;
  };
};

export async function pickCorpusFile(): Promise<string | null> {
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [
      {
        name: "Text/PDF",
        extensions: ["txt", "md", "pdf"],
      },
    ],
  });

  return typeof selected === "string" ? selected : null;
}

export async function pickCorpusFiles(): Promise<string[]> {
  const selected = await open({
    multiple: true,
    directory: false,
    filters: [
      {
        name: "Text/PDF",
        extensions: ["txt", "md", "pdf"],
      },
    ],
  });

  return Array.isArray(selected) ? selected.filter((value): value is string => typeof value === "string") : [];
}

export async function importCorpusLines(path: string): Promise<string[]> {
  return invoke("embeddings_import_lines", { path });
}

export async function listCorpora(archived = false): Promise<EmbeddingCorpus[]> {
  return invoke("embeddings_list_corpora", { archived });
}

export async function setCorpusFavorite(
  corpusId: number,
  favorite: boolean,
): Promise<EmbeddingCorpus> {
  return invoke("embeddings_set_corpus_favorite", { corpusId, favorite });
}

export async function setCorpusArchived(
  corpusId: number,
  archived: boolean,
): Promise<EmbeddingCorpus> {
  return invoke("embeddings_set_corpus_archived", { corpusId, archived });
}

export async function getCorpus(corpusId: number): Promise<EmbeddingCorpusDetails> {
  return invoke("embeddings_get_corpus", { corpusId });
}

export async function createCorpus(input: {
  name: string;
  modelAlias: string;
  dimensions?: number;
  documents?: string[];
  documentSources?: Array<string | null>;
}): Promise<EmbeddingCorpus> {
  return invoke("embeddings_create_corpus", {
    name: input.name,
    modelAlias: input.modelAlias,
    dimensions: input.dimensions ?? null,
    documents: input.documents ?? [],
    documentSources: input.documentSources ?? null,
  });
}

export async function renameCorpus(corpusId: number, name: string): Promise<EmbeddingCorpus> {
  return invoke("embeddings_rename_corpus", { corpusId, name });
}

export async function replaceCorpusDocuments(
  corpusId: number,
  documents: string[],
  documentSources?: Array<string | null>,
): Promise<EmbeddingCorpus> {
  return invoke("embeddings_replace_documents", {
    corpusId,
    documents,
    documentSources: documentSources ?? null,
  });
}

export async function setCorpusModel(input: {
  corpusId: number;
  modelAlias: string;
  dimensions?: number;
}): Promise<EmbeddingCorpus> {
  return invoke("embeddings_set_model", {
    corpusId: input.corpusId,
    modelAlias: input.modelAlias,
    dimensions: input.dimensions ?? null,
  });
}

export async function deleteCorpus(corpusId: number): Promise<void> {
  return invoke("embeddings_delete_corpus", { corpusId });
}

export async function vectorizeCorpus(
  corpusId: number,
  onEvent: (event: VectorizeEvent) => void,
): Promise<VectorizeReceipt> {
  const channel = new Channel<VectorizeEvent>((event) => onEvent(event));

  return invoke("embeddings_vectorize", { corpusId, channel });
}

export async function cancelVectorize(requestId: string): Promise<boolean> {
  return invoke("embeddings_cancel_vectorize", { requestId });
}

export async function revealCorpusSource(path: string): Promise<boolean> {
  return invoke("embeddings_reveal_source", { path });
}

export async function searchCorpus(input: {
  corpusId: number;
  query: string;
  topK?: number;
}): Promise<EmbeddingSearchResult> {
  return invoke("embeddings_search", {
    corpusId: input.corpusId,
    query: input.query,
    topK: input.topK ?? null,
  });
}

export async function answerCorpus(input: {
  corpusId: number;
  query: string;
  topK?: number;
}): Promise<EmbeddingAnswerResult> {
  return invoke("embeddings_answer", {
    corpusId: input.corpusId,
    query: input.query,
    topK: input.topK ?? null,
  });
}

export async function compareTexts(input: {
  modelAlias: string;
  left: string;
  right: string;
}): Promise<EmbeddingCompareResult> {
  return invoke("embeddings_compare", input);
}

export function parsePastedDocuments(input: string): string[] {
  return input
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
}
