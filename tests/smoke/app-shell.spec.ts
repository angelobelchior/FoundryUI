import { expect, test } from "@playwright/test";

test("carrega o shell com mocks estáveis", async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem("foundryui.language", "en");
    localStorage.setItem("foundryui.workspace", "chat");

    const state = {
      models: [
        {
          alias: "phi-4-mini",
          id: "phi-4-mini",
          displayName: "Phi-4 Mini",
          family: "phi",
          kind: "chat",
          modelType: "chat",
          device: "cpu",
          fileSizeMb: 2048,
          cached: true,
          loaded: true,
          supportsToolCalling: true,
          license: "MIT",
          publisher: "Microsoft",
          inputModalities: { value: ["text"], origin: "curated", source: "smoke" },
          outputModalities: { value: ["text"], origin: "curated", source: "smoke" },
          contextLength: { value: 4096, origin: "curated", source: "smoke" },
          embeddingDimensions: { value: null, origin: "notDeclared", source: null },
          quantization: { value: null, origin: "notDeclared", source: null },
          overlaySource: "smoke",
          variants: [],
        },
      ],
      conversations: [] as Array<{
        id: number;
        title: string;
        modelAlias: string;
        createdAt: number;
        updatedAt: number;
        favorite: boolean;
        archived: boolean;
      }>,
    };

    let callbackId = 1;
    let eventId = 1;
    const callbacks = new Map<number, (...args: unknown[]) => void>();

    function loadedSummary() {
      return {
        models: state.models
          .filter((model) => model.loaded)
          .map((model) => ({
            alias: model.alias,
            id: model.id,
            displayName: model.displayName,
            modelType: model.modelType,
            device: model.device,
            fileSizeMb: model.fileSizeMb,
            loadedSince: Date.now() - 5_000,
          })),
        totalBytes: 2_147_483_648,
        availableMemoryBytes: 16_106_127_360,
        totalMemoryBytes: 17_179_869_184,
        serverRunning: true,
      };
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

        if (command === "catalog_list") {
          return state.models;
        }

        if (command === "workspace_snapshot") {
          return {
            server: {
              endpoint: "http://127.0.0.1:5273",
              running: true,
              state: "ready",
              pid: 43210,
              startedByApp: true,
              logFile: "/tmp/foundry-local.log",
              port: 5273,
              uptime: "00:05:00",
            },
            loaded: loadedSummary(),
            catalog: state.models,
          };
        }

        if (command === "catalog_model") {
          const alias = payload.alias;
          const model = state.models.find((entry) => entry.alias === alias);

          if (model) {
            return model;
          }

          throw {
            code: "CommandFailed",
            messageKey: "errors.CommandFailed",
            message: `Unknown model alias: ${String(alias)}`,
            detail: null,
          };
        }

        if (command === "loaded_models") {
          return loadedSummary();
        }

        if (command === "chat_list_conversations") {
          const archived = Boolean(payload.archived);
          return state.conversations.filter((conversation) => conversation.archived === archived);
        }

        if (command === "chat_list_messages") {
          return {
            messages: [],
            hasMore: false,
          };
        }

        if (command === "chat_conversation_usage") {
          return {
            promptTokens: 0,
            completionTokens: 0,
            totalTokens: 0,
          };
        }

        if (command === "plugin:event|listen") {
          const id = eventId;
          eventId += 1;
          return id;
        }

        if (command === "plugin:event|unlisten") {
          return true;
        }

        if (command === "server_set_preferences") {
          return payload.preferences ?? {
            port: null,
            keepRunning: false,
            connectTimeoutSecs: 5,
            firstTokenTimeoutSecs: 120,
            tokenIdleTimeoutSecs: 60,
          };
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
  });

  await page.goto("/");

  await expect(page.getByRole("navigation", { name: "Workspaces" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Conversations" })).toBeVisible();
  await expect(page.getByText("No conversations yet")).toBeVisible();
  await expect(page.getByRole("button", { name: "New conversation" })).toBeVisible();

  await page.getByRole("button", { name: "Keyboard shortcuts" }).click();
  await expect(page.getByRole("heading", { name: "Keyboard shortcuts" })).toBeVisible();
});
