import { expect, type Page } from "@playwright/test";

export async function openProjectTestSession(page: Page) {
  const neverShow = page.getByRole("button", {
    name: /^(Never show again|Não mostrar novamente|No mostrar de nuevo)$/,
  }).first();
  const skip = page.getByRole("button", {
    name: /^(Skip for now|Pular por enquanto|Omitir por ahora)$/,
  }).first();
  async function dismissOnboarding() {
    if (await neverShow.isVisible()) await neverShow.click();
    else if (await skip.isVisible()) await skip.click();
  }
  await page.addLocatorHandler(neverShow.or(skip).first(), dismissOnboarding);
  await page.goto("/");
  await expect(page.getByRole("button", { name: /^(Projects|Projetos|Proyectos)$/ })).toBeVisible();
  await dismissOnboarding();
}
