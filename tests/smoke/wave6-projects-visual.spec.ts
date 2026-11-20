import { copyFile, mkdir, writeFile } from "node:fs/promises";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { installFoundryMock } from "./helpers/installFoundryMock";

const output = "checks/wave6/final";
const longName = "Research notes for the international accessibility and language team";
const folderPath = "/Users/demo/Work/International projects/Research notes with a deliberately long folder name";
const history = "Compare the keyboard navigation findings from Lisbon and São Paulo. Keep the original interview notes unchanged, and summarize the remaining translation questions for Friday’s review.";
const entries = [
  { id: 21, name: longName, folderPath, bindingRevision: 1, availability: "available" as const },
  { id: 22, name: "Harbor design system", folderPath: "/Users/demo/Work/Harbor/design-system", bindingRevision: 1, availability: "available" as const },
  { id: 23, name: "Field recordings — spring interviews", folderPath: "/Volumes/Fieldwork/Interviews/2026 spring", bindingRevision: 1, availability: "missing" as const },
  { id: 24, name: "Filesystem root reference", folderPath: "/", bindingRevision: 1, availability: "available" as const },
];
const conversations = [
  { title: "Interview synthesis — open questions", projectId: 21, content: history },
  { title: "Accessibility review checklist", projectId: 21, content: "Check focus order, folder context, and language fit before the next review." },
  { title: "Recovered interview notes", projectId: 23, content: "The external drive is disconnected. These saved interview notes remain available for reading while the original folder is unavailable." },
];

async function capture(page: Page, state: string, width: number) {
  await page.evaluate(async () => {
    await document.fonts.ready;
    window.scrollTo(0, 0);
    await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
  });
  const metrics = await page.evaluate(() => {
    const dialog = document.querySelector('[role="dialog"]')?.getBoundingClientRect();
    const focused = document.activeElement;
    const genericFailures = new Set(["The Foundry Local command failed.", "O comando do Foundry Local falhou.", "El comando de Foundry Local falló."]);
    function computedColors(element: Element) {
      const style = getComputedStyle(element);
      const backgrounds: string[] = [];
      let ancestor: Element | null = element;
      while (ancestor) {
        backgrounds.push(getComputedStyle(ancestor).backgroundColor);
        ancestor = ancestor.parentElement;
      }
      return { text: element.textContent?.trim(), color: style.color, backgroundColor: style.backgroundColor, ancestorBackgrounds: backgrounds, fontSize: style.fontSize, opacity: style.opacity };
    }
    return {
      unhandledCommands: (window as unknown as { __foundryUnhandledCalls: string[] }).__foundryUnhandledCalls,
      projectCreationCommands: (window as unknown as { __foundryProjectCalls: Array<{ command: string }> }).__foundryProjectCalls.map((call) => call.command).filter((command) => ["projects_pick_folder", "projects_confirm_folder", "projects_discard_folder"].includes(command)),
      dialogFields: Array.from(document.querySelectorAll<HTMLInputElement>('[role="dialog"] input')).map((input) => ({ id: input.id, value: input.value, readOnly: input.readOnly, disabled: input.disabled })),
      normalStateFailureToastCount: Array.from(document.querySelectorAll('[role="status"] button')).filter((button) => genericFailures.has(button.textContent?.trim() ?? "")).length,
      viewportWidth: innerWidth,
      viewportHeight: innerHeight,
      layoutWidth: document.documentElement.clientWidth,
      scrollWidth: document.documentElement.scrollWidth,
      bodyWidth: document.body.getBoundingClientRect().width,
      bodyMinWidth: getComputedStyle(document.body).minWidth,
      scrollX,
      scrollY,
      fonts: document.fonts.status,
      dialogBounds: dialog ? { x: dialog.x, y: dialog.y, width: dialog.width, height: dialog.height, right: dialog.right, bottom: dialog.bottom } : null,
      focus: focused ? { tag: focused.tagName, id: focused.id, label: focused.getAttribute("aria-label") } : null,
      focusedControlOutline: focused ? { ...computedColors(focused), focusVisible: focused.matches(":focus-visible"), outlineColor: getComputedStyle(focused).outlineColor, outlineStyle: getComputedStyle(focused).outlineStyle, outlineWidth: getComputedStyle(focused).outlineWidth, outlineOffset: getComputedStyle(focused).outlineOffset } : null,
      criticalNewTextColors: Array.from(document.querySelectorAll('[role="dialog"] p, [role="dialog"] label, [role="group"][aria-label] button, [aria-label="Project folder"], [aria-label="Pasta do projeto"], [aria-label="Carpeta del proyecto"], [role="status"], button[title], span[title], aside p')).filter((element) => element.getBoundingClientRect().width > 0 && element.textContent?.trim()).map(computedColors),
      disabledControls: Array.from(document.querySelectorAll<HTMLButtonElement>("button:disabled")).map((button) => button.getAttribute("aria-label") || button.textContent?.trim()),
    };
  });
  expect(metrics.unhandledCommands, `${state}: unhandled IPC commands`).toEqual([]);
  expect(metrics.normalStateFailureToastCount, `${state}: generic command-failure toasts`).toBe(0);
  expect(metrics.fonts, `${state}: loaded fonts`).toBe("loaded");
  if (state.startsWith("keyboard-")) {
    expect(metrics.focusedControlOutline?.focusVisible, `${state}: keyboard focus visible`).toBe(true);
    expect(metrics.focusedControlOutline?.outlineStyle).toBe("solid");
    expect(parseFloat(metrics.focusedControlOutline?.outlineWidth ?? "0")).toBeGreaterThanOrEqual(2);
    expect(parseFloat(metrics.focusedControlOutline?.outlineOffset ?? "0")).toBeGreaterThanOrEqual(2);
  }
  await mkdir(output, { recursive: true });
  await page.screenshot({ path: `${output}/${state}-${width}.png`, fullPage: true, animations: "disabled", caret: "hide" });
  await writeFile(`${output}/${state}-${width}.json`, `${JSON.stringify({ state, supportedDesktop: width >= 960, note: width < 960 ? "Exploratory only: existing desktop minimum width is 960px; this capture does not establish mobile support." : null, ...metrics }, null, 2)}\n`);
  if (width >= 960) expect.soft(metrics.scrollWidth, `${state}: supported desktop horizontal overflow`).toBeLessThanOrEqual(width);
}

