import type { Page } from "@playwright/test";

type InstallMockOptions = {
  language?: "en" | "pt-BR" | "es";
  workspace?: "chat" | "models" | "speech" | "embeddings" | "settings";
  onboarding?: boolean;
};

export async function installFoundryMock(page: Page, options: InstallMockOptions = {}) {
  await page.addInitScript((config: InstallMockOptions) => {
    const now = Date.now();
    const onboarding = Boolean(config.onboarding);
    const language = config.language ?? "en";
    const workspace = config.workspace ?? "chat";

    localStorage.setItem("foundryui.language", language);
    localStorage.setItem("foundryui.workspace", workspace);

    const baseModels = [
      {
        alias: "qwen3-0.6b",
        id: "qwen3-0.6b",
        displayName: "Qwen3 0.6B",
        family: "qwen3",
        kind: "chat",
        modelType: "chat",
        device: "cpu",
        fileSizeMb: 529,
        cached: !onboarding,
        loaded: !onboarding,
        supportsToolCalling: true,
        license: "MIT",
        publisher: "Microsoft",
        inputModalities: { value: ["text"], origin: "curated", source: "smoke" },
        outputModalities: { value: ["text"], origin: "curated", source: "smoke" },
        contextLength: { value: 8192, origin: "curated", source: "smoke" },
        embeddingDimensions: { value: null, origin: "notDeclared", source: null },
        quantization: { value: "q4", origin: "curated", source: "smoke" },
        overlaySource: "smoke",
        variants: [],
      },
      {
        alias: "phi-4-mini",
        id: "phi-4-mini",
        displayName: "Phi-4 Mini",
        family: "phi",
        kind: "chat",
        modelType: "chat",
        device: "cpu",
        fileSizeMb: 2048,
        cached: onboarding ? false : true,
        loaded: false,
        supportsToolCalling: true,
        license: "MIT",
        publisher: "Microsoft",
        inputModalities: { value: ["text"], origin: "curated", source: "smoke" },
        outputModalities: { value: ["text"], origin: "curated", source: "smoke" },
        contextLength: { value: 4096, origin: "curated", source: "smoke" },
        embeddingDimensions: { value: null, origin: "notDeclared", source: null },
        quantization: { value: "q4", origin: "curated", source: "smoke" },
        overlaySource: "smoke",
        variants: [],
      },
      {
        alias: "whisper-tiny",
        id: "whisper-tiny",
        displayName: "Whisper Tiny",
        family: "whisper",
        kind: "speech",
        modelType: "speech",
        device: "cpu",
        fileSizeMb: 131,
        cached: !onboarding,
        loaded: !onboarding,
        supportsToolCalling: false,
        license: "MIT",
        publisher: "OpenAI",
        inputModalities: { value: ["audio"], origin: "curated", source: "smoke" },
        outputModalities: { value: ["text"], origin: "curated", source: "smoke" },
        contextLength: { value: null, origin: "notDeclared", source: null },
        embeddingDimensions: { value: null, origin: "notDeclared", source: null },
        quantization: { value: "int8", origin: "curated", source: "smoke" },
        overlaySource: "smoke",
        variants: [],
      },
      {
        alias: "qwen3-embedding-0.6b",
        id: "qwen3-embedding-0.6b",
        displayName: "Qwen3 Embedding 0.6B",
        family: "qwen3",
        kind: "embedding",
        modelType: "embedding",
        device: "cpu",
        fileSizeMb: 515,
        cached: !onboarding,
        loaded: !onboarding,
        supportsToolCalling: false,
        license: "MIT",
        publisher: "Microsoft",
        inputModalities: { value: ["text"], origin: "curated", source: "smoke" },
        outputModalities: { value: ["embedding"], origin: "curated", source: "smoke" },
        contextLength: { value: 8192, origin: "curated", source: "smoke" },
        embeddingDimensions: { value: 1024, origin: "curated", source: "smoke" },
        quantization: { value: "q8", origin: "curated", source: "smoke" },
        overlaySource: "smoke",
        variants: [],
      },
    ];

    const state = {
      models: baseModels,
      server: {
        endpoint: "http://127.0.0.1:5273",
        running: true,
        state: "running",
        pid: 43210,
        startedByApp: true,
        logFile: "/tmp/foundry-local.log",
        port: 5273,
        uptime: "00:05:00",
      },
      serverPreferences: {
        port: null,
        keepRunning: false,
        connectTimeoutSecs: 5,
        firstTokenTimeoutSecs: 120,
        tokenIdleTimeoutSecs: 60,
      },
      conversations: [] as Array<{
        id: number;
        title: string;
        modelAlias: string;
        createdAt: number;
        updatedAt: number;
        favorite: boolean;
        archived: boolean;
      }>,
      messages: {} as Record<number, Array<{
        id: number;
        conversationId: number;
        seq: number;
        role: "user" | "assistant";
        content: string;
        reasoning: string;
        modelAlias: string | null;
        params: null;
        usage: { promptTokens: number; completionTokens: number; totalTokens: number } | null;
        error: null;
        status: "complete" | "streaming";
        attachments: [];
        createdAt: number;
        updatedAt: number;
      }>>,
      nextConversationId: 1,
      nextMessageId: 1,
      transcriptions: [] as Array<{
        id: number;
        modelAlias: string;
        sourcePath: string;
        language: string | null;
        params: {
          requestedLanguage: string | null;
          temperature: number;
        } | null;
        text: string;
        durationMs: number;
        audioExists: boolean;
        createdAt: number;
        favorite: boolean;
        archived: boolean;
      }>,
      nextTranscriptionId: 1,
      corpora: [] as Array<{
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
      }>,
      corpusDocuments: {} as Record<number, Array<{
        id: number;
        corpusId: number;
        position: number;
        text: string;
        sourcePath: string | null;
        createdAt: number;
      }>>,
      nextCorpusId: 1,
      nextDocumentId: 1,
      jobs: [] as Array<{
        alias: string;
        status: "running" | "succeeded" | "failed" | "cancelled";
        receivedBytes: number;
        totalBytes: number;
        bytesPerSecond: number;
        etaSeconds: number | null;
        startedAt: number;
        updatedAt: number;
        error: null;
      }>,
    };

    const callbacks = new Map<number, (...args: unknown[]) => void>();
    const listeners = new Map<number, { event: string; callbackId: number }>();
    let callbackId = 1;
    let listenerId = 1;

    function deepClone<T>(value: T): T {
      return JSON.parse(JSON.stringify(value)) as T;
    }

    function parseChannelId(value: unknown): number | null {
      if (typeof value !== "string") {
        return null;
      }

      if (!value.startsWith("__CHANNEL__:")) {
        return null;
      }

      const parsed = Number(value.slice("__CHANNEL__:".length));

      return Number.isFinite(parsed) ? parsed : null;
    }

    function emitChannelMessage(channelValue: unknown, messages: unknown[]) {
      const id = parseChannelId(channelValue);

      if (id === null) {
        return;
      }

      const callback = callbacks.get(id);

      if (!callback) {
        return;
      }

      for (let index = 0; index < messages.length; index += 1) {
        callback({ index, message: messages[index] });
      }

      callback({ index: messages.length, end: true });
    }

    function loadedSummary() {
      const loaded = state.models
        .filter((model) => model.loaded)
        .map((model) => ({
          alias: model.alias,
          id: model.id,
          displayName: model.displayName,
          modelType: model.modelType,
          device: model.device,
          fileSizeMb: model.fileSizeMb,
          loadedSince: now - 10_000,
        }));

      return {
        models: loaded,
        totalBytes: loaded.reduce((total, model) => total + ((model.fileSizeMb ?? 0) * 1024 * 1024), 0),
        availableMemoryBytes: 16 * 1024 * 1024 * 1024,
        totalMemoryBytes: 18 * 1024 * 1024 * 1024,
        serverRunning: state.server.running,
      };
    }

    function findConversation(id: number) {
      return state.conversations.find((conversation) => conversation.id === id) ?? null;
    }

    function fileName(path: string) {
      const segments = path.split(/[\\/]/);
      return segments[segments.length - 1] ?? path;
    }

    function createConversation(modelAlias: string) {
      const conversation = {
        id: state.nextConversationId,
        title: "",
        modelAlias,
        createdAt: Date.now(),
        updatedAt: Date.now(),
        favorite: false,
        archived: false,
      };

      state.nextConversationId += 1;
      state.conversations.unshift(conversation);
      state.messages[conversation.id] = [];

      return conversation;
    }

    function normalizeConversations() {
      state.conversations.sort(
        (left, right) =>
          Number(right.favorite) - Number(left.favorite) ||
          right.updatedAt - left.updatedAt ||
          right.id - left.id,
      );
    }

    function updateCorpus(corpusId: number, updater: (current: (typeof state.corpora)[number]) => void) {
      const corpus = state.corpora.find((entry) => entry.id === corpusId);

      if (!corpus) {
        return null;
      }

      updater(corpus);
      corpus.updatedAt = Date.now();

      return corpus;
    }

    const internals = {
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main" },
      },
      transformCallback(callback: (...args: unknown[]) => void) {
        const id = callbackId;
        callbackId += 1;
        callbacks.set(id, callback);
        return id;
      },
      unregisterCallback(id: number) {
        callbacks.delete(id);
      },
      convertFileSrc(path: string) {
        return `asset://${path}`;
      },
      async invoke(command: string, payload: Record<string, unknown> = {}) {
        if (command === "cli_detect") {
          return {
            path: "/opt/homebrew/bin/foundry",
            version: "0.2.0",
            minimumVersion: "0.1.0",
          };
        }

        if (command === "server_start") {
          state.server.running = true;
          return deepClone(state.server);
        }

        if (command === "server_set_preferences") {
          state.serverPreferences = {
            ...state.serverPreferences,
            ...(payload.preferences as typeof state.serverPreferences),
          };
          return deepClone(state.serverPreferences);
        }

        if (command === "catalog_list") {
          return deepClone(state.models);
        }

        if (command === "catalog_model") {
          const alias = String(payload.alias ?? "");
          const model = state.models.find((entry) => entry.alias === alias);

          if (!model) {
            throw {
              code: "CommandFailed",
              messageKey: "errors.CommandFailed",
              message: `Unknown model alias: ${alias}`,
              detail: null,
            };
          }

          return deepClone(model);
        }

        if (command === "loaded_models") {
          return deepClone(loadedSummary());
        }

        if (command === "model_load") {
          const alias = String(payload.alias ?? "");
          const model = state.models.find((entry) => entry.alias === alias);

          if (!model || !model.cached) {
            throw {
              code: "ModelNotCached",
              messageKey: "errors.ModelNotCached",
              message: "This model is not downloaded yet.",
              detail: null,
            };
          }

          model.loaded = true;
          return deepClone(loadedSummary());
        }

        if (command === "model_unload") {
          const alias = String(payload.alias ?? "");
          const model = state.models.find((entry) => entry.alias === alias);

          if (model) {
            model.loaded = false;
          }

          return deepClone(loadedSummary());
        }

        if (command === "model_jobs") {
          return deepClone(state.jobs);
        }

        if (command === "model_download") {
          const alias = String(payload.alias ?? "");
          const model = state.models.find((entry) => entry.alias === alias);

          if (model) {
            model.cached = true;
          }

          const job = {
            alias,
            status: "succeeded" as const,
            receivedBytes: (model?.fileSizeMb ?? 256) * 1024 * 1024,
            totalBytes: (model?.fileSizeMb ?? 256) * 1024 * 1024,
            bytesPerSecond: 0,
            etaSeconds: 0,
            startedAt: Date.now() - 2_000,
            updatedAt: Date.now(),
            error: null,
          };

          state.jobs = state.jobs.filter((entry) => entry.alias !== alias);
          state.jobs.push(job);

          return deepClone(job);
        }

        if (command === "model_download_cancel") {
          return true;
        }

        if (command === "chat_list_conversations") {
          const archived = Boolean(payload.archived);
          return deepClone(state.conversations.filter((conversation) => conversation.archived === archived));
        }

        if (command === "chat_create_conversation") {
          const modelAlias = String(payload.modelAlias ?? "qwen3-0.6b");
          return deepClone(createConversation(modelAlias));
        }

        if (command === "chat_set_conversation_favorite") {
          const id = Number(payload.id);
          const favorite = Boolean(payload.favorite);
          const conversation = findConversation(id);

          if (!conversation) {
            throw {
              code: "CommandFailed",
              messageKey: "errors.CommandFailed",
              message: `Conversation ${id} not found`,
              detail: null,
            };
          }

          conversation.favorite = favorite;
          conversation.updatedAt = Date.now();
          normalizeConversations();

          return deepClone(conversation);
        }

        if (command === "chat_set_conversation_archived") {
          const id = Number(payload.id);
          const archived = Boolean(payload.archived);
          const conversation = findConversation(id);

          if (!conversation) {
            throw {
              code: "CommandFailed",
              messageKey: "errors.CommandFailed",
              message: `Conversation ${id} not found`,
              detail: null,
            };
          }

          conversation.archived = archived;
          conversation.updatedAt = Date.now();

          return deepClone(conversation);
        }

        if (command === "chat_list_messages") {
          const conversationId = Number(payload.conversationId);
          const beforeSeq = payload.beforeSeq === null || payload.beforeSeq === undefined
            ? null
            : Number(payload.beforeSeq);
          const limit = payload.limit === null || payload.limit === undefined ? 40 : Number(payload.limit);
          const items = [...(state.messages[conversationId] ?? [])].sort((left, right) => left.seq - right.seq);
          const filtered = beforeSeq === null ? items : items.filter((message) => message.seq < beforeSeq);
          const slice = filtered.slice(Math.max(0, filtered.length - limit));

          return {
            messages: deepClone(slice),
            hasMore: filtered.length > slice.length,
          };
        }

        if (command === "chat_conversation_usage") {
          const conversationId = Number(payload.conversationId);
          const items = state.messages[conversationId] ?? [];
          const userChars = items
            .filter((message) => message.role === "user")
            .reduce((total, message) => total + message.content.length, 0);
          const assistantChars = items
            .filter((message) => message.role === "assistant")
            .reduce((total, message) => total + message.content.length, 0);

          return {
            promptTokens: Math.max(1, Math.ceil(userChars / 4)),
            completionTokens: Math.max(1, Math.ceil(assistantChars / 4)),
            totalTokens: Math.max(2, Math.ceil((userChars + assistantChars) / 4)),
          };
        }

        if (command === "chat_send_message") {
          const conversationId = Number(payload.conversationId);
          const conversation = findConversation(conversationId);

          if (!conversation) {
            throw {
              code: "CommandFailed",
              messageKey: "errors.CommandFailed",
              message: `Conversation ${conversationId} not found`,
              detail: null,
            };
          }

          const previous = state.messages[conversationId] ?? [];
          const nextSeq = previous.length ? previous[previous.length - 1]?.seq ?? 0 : 0;
          const content = String(payload.content ?? "").trim();
          const requestId = `req-${Date.now()}`;
          const userMessage = {
            id: state.nextMessageId,
            conversationId,
            seq: nextSeq + 1,
            role: "user" as const,
            content,
            reasoning: "",
            modelAlias: null,
            params: null,
            usage: null,
            error: null,
            status: "complete" as const,
            attachments: [] as [],
            createdAt: Date.now(),
            updatedAt: Date.now(),
          };
          state.nextMessageId += 1;

          const replyText = `Mock reply to: ${content}`;
          const assistantStreaming = {
            id: state.nextMessageId,
            conversationId,
            seq: nextSeq + 2,
            role: "assistant" as const,
            content: "",
            reasoning: "",
            modelAlias: conversation.modelAlias,
            params: null,
            usage: null,
            error: null,
            status: "streaming" as const,
            attachments: [] as [],
            createdAt: Date.now(),
            updatedAt: Date.now(),
          };
          state.nextMessageId += 1;

          const assistantDone = {
            ...assistantStreaming,
            content: replyText,
            usage: {
              promptTokens: Math.max(1, Math.ceil(content.length / 4)),
              completionTokens: Math.max(1, Math.ceil(replyText.length / 4)),
              totalTokens: Math.max(2, Math.ceil((content.length + replyText.length) / 4)),
            },
            status: "complete" as const,
            updatedAt: Date.now(),
          };

          conversation.updatedAt = Date.now();
          state.messages[conversationId] = [...previous, userMessage, assistantDone];
          normalizeConversations();

          emitChannelMessage(payload.channel, [
            {
              type: "started",
              requestId,
              conversation: deepClone(conversation),
              userMessage: deepClone(userMessage),
              assistantMessage: deepClone(assistantStreaming),
            },
            {
              type: "delta",
              requestId,
              messageId: assistantStreaming.id,
              content: "Mock reply to: ",
              reasoning: "",
            },
            {
              type: "delta",
              requestId,
              messageId: assistantStreaming.id,
              content,
              reasoning: "",
            },
            {
              type: "usage",
              requestId,
              messageId: assistantStreaming.id,
              usage: deepClone(assistantDone.usage),
            },
            {
              type: "done",
              requestId,
              message: deepClone(assistantDone),
              conversation: deepClone(conversation),
              cancelled: false,
            },
          ]);

          return {
            requestId,
            conversation: deepClone(conversation),
            userMessage: deepClone(userMessage),
            assistantMessage: deepClone(assistantDone),
            removed: [],
          };
        }

        if (command === "chat_cancel_generation") {
          return true;
        }

        if (command === "chat_search") {
          return [];
        }

        if (command === "speech_get_storage_settings") {
          return {
            directory: "/tmp/foundryui/speech",
            defaultDirectory: "/tmp/foundryui/speech",
            custom: false,
          };
        }

        if (command === "speech_set_storage_settings") {
          const directory = String(payload.directory ?? "/tmp/foundryui/speech");
          return {
            directory,
            defaultDirectory: "/tmp/foundryui/speech",
            custom: directory !== "/tmp/foundryui/speech",
          };
        }

        if (command === "speech_open_storage_folder") {
          return null;
        }

        if (command === "speech_list_input_devices") {
          return [{ id: "default", name: "Default microphone", isDefault: true }];
        }

        if (command === "speech_microphone_permission") {
          return "granted";
        }

        if (command === "speech_open_microphone_settings") {
          return null;
        }

        if (command === "speech_import") {
          const path = String(payload.path ?? "/tmp/audio-sample.mp3");
          return {
            path,
            name: fileName(path),
            mime: "audio/mpeg",
            bytes: 48_000,
            durationMs: 15_000,
          };
        }

        if (command === "speech_audio_data") {
          return "data:audio/wav;base64,UklGRiQAAABXQVZFZm10IBAAAAABAAEA";
        }

        if (command === "speech_record_start") {
          return Date.now();
        }

        if (command === "speech_record_level") {
          return 0.35;
        }

        if (command === "speech_record_stop") {
          return {
            path: "/tmp/recorded-sample.wav",
            name: "recorded-sample.wav",
            mime: "audio/wav",
            bytes: 32_000,
            durationMs: 8_000,
          };
        }

        if (command === "speech_record_cancel") {
          return null;
        }

        if (command === "speech_discard_audio") {
          return null;
        }

        if (command === "speech_list_transcriptions") {
          const archived = Boolean(payload.archived);
          return deepClone(state.transcriptions.filter((item) => item.archived === archived));
        }

        if (command === "speech_transcribe") {
          const modelAlias = String(payload.modelAlias ?? "whisper-tiny");
          const model = state.models.find((entry) => entry.alias === modelAlias);
          const autoLoaded = model ? !model.loaded : false;

          if (model) {
            model.loaded = true;
          }

          const path = String(payload.path ?? "/tmp/audio-sample.mp3");
          const transcription = {
            id: state.nextTranscriptionId,
            modelAlias,
            sourcePath: path,
            language: (payload.language as string | null | undefined) ?? "en",
            params: {
              requestedLanguage: (payload.language as string | null | undefined) ?? null,
              temperature: Number(payload.temperature ?? 0),
            },
            text: `Mock transcription from ${fileName(path)}`,
            durationMs: 15_000,
            audioExists: true,
            createdAt: Date.now(),
            favorite: false,
            archived: false,
          };

          state.nextTranscriptionId += 1;
          state.transcriptions = [transcription, ...state.transcriptions.filter((entry) => entry.id !== transcription.id)];

          return {
            transcription: deepClone(transcription),
            autoLoaded,
          };
        }

        if (command === "speech_set_transcription_favorite") {
          const id = Number(payload.id);
          const favorite = Boolean(payload.favorite);
          const item = state.transcriptions.find((entry) => entry.id === id);

          if (!item) {
            throw {
              code: "CommandFailed",
              messageKey: "errors.CommandFailed",
              message: `Transcription ${id} not found`,
              detail: null,
            };
          }

          item.favorite = favorite;
          return deepClone(item);
        }

        if (command === "speech_set_transcription_archived") {
          const id = Number(payload.id);
          const archived = Boolean(payload.archived);
          const item = state.transcriptions.find((entry) => entry.id === id);

          if (!item) {
            throw {
              code: "CommandFailed",
              messageKey: "errors.CommandFailed",
              message: `Transcription ${id} not found`,
              detail: null,
            };
          }

          item.archived = archived;
          return deepClone(item);
        }

        if (command === "speech_update_transcription") {
          const id = Number(payload.id);
          const text = String(payload.text ?? "");
          const item = state.transcriptions.find((entry) => entry.id === id);

          if (!item) {
            throw {
              code: "CommandFailed",
              messageKey: "errors.CommandFailed",
              message: `Transcription ${id} not found`,
              detail: null,
            };
          }

          item.text = text;
          return deepClone(item);
        }

        if (command === "speech_delete_transcription") {
          const id = Number(payload.id);
          state.transcriptions = state.transcriptions.filter((entry) => entry.id !== id);
          return { removed: true, transcription: null };
        }

        if (command === "speech_save_text") {
          const id = Number(payload.id);
          return { path: `/tmp/transcription-${id}.txt`, bytes: 100 };
        }

        if (command === "speech_send_to_chat") {
          const text = String(payload.text ?? "").trim();
          const conversationId = payload.conversationId === null || payload.conversationId === undefined
            ? null
            : Number(payload.conversationId);
          const modelAlias = String(payload.modelAlias ?? "qwen3-0.6b");
          const conversation = conversationId === null
            ? createConversation(modelAlias)
            : (findConversation(conversationId) ?? createConversation(modelAlias));
          const previous = state.messages[conversation.id] ?? [];
          const nextSeq = previous.length ? previous[previous.length - 1]?.seq ?? 0 : 0;
          const message = {
            id: state.nextMessageId,
            conversationId: conversation.id,
            seq: nextSeq + 1,
            role: "user" as const,
            content: text,
            reasoning: "",
            modelAlias: null,
            params: null,
            usage: null,
            error: null,
            status: "complete" as const,
            attachments: [] as [],
            createdAt: Date.now(),
            updatedAt: Date.now(),
          };
          state.nextMessageId += 1;
          conversation.updatedAt = Date.now();
          state.messages[conversation.id] = [...previous, message];
          normalizeConversations();

          return deepClone(conversation);
        }

        if (command === "embeddings_list_corpora") {
          const archived = Boolean(payload.archived);
          return deepClone(
            state.corpora
              .filter((entry) => entry.archived === archived)
              .sort((left, right) => right.updatedAt - left.updatedAt || right.id - left.id),
          );
        }

        if (command === "embeddings_create_corpus") {
          const corpusId = state.nextCorpusId;
          state.nextCorpusId += 1;

          const documents = Array.isArray(payload.documents)
            ? payload.documents.map((value) => String(value))
            : [];
          const sources = Array.isArray(payload.documentSources)
            ? payload.documentSources.map((value) => (value === null ? null : String(value)))
            : [];

          const corpus = {
            id: corpusId,
            name: String(payload.name ?? `Corpus ${corpusId}`),
            modelAlias: String(payload.modelAlias ?? "qwen3-embedding-0.6b"),
            dimensions: Number(payload.dimensions ?? 1024),
            documentCount: documents.length,
            vectorCount: 0,
            createdAt: Date.now(),
            updatedAt: Date.now(),
            favorite: false,
            archived: false,
          };

          state.corpora.unshift(corpus);
          state.corpusDocuments[corpusId] = documents.map((text, index) => ({
            id: state.nextDocumentId + index,
            corpusId,
            position: index + 1,
            text,
            sourcePath: sources[index] ?? null,
            createdAt: Date.now(),
          }));
          state.nextDocumentId += documents.length;

          return deepClone(corpus);
        }

        if (command === "embeddings_get_corpus") {
          const corpusId = Number(payload.corpusId);
          const corpus = state.corpora.find((entry) => entry.id === corpusId);

          if (!corpus) {
            throw {
              code: "CommandFailed",
              messageKey: "errors.CommandFailed",
              message: `Corpus ${corpusId} not found`,
              detail: null,
            };
          }

          return {
            corpus: deepClone(corpus),
            documents: deepClone(state.corpusDocuments[corpusId] ?? []),
          };
        }

        if (command === "embeddings_rename_corpus") {
          const corpusId = Number(payload.corpusId);
          const corpus = updateCorpus(corpusId, (current) => {
            current.name = String(payload.name ?? current.name);
          });

          if (!corpus) {
            throw {
              code: "CommandFailed",
              messageKey: "errors.CommandFailed",
              message: `Corpus ${corpusId} not found`,
              detail: null,
            };
          }

          return deepClone(corpus);
        }

        if (command === "embeddings_set_model") {
          const corpusId = Number(payload.corpusId);
          const corpus = updateCorpus(corpusId, (current) => {
            current.modelAlias = String(payload.modelAlias ?? current.modelAlias);
            current.dimensions = Number(payload.dimensions ?? current.dimensions);
            current.vectorCount = 0;
          });

          if (!corpus) {
            throw {
              code: "CommandFailed",
              messageKey: "errors.CommandFailed",
              message: `Corpus ${corpusId} not found`,
              detail: null,
            };
          }

          return deepClone(corpus);
        }

        if (command === "embeddings_replace_documents") {
          const corpusId = Number(payload.corpusId);
          const corpus = state.corpora.find((entry) => entry.id === corpusId);

          if (!corpus) {
            throw {
              code: "CommandFailed",
              messageKey: "errors.CommandFailed",
              message: `Corpus ${corpusId} not found`,
              detail: null,
            };
          }

          const documents = Array.isArray(payload.documents)
            ? payload.documents.map((value) => String(value))
            : [];
          const sources = Array.isArray(payload.documentSources)
            ? payload.documentSources.map((value) => (value === null ? null : String(value)))
            : [];

          state.corpusDocuments[corpusId] = documents.map((text, index) => ({
            id: state.nextDocumentId + index,
            corpusId,
            position: index + 1,
            text,
            sourcePath: sources[index] ?? null,
            createdAt: Date.now(),
          }));
          state.nextDocumentId += documents.length;
          corpus.documentCount = documents.length;
          corpus.vectorCount = 0;
          corpus.updatedAt = Date.now();

          return deepClone(corpus);
        }

        if (command === "embeddings_import_lines") {
          const path = String(payload.path ?? "/tmp/corpus-one.txt");
          const name = fileName(path);

          return [`Document extracted from ${name}`, `Second paragraph from ${name}`];
        }

        if (command === "embeddings_vectorize") {
          const corpusId = Number(payload.corpusId);
          const corpus = state.corpora.find((entry) => entry.id === corpusId);
          const docs = state.corpusDocuments[corpusId] ?? [];

          if (!corpus) {
            throw {
              code: "CommandFailed",
              messageKey: "errors.CommandFailed",
              message: `Corpus ${corpusId} not found`,
              detail: null,
            };
          }

          const requestId = `vec-${Date.now()}`;
          emitChannelMessage(payload.channel, [
            {
              type: "started",
              requestId,
              corpusId,
              totalDocuments: docs.length,
            },
            ...docs.map((document, index) => ({
              type: "progress",
              requestId,
              corpusId,
              completed: index + 1,
              totalDocuments: docs.length,
              documentId: document.id,
              usage: {
                promptTokens: (index + 1) * 4,
                totalTokens: (index + 1) * 4,
              },
            })),
          ]);

          corpus.vectorCount = docs.length;
          corpus.updatedAt = Date.now();

          return {
            requestId,
            corpus: deepClone(corpus),
            usage: {
              promptTokens: Math.max(1, docs.length * 4),
              totalTokens: Math.max(1, docs.length * 4),
            },
            completed: docs.length,
            totalDocuments: docs.length,
            cancelled: false,
          };
        }

        if (command === "embeddings_cancel_vectorize") {
          return true;
        }

        if (command === "embeddings_search") {
          const corpusId = Number(payload.corpusId);
          const query = String(payload.query ?? "").toLocaleLowerCase();
          const topK = Number(payload.topK ?? 5);
          const docs = state.corpusDocuments[corpusId] ?? [];
          const hits = docs
            .map((document, index) => ({
              documentId: document.id,
              position: document.position,
              text: document.text,
              sourcePath: document.sourcePath,
              sourceExists: true,
              score: document.text.toLocaleLowerCase().includes(query) ? 0.97 - index * 0.01 : 0.75 - index * 0.01,
            }))
            .slice(0, Math.max(1, topK));

          return {
            usage: {
              promptTokens: Math.max(1, Math.ceil(query.length / 4)),
              totalTokens: Math.max(1, Math.ceil(query.length / 4)),
            },
            hits: deepClone(hits),
          };
        }

        if (command === "embeddings_answer") {
          const search = await internals.invoke("embeddings_search", payload);
          return {
            answer: "Mock semantic answer for smoke test.",
            modelAlias: "qwen3-0.6b",
            modelId: "qwen3-0.6b",
            autoLoadedModel: false,
            search,
            citations: (search as { hits: unknown[] }).hits,
            usage: {
              promptTokens: 12,
              completionTokens: 18,
              totalTokens: 30,
            },
          };
        }

        if (command === "embeddings_compare") {
          return {
            usage: {
              promptTokens: 10,
              totalTokens: 10,
            },
            score: 0.84,
          };
        }

        if (command === "embeddings_set_corpus_favorite") {
          const corpusId = Number(payload.corpusId);
          const favorite = Boolean(payload.favorite);
          const corpus = updateCorpus(corpusId, (current) => {
            current.favorite = favorite;
          });

          if (!corpus) {
            throw {
              code: "CommandFailed",
              messageKey: "errors.CommandFailed",
              message: `Corpus ${corpusId} not found`,
              detail: null,
            };
          }

          return deepClone(corpus);
        }

        if (command === "embeddings_set_corpus_archived") {
          const corpusId = Number(payload.corpusId);
          const archived = Boolean(payload.archived);
          const corpus = updateCorpus(corpusId, (current) => {
            current.archived = archived;
          });

          if (!corpus) {
            throw {
              code: "CommandFailed",
              messageKey: "errors.CommandFailed",
              message: `Corpus ${corpusId} not found`,
              detail: null,
            };
          }

          return deepClone(corpus);
        }

        if (command === "embeddings_delete_corpus") {
          const corpusId = Number(payload.corpusId);
          state.corpora = state.corpora.filter((entry) => entry.id !== corpusId);
          delete state.corpusDocuments[corpusId];
          return null;
        }

        if (command === "embeddings_reveal_source") {
          return true;
        }

        if (command === "plugin:event|listen") {
          const id = listenerId;
          listenerId += 1;
          listeners.set(id, {
            event: String(payload.event ?? ""),
            callbackId: Number(payload.handler),
          });
          return id;
        }

        if (command === "plugin:event|unlisten") {
          listeners.delete(Number(payload.eventId));
          return true;
        }

        if (command === "plugin:event|emit" || command === "plugin:event|emit_to") {
          return null;
        }

        if (command === "plugin:dialog|open") {
          const options = (payload.options ?? {}) as {
            multiple?: boolean;
            filters?: Array<{ extensions?: string[] }>;
          };
          const extensions = options.filters?.[0]?.extensions ?? [];
          const isAudio = extensions.some((entry) => ["mp3", "wav", "flac", "ogg", "webm"].includes(entry));

          if (options.multiple) {
            return ["/tmp/corpus-one.txt"];
          }

          return isAudio ? "/tmp/audio-sample.mp3" : "/tmp/corpus-one.txt";
        }

        if (command === "plugin:dialog|save") {
          return "/tmp/mock-output.txt";
        }

        if (
          command === "plugin:opener|open_url" ||
          command === "plugin:opener|open_path" ||
          command === "plugin:opener|reveal_item_in_dir" ||
          command.startsWith("plugin:window|") ||
          command.startsWith("plugin:webview|")
        ) {
          return null;
        }

        throw {
          code: "CommandFailed",
          messageKey: "errors.CommandFailed",
          message: `Smoke mock missing command: ${command}`,
          detail: null,
        };
      },
    };

    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = internals;
    (window as unknown as Record<string, unknown>).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener() {
        return undefined;
      },
    };
  }, options);
}
