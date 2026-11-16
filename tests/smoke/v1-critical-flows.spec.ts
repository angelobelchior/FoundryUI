import { expect, test } from "@playwright/test";
import { installFoundryMock } from "./helpers/installFoundryMock";

test("onboarding mínimo com entrada no workspace", async ({ page }) => {
  await installFoundryMock(page, { language: "en", workspace: "chat", onboarding: true });

  await page.goto("/");

  await expect(page.getByRole("heading", { name: "Set up FoundryUI" })).toBeVisible();
  await expect(page.getByText(/Foundry Local CLI .* is ready/)).toBeVisible();

  await page.getByRole("button", { name: "Continue" }).click();
  await expect(page.getByText("The local server is running.")).toBeVisible();

  await page.getByRole("button", { name: "Continue" }).click();
  await expect(page.getByText("Downloads are optional.")).toBeVisible();

  await page.getByRole("button", { name: "Continue" }).click();
  await expect(page.getByText("Everything is ready. Pick a workspace to begin.")).toBeVisible();

  await page.getByRole("dialog").getByRole("button", { name: "Models" }).click();

  await expect(page.getByRole("heading", { name: "Models" }).first()).toBeVisible();
  await expect(page.getByRole("heading", { name: "Set up FoundryUI" })).toHaveCount(0);
});

test("chat cria conversa, envia e recebe resposta stream", async ({ page }) => {
  await installFoundryMock(page, { language: "en", workspace: "chat" });

  await page.goto("/");

  await page.getByRole("button", { name: "New conversation" }).click();
  await expect(page.getByRole("heading", { name: "Untitled conversation" })).toBeVisible();

  await page.getByPlaceholder(/Message /).fill("Hello smoke");
  await page.getByRole("button", { name: "Send" }).click();

  await expect(page.getByText("Mock reply to: Hello smoke")).toBeVisible();
});

test("models lista, carrega e descarrega", async ({ page }) => {
  await installFoundryMock(page, { language: "en", workspace: "models" });

  await page.goto("/");

  await expect(page.getByRole("heading", { name: "Models" }).first()).toBeVisible();
  await page.getByRole("button", { name: /phi-4-mini/ }).click();

  await page.getByRole("button", { name: "Load", exact: true }).click();
  await expect(page.getByRole("button", { name: "Unload", exact: true })).toBeVisible();

  await page.getByRole("button", { name: "Unload", exact: true }).click();
  await expect(page.getByRole("button", { name: "Load", exact: true })).toBeVisible();
});

test("speech importa áudio e conclui transcrição", async ({ page }) => {
  await installFoundryMock(page, { language: "en", workspace: "speech" });

  await page.goto("/");

  await page.getByRole("button", { name: "New transcription" }).click();
  await expect(page.getByText("Step 1 of 3")).toBeVisible();
  await page.getByRole("button", { name: "Next: audio" }).click();
  await page.getByRole("button", { name: /Drop an MP3/ }).click();
  await page.getByRole("button", { name: "Next: transcribe" }).click();
  await page.getByRole("button", { name: "Transcribe", exact: true }).click();

  await expect(page.getByRole("textbox", { name: "Transcription text" })).toHaveValue(
    "Mock transcription from audio-sample.mp3",
  );
});

test("speech alerta ao trocar de aba durante nova transcrição", async ({ page }) => {
  await installFoundryMock(page, { language: "en", workspace: "speech" });

  await page.goto("/");

  await page.getByRole("button", { name: "New transcription" }).click();
  await expect(page.getByText("Step 1 of 3")).toBeVisible();

  await page.getByRole("button", { name: "Embeddings" }).click();
  await expect(page.getByRole("heading", { name: "Leave and discard progress?" })).toBeVisible();
  await page.getByRole("button", { name: "Cancel" }).click();

  await expect(page.getByText("Step 1 of 3")).toBeVisible();

  await page.getByRole("button", { name: "Embeddings" }).click();
  await expect(page.getByRole("heading", { name: "Leave and discard progress?" })).toBeVisible();
  await page.getByRole("button", { name: "Leave", exact: true }).click();

  await expect(page.getByRole("button", { name: "New collection" })).toBeVisible();
});

test("embeddings cria coleção, vetoriza e pesquisa", async ({ page }) => {
  await installFoundryMock(page, { language: "en", workspace: "embeddings" });

  await page.goto("/");

  await page.getByRole("button", { name: "New collection" }).click();
  await page.getByRole("button", { name: "Next: load files" }).click();
  await page.getByRole("button", { name: "Import files (.txt/.md/.pdf)" }).click();
  await page.getByRole("button", { name: "Next: vectorization" }).click();
  await page.getByRole("button", { name: "Start vectorization" }).click();
  await page.getByRole("button", { name: "Next: vector search" }).click();

  await page.getByLabel("Query").fill("Document extracted");
  await page.getByRole("button", { name: "Search", exact: true }).click();

  await expect(page.getByText("Document extracted from corpus-one.txt")).toBeVisible();
});