async function keyboardFocus(page: Page, locator: Locator) {
  await locator.focus();
  await page.keyboard.press("Tab");
  await page.keyboard.press("Shift+Tab");
  await expect(locator).toBeFocused();
}

async function openProjects(page: Page, options: Parameters<typeof installFoundryMock>[1] = {}) {
  await installFoundryMock(page, options);
  await page.goto("/");
  const language = options.language ?? "en";
  await page.getByRole("button", { name: language === "pt-BR" ? "Não mostrar novamente" : language === "es" ? "No mostrar de nuevo" : "Never show again", exact: true }).click();
  await page.getByRole("button", { name: language === "pt-BR" ? "Projetos" : language === "es" ? "Proyectos" : "Projects", exact: true }).click();
}

for (const [width, height] of [[1440, 900], [960, 640], [390, 844]]) {
  test(`project visual states at ${width}`, async ({ browser }) => {
    test.setTimeout(60_000);
    const context = await browser.newContext({ viewport: { width, height }, baseURL: "http://127.0.0.1:4173", reducedMotion: "reduce" });
    const page = await context.newPage();
    const newSelection = { id: 25, name: "Coastal accessibility fieldwork", folderPath: "/Users/demo/Work/International projects/Coastal accessibility fieldwork", bindingRevision: 1, availability: "available" as const, created: true };
    await openProjects(page, { projects: { entries, conversations, picks: [newSelection, { ...entries[0], created: false }], failures: { projects_create_conversation: ["ProjectBusy"] } } });
    await expect(page.getByRole("button", { name: /Harbor design system.*Users/ })).toBeVisible();
    await page.getByRole("button", { name: "New project", exact: true }).focus();
    await page.keyboard.press("Enter");
    const createDialog = page.getByRole("dialog", { name: "New project", exact: true });
    const initialName = createDialog.getByRole("textbox", { name: "Project name", exact: true });
    await expect(initialName).toHaveValue(newSelection.name);
    await expect(initialName).toBeEditable();
    await expect(initialName).toBeFocused();
    await expect(createDialog.getByLabel("Project folder")).toHaveText(newSelection.folderPath);
    await expect.poll(() =>
      initialName.evaluate((input) => parseFloat(getComputedStyle(input).outlineOffset)),
    ).toBeGreaterThanOrEqual(2);
    const initialOutline = await initialName.evaluate((input) => {
      const style = getComputedStyle(input);
      return { style: style.outlineStyle, width: parseFloat(style.outlineWidth), offset: parseFloat(style.outlineOffset) };
    });
    expect(initialOutline.style).toBe("solid");
    expect(initialOutline.width).toBeGreaterThanOrEqual(2);
    expect(initialOutline.offset).toBeGreaterThanOrEqual(2);
    await capture(page, "create-project-name", width);
    if (width >= 960) {
      await page.keyboard.press("Shift+Tab");
      await expect(createDialog.getByRole("button", { name: "Close", exact: true })).toBeFocused();
      await capture(page, "keyboard-dialog-close", width);
    }
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await page.getByRole("button", { name: "New project", exact: true }).focus();
    await page.keyboard.press("Enter");
    const existingDialog = page.getByRole("dialog", { name: "Folder already registered", exact: true });
    await expect(existingDialog).toBeVisible();
    await expect(existingDialog.getByRole("textbox", { name: "Project name", exact: true })).toHaveCount(0);
    await expect(existingDialog.getByRole("button", { name: "Open existing project", exact: true })).toBeFocused();
    await expect(existingDialog.getByLabel("Project folder")).toHaveText(folderPath);
    await capture(page, "open-existing-project", width);
    await existingDialog.getByRole("button", { name: "Cancel", exact: true }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    const creationCommands = await page.evaluate(() => (window as unknown as { __foundryProjectCalls: Array<{ command: string }> }).__foundryProjectCalls.map((call) => call.command));
    expect(creationCommands.filter((command) => command === "projects_confirm_folder")).toEqual([]);
    expect(creationCommands.filter((command) => command === "projects_discard_folder")).toHaveLength(2);
    await expect(page.getByRole("button", { name: new RegExp(newSelection.name) })).toHaveCount(0);
    await capture(page, "project-list", width);
    if (width >= 960) {
      await keyboardFocus(page, page.getByRole("button", { name: "Projects", exact: true }));
      await capture(page, "keyboard-project-navigation", width);
    }
    await page.getByRole("button", { name: new RegExp(longName) }).click();
    await expect(page.getByLabel("Project folder")).toHaveText(folderPath);
    await page.getByRole("button", { name: /Interview synthesis.*qwen/ }).click();
    await expect(page.getByText(history, { exact: true })).toBeVisible();
    await capture(page, "selected-project", width);
    await mkdir(".impeccable/review", { recursive: true });
    await copyFile(`${output}/selected-project-${width}.png`, `.impeccable/review/${width === 1440 ? "desktop" : width === 960 ? "desktop-minimum" : "mobile"}.png`);
    await page.getByText("Conversation actions", { exact: true }).click();
    await page.getByRole("button", { name: "Move conversation", exact: true }).click();
    await expect(page.getByRole("dialog")).toContainText("History and attachments retain information from the previous context");
    await page.getByRole("combobox", { name: "Destination" }).selectOption("22");
    await page.getByRole("combobox", { name: "Destination" }).focus();
    await page.keyboard.press("Tab");
    await page.keyboard.press("Shift+Tab");
    await expect(page.getByRole("combobox", { name: "Destination" })).toBeFocused();
    await capture(page, "move-dialog", width);
    await page.keyboard.press("Escape");
    await page.getByRole("button", { name: "Rename project", exact: true }).focus();
    await page.keyboard.press("Enter");
    await expect(page.getByRole("textbox", { name: "Project name", exact: true })).toBeFocused();
    await capture(page, "rename-dialog", width);
    await page.keyboard.press("Escape");
    await page.getByRole("button", { name: "New conversation", exact: true }).click();
    await expect(page.getByRole("alert")).toContainText("An execution is active");
    await expect(page.getByText(history, { exact: true })).toBeVisible();
    await capture(page, "busy-error", width);
    await page.getByRole("button", { name: "All projects", exact: true }).click();
    await page.getByRole("button", { name: /Field recordings.*Volumes/ }).click();
    await expect(page.getByRole("button", { name: "Locate folder", exact: true })).toBeVisible();
    await expect(page.getByText(conversations[2].content, { exact: true })).toBeVisible();
    await capture(page, "unavailable-folder", width);
    if (width >= 960) {
      await keyboardFocus(page, page.getByRole("button", { name: "Locate folder", exact: true }));
      await capture(page, "keyboard-locate-folder", width);
    }
    await page.getByRole("button", { name: "All projects", exact: true }).click();
    await page.getByRole("button", { name: /Filesystem root reference/ }).click();
    await expect(page.getByLabel("Project folder")).toHaveText("/");
    await page.getByRole("button", { name: "Rename project", exact: true }).click();
    await expect(page.getByRole("textbox", { name: "Project name", exact: true })).toBeFocused();
    await capture(page, "root-rename-dialog", width);
    await context.close();
    const emptyContext = await browser.newContext({ viewport: { width, height }, baseURL: "http://127.0.0.1:4173", reducedMotion: "reduce" });
    const empty = await emptyContext.newPage();
    await openProjects(empty);
    await expect(empty.getByText("No projects yet", { exact: true })).toBeVisible();
    await capture(empty, "empty-projects", width);
    await emptyContext.close();
  });
}

for (const language of ["pt-BR", "es"] as const) {
  test(`localized unavailable project ${language}`, async ({ page }) => {
    await page.setViewportSize({ width: 960, height: 640 });
    await openProjects(page, { language, projects: { entries, conversations } });
    await page.getByRole("button", { name: /Field recordings.*Volumes/ }).click();
    await expect(page.getByRole("button", { name: language === "pt-BR" ? "Localizar pasta" : "Localizar carpeta", exact: true })).toBeVisible();
    await expect(page.getByText(conversations[2].content, { exact: true })).toBeVisible();
    await capture(page, `unavailable-folder-${language}`, 960);
  });
}
