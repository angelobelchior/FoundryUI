import { expect, test, type Locator, type Page } from "@playwright/test";
import { installFoundryMock } from "./helpers/installFoundryMock";

async function clickOnboardingNextUntilVisible(page: Page, target: Locator, maxClicks = 40) {
  for (let attempt = 0; attempt < maxClicks; attempt += 1) {
    if (await target.isVisible()) {
      return;
    }

    await page.getByRole("button", { name: "Next" }).click();

    try {
      await expect(target).toBeVisible({ timeout: 400 });
      return;
    } catch {
      continue;
    }
  }

  await expect(target).toBeVisible();
}

async function openOnboardingReadyStep(page: Page) {
  await page.goto("/");

  await expect(page.getByText("Workspace map")).toBeVisible();

  await clickOnboardingNextUntilVisible(page, page.getByRole("button", { name: "Finish walkthrough" }));
}

test("onboarding mínimo com entrada no workspace", async ({ page }) => {
  await installFoundryMock(page, { language: "en", workspace: "chat", onboarding: true });

  await openOnboardingReadyStep(page);

  await page.getByRole("button", { name: "Finish walkthrough" }).click();

  await expect(page.getByText("Workspace map")).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "Settings", level: 1 })).toBeVisible();
});

test("onboarding navega pelos workspaces ao avançar", async ({ page }) => {
  await installFoundryMock(page, { language: "en", workspace: "chat", onboarding: true });

  await page.goto("/");

  await expect(page.getByText("Workspace map")).toBeVisible();

  await clickOnboardingNextUntilVisible(page, page.getByRole("heading", { name: "Conversations" }), 10);

  await clickOnboardingNextUntilVisible(page, page.getByRole("button", { name: "New transcription" }), 10);

  await clickOnboardingNextUntilVisible(page, page.getByRole("button", { name: "New collection" }), 10);

  await clickOnboardingNextUntilVisible(page, page.getByRole("heading", { name: "Models" }).first(), 10);

  await clickOnboardingNextUntilVisible(page, page.getByRole("heading", { name: "Settings", level: 1 }), 10);
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

test("chat troca variant por conversa e registra divisores", async ({ page }) => {
  await installFoundryMock(page, { language: "en", workspace: "chat" });

  await page.goto("/");

  await page.getByRole("button", { name: "New conversation" }).click();
  await expect(page.getByRole("heading", { name: "Untitled conversation" })).toBeVisible();

  await expect(page.getByRole("button", { name: "Variant" })).toBeVisible();

  await page.getByRole("button", { name: "Variant" }).click();
  await page.getByRole("menuitemradio", { name: /CPU \(4 threads\)/ }).click();

  await expect(
    page.getByText("Model changed to qwen3-0.6b · qwen3-0.6b-cpu:4"),
  ).toBeVisible();

  await page.getByRole("button", { name: "Variant" }).click();
  await page.getByRole("menuitemradio", { name: "Automatic" }).click();

  await expect(page.getByText(/^Model changed to qwen3-0\.6b$/)).toBeVisible();
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

  await page.getByRole("complementary").getByRole("button", { name: "Embeddings" }).click();
  await expect(page.getByRole("heading", { name: "Leave and discard progress?" })).toBeVisible();
  await page.getByRole("button", { name: "Cancel" }).click();

  await expect(page.getByText("Step 1 of 3")).toBeVisible();

  await page.getByRole("complementary").getByRole("button", { name: "Embeddings" }).click();
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

  await expect(
    page.getByRole("button", { name: /Document extracted from corpus-one\.txt/ }).first(),
  ).toBeVisible();
});
