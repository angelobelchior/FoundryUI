import { expect, test } from "@playwright/test";
import { installFoundryMock } from "./helpers/installFoundryMock";

test("menu de contexto abre por teclado e fecha com Escape", async ({ page }) => {
  await installFoundryMock(page, { language: "en", workspace: "chat" });

  await page.goto("/");
  await page.getByRole("button", { name: "New conversation" }).click();

  const conversation = page.getByRole("button", { name: /Untitled conversation/ }).first();

  await conversation.focus();
  await page.keyboard.press("Shift+F10");

  const menu = page.getByRole("menu", { name: "Conversation actions" });

  await expect(menu).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
});

test("filtros de models abrem por teclado e fecham com Escape", async ({ page }) => {
  await installFoundryMock(page, { language: "en", workspace: "models" });

  await page.goto("/");

  const stateFilter = page.getByRole("button", { name: /^State/ }).first();

  await stateFilter.focus();
  await page.keyboard.press("Enter");

  const menu = page.getByRole("menu", { name: "State" });

  await expect(menu).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  await expect(stateFilter).toBeFocused();
});

test("onboarding fecha com Escape", async ({ page }) => {
  await installFoundryMock(page, { language: "en", workspace: "chat", onboarding: true });

  await page.goto("/");

  const onboardingTitle = page.getByRole("heading", { name: "Set up FoundryUI" });

  await expect(onboardingTitle).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(onboardingTitle).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "Conversations" })).toBeVisible();
});
