import { expect, test, type Locator, type Page } from "@playwright/test";
import { installFoundryMock } from "./helpers/installFoundryMock";
import { deferProjectReply, projectReplyEntered, releaseProjectReply } from "./helpers/projectDeferredReply";
import { openProjectTestSession } from "./helpers/projectTestSession";

for (const revisitSource of [false, true]) {
test(`a delayed relocation cannot refresh ${revisitSource ? "a revisited source epoch" : "another project's scope"}`, async ({ page }) => {
  const research = { id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "missing" as const };
  await installFoundryMock(page, { projects: {
    entries: [research, { id: 22, name: "Design", folderPath: "/Users/demo/Design", bindingRevision: 1, availability: "available" }],
    conversations: [{ title: "Research notes", projectId: 21 }, { title: "Design notes", projectId: 22 }],
    relocations: [{ ...research, folderPath: "/Users/demo/Recovered research", bindingRevision: 2, availability: "available" }],
  } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: /Research.*Users/ }).click();
  await deferProjectReply(page, "projects_pick_relocate", 21, 1, "id");
  await page.getByRole("button", { name: "Locate folder", exact: true }).click();
  await expect.poll(() => projectReplyEntered(page)).toBe(true);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: /Design.*Users/ }).click();
  await expect(page.getByRole("heading", { name: "Design notes", level: 1 })).toBeVisible();
  if (revisitSource) {
    await page.getByRole("button", { name: "All projects", exact: true }).click();
    await page.getByRole("button", { name: /Research.*Users/ }).click();
    await expect(page.getByRole("heading", { name: "Research notes", level: 1 })).toBeVisible();
  }
  const readsBeforeRelease = await page.evaluate(() => (window as unknown as { __foundryProjectCalls: Array<{ command: string }> }).__foundryProjectCalls.filter((call) => call.command === "projects_list_conversations").length);
  await releaseProjectReply(page);
  await expect(page.getByRole("button", { name: "New project", exact: true })).toBeEnabled();
  expect(await page.evaluate(() => (window as unknown as { __foundryProjectCalls: Array<{ command: string }> }).__foundryProjectCalls.filter((call) => call.command === "projects_list_conversations").length)).toBe(readsBeforeRelease);
  if (revisitSource) {
    await expect(page.getByLabel("Project folder")).toHaveText("/Users/demo/Recovered research");
    await expect(page.getByRole("heading", { name: "Research notes", level: 1 })).toBeVisible();
    return;
  }
  await expect(page.getByLabel("Project folder")).toHaveText("/Users/demo/Design");
  await expect(page.getByRole("heading", { name: "Design notes", level: 1 })).toBeVisible();
  await expect(page.getByRole("button", { name: /Design notes.*qwen/ })).toHaveAttribute("aria-current", "true");
  await expect(page.getByRole("button", { name: /Research notes.*qwen/ })).toHaveCount(0);
  await page.getByRole("button", { name: "All projects", exact: true }).click();
  await page.getByRole("button", { name: /Research.*Users/ }).click();
  await expect(page.getByLabel("Project folder")).toHaveText("/Users/demo/Recovered research");
  await expect(page.getByRole("heading", { name: "Research notes", level: 1 })).toBeVisible();
});
}

for (const [action, command] of [["Favorite", "chat_set_conversation_favorite"], ["Archive", "chat_set_conversation_archived"]] as const) {
  test(`a delayed ${action.toLowerCase()} receipt cannot overwrite another conversation scope`, async ({ page }) => {
    await installFoundryMock(page, { projects: {
      entries: [{ id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available" }],
      conversations: [{ title: "Source notes", projectId: 21, content: "Source project history" }, { title: "Independent conversation", projectId: null, content: "Independent history remains selected" }],
    } });
    await openProjectTestSession(page);
    await page.getByRole("button", { name: "Projects", exact: true }).click();
    await page.getByRole("button", { name: /Research.*Users/ }).click();
    await deferProjectReply(page, command, 1, 1, "id");
    await page.getByRole("button", { name: /Source notes.*qwen/ }).click({ button: "right" });
    await page.getByRole("menuitem", { name: action, exact: true }).click();
    await expect.poll(() => projectReplyEntered(page)).toBe(true);
    await page.getByRole("button", { name: "Conversations", exact: true }).click();
    await expect(page.getByRole("heading", { name: "Independent conversation", level: 1 })).toBeVisible();
    await releaseProjectReply(page);
    await expect(page.getByRole("heading", { name: "Independent conversation", level: 1 })).toBeVisible();
    await expect(page.getByRole("button", { name: /Independent conversation.*qwen/ })).toHaveAttribute("aria-current", "true");
    await expect(page.getByRole("button", { name: /Source notes.*qwen/ })).toHaveCount(0);
    await expect(page.getByText("Independent history remains selected", { exact: true })).toBeVisible();
    await expect(page.getByLabel("Project folder")).toHaveCount(0);
  });
}

test("native name validation keeps the same folder selection available for retry", async ({ page }) => {
  await installFoundryMock(page, { projects: {
    picks: [{ id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available", created: true }],
    failures: { projects_confirm_folder: ["ProjectInvalidInput"] },
  } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: "New project", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "New project", exact: true });
  await dialog.getByRole("button", { name: "Create project", exact: true }).click();
  await expect(dialog.getByRole("alert")).toHaveText("Enter a valid project name.");
  await expect(page.getByRole("heading", { name: "Research", level: 1 })).toHaveCount(0);
  await dialog.getByRole("textbox", { name: "Project name", exact: true }).fill("Field work");
  await dialog.getByRole("button", { name: "Create project", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Field work", level: 1 })).toBeVisible();
  const selections = await page.evaluate(() => (window as unknown as { __foundryProjectCalls: Array<{ command: string; payload: { selectionId?: string } }> }).__foundryProjectCalls.filter((call) => call.command === "projects_confirm_folder").map((call) => call.payload.selectionId));
  expect(selections).toHaveLength(2);
  expect(selections[0]).toBe(selections[1]);
});

for (const dismissal of ["Cancel", "Escape"] as const) {
test(`an expired folder selection recovers through ${dismissal} without registering a project`, async ({ page }) => {
  await installFoundryMock(page, { projects: {
    picks: [{ id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available", created: true }, { id: 22, name: "Fresh selection", folderPath: "/Users/demo/Fresh", bindingRevision: 1, availability: "available", created: true }],
    failures: { projects_confirm_folder: ["ProjectInvalidSelection"] },
  } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: "New project", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "New project", exact: true });
  await dialog.getByRole("button", { name: "Create project", exact: true }).click();
  await expect(dialog.getByRole("alert")).toContainText("This folder selection is no longer valid");
  await expect(dialog.getByRole("button", { name: "Create project", exact: true })).toBeDisabled();
  const reuseCode = await page.evaluate(async () => {
    const host = window as unknown as {
      __foundryProjectCalls: Array<{ command: string; payload: { selectionId?: string } }>;
      __TAURI_INTERNALS__: { invoke: (command: string, payload: Record<string, unknown>) => Promise<unknown> };
    };
    const selectionId = host.__foundryProjectCalls.find((call) => call.command === "projects_confirm_folder")?.payload.selectionId;
    try {
      await host.__TAURI_INTERNALS__.invoke("projects_confirm_folder", { selectionId, name: "Must never be registered" });
      return null;
    } catch (error) {
      return typeof error === "object" && error !== null && "code" in error ? error.code : null;
    }
  });
  expect(reuseCode).toBe("ProjectInvalidSelection");
  if (dismissal === "Escape") await page.keyboard.press("Escape");
  else await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.getByText("No projects yet", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "New project", exact: true }).click();
  await expect(dialog.getByRole("textbox", { name: "Project name", exact: true })).toHaveValue("Fresh selection");
  await expect(dialog.getByLabel("Project folder")).toHaveText("/Users/demo/Fresh");
  await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(page.getByText("No projects yet", { exact: true })).toBeVisible();
  expect(await page.evaluate(() => (window as unknown as { __TAURI_INTERNALS__: { invoke: (command: string) => Promise<unknown> } }).__TAURI_INTERNALS__.invoke("projects_list"))).toEqual([]);
});
}

test("a duplicate discovered at confirmation opens the committed project without applying the proposed name", async ({ page }) => {
  await installFoundryMock(page, { projects: {
    picks: [{ id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available", created: true }],
    confirmations: [{ id: 32, name: "Existing research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available", created: false }],
  } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: "New project", exact: true }).click();
  await page.getByRole("textbox", { name: "Project name", exact: true }).fill("Must not rename existing project");
  await page.getByRole("button", { name: "Create project", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Existing research", level: 1 })).toBeVisible();
  await expect(page.getByRole("status").filter({ hasText: "This folder already belongs to a project" })).toBeVisible();
  await expect(page.getByLabel("Project folder")).toHaveText("/Users/demo/Research");
});

test("new project names are confirmed before registration and cancel discards the folder selection", async ({ page }) => {
  const project = { id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available" as const, created: true };
  await installFoundryMock(page, { onboarding: true, projects: { picks: [project, project] } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: "New project", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "New project", exact: true });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("textbox", { name: "Project name", exact: true })).toHaveValue("Research");
  await expect(dialog.getByRole("textbox", { name: "Project name", exact: true })).toBeFocused();
  const initialNameFocus = await visualContrast(dialog.getByRole("textbox", { name: "Project name", exact: true }), true);
  expect(initialNameFocus.outlineStyle).toBe("solid");
  expect(initialNameFocus.outlineWidth).toBeGreaterThanOrEqual(2);
  expect(initialNameFocus.outlineOffset).toBeGreaterThanOrEqual(2);
  expect(initialNameFocus.contrast).toBeGreaterThanOrEqual(3);
  expect((await visualContrast(dialog.getByText("Confirm the project name before adding this folder. No files will be changed.", { exact: true }))).contrast).toBeGreaterThanOrEqual(4.5);
  await expect(dialog.getByLabel("Project folder")).toHaveText("/Users/demo/Research");
  await expect(page.getByRole("heading", { name: "Research", level: 1 })).toHaveCount(0);
  expect(await page.evaluate(() => (window as unknown as { __foundryProjectCalls: Array<{ command: string }> }).__foundryProjectCalls.filter((call) => call.command === "projects_confirm_folder"))).toEqual([]);
  await dialog.getByRole("textbox", { name: "Project name", exact: true }).fill("Not registered");
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.getByText("No projects yet", { exact: true })).toBeVisible();
  expect(await page.evaluate(() => (window as unknown as { __foundryProjectCalls: Array<{ command: string }> }).__foundryProjectCalls.filter((call) => call.command === "projects_discard_folder"))).toHaveLength(1);
  await page.getByRole("button", { name: "New project", exact: true }).click();
  await dialog.getByRole("textbox", { name: "Project name", exact: true }).fill("Field work");
  await dialog.getByRole("button", { name: "Create project", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Field work", level: 1 })).toBeVisible();
  await expect(page.getByLabel("Project folder")).toHaveText("/Users/demo/Research");
  await expect(page.getByRole("button", { name: "New conversation", exact: true })).toBeDisabled();
});

test("a duplicate folder prompts to open the existing project without renaming it", async ({ page }) => {
  const project = { id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available" as const };
  await installFoundryMock(page, { projects: { entries: [project], picks: [{ ...project, created: false }], conversations: [{ title: "Field notes", projectId: 21 }] } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: "New project", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Folder already registered", exact: true });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("textbox", { name: "Project name", exact: true })).toHaveCount(0);
  await dialog.getByRole("button", { name: "Open existing project", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Field notes", level: 1 })).toBeVisible();
  await expect(page.getByLabel("Project folder")).toHaveText("/Users/demo/Research");
  await expect(page.getByText("Research", { exact: true }).first()).toBeVisible();
});

test("non-first cross-project search selection survives a delayed scope refresh", async ({ page }) => {
  await installFoundryMock(page, { projects: {
    entries: [{ id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available" }],
    conversations: [{ title: "Requested notes", projectId: 21, content: "Needle evidence requested" }, { title: "Newest notes", projectId: 21, content: "Other project history" }, { title: "Independent conversation", projectId: null }],
  } });
  await openProjectTestSession(page);
  await deferProjectReply(page, "projects_list_conversations", 21, 2);
  await page.getByRole("button", { name: "Search messages", exact: true }).click();
  await page.getByRole("radio", { name: "All conversations", exact: true }).click();
  await page.getByRole("textbox", { name: "Search messages", exact: true }).fill("Needle");
  await page.getByRole("search").getByRole("button", { name: /Requested notes.*Needle evidence requested/ }).click();
  await expect.poll(() => projectReplyEntered(page)).toBe(true);
  await releaseProjectReply(page);
  await expect(page.getByRole("heading", { name: "Requested notes", level: 1 })).toBeVisible();
  await expect(page.getByRole("button", { name: /Requested notes.*qwen/ })).toHaveAttribute("aria-current", "true");
  await expect(page.getByLabel("Project folder")).toHaveText("/Users/demo/Research");
});

test("a delayed conversation creation cannot leak into a later navigation scope", async ({ page }) => {
  await installFoundryMock(page, { projects: {
    entries: [{ id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available" }],
    conversations: [{ title: "Field notes", projectId: 21 }, { title: "Independent conversation", projectId: null }],
  } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: /Research.*Users/ }).click();
  await deferProjectReply(page, "projects_create_conversation", 21);
  await page.getByRole("button", { name: "New conversation", exact: true }).click();
  await expect.poll(() => projectReplyEntered(page)).toBe(true);
  await page.getByRole("button", { name: "Conversations", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Independent conversation", level: 1 })).toBeVisible();
  await releaseProjectReply(page);
  await expect(page.getByRole("button", { name: "New conversation", exact: true })).toBeEnabled();
  await expect(page.getByRole("heading", { name: "Independent conversation", level: 1 })).toBeVisible();
  await expect(page.getByRole("button", { name: /Untitled conversation.*qwen/ })).toHaveCount(0);
  await expect(page.getByLabel("Project folder")).toHaveCount(0);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: /Research.*Users/ }).click();
  await expect(page.getByRole("button", { name: /Untitled conversation.*qwen/ })).toBeVisible();
});

async function visualContrast(locator: Locator, outline = false) {
  return locator.evaluate((element, checkOutline) => {
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = 1;
    const context = canvas.getContext("2d")!;
    function rgba(color: string) {
      context.clearRect(0, 0, 1, 1);
      context.fillStyle = color;
      context.fillRect(0, 0, 1, 1);
      return Array.from(context.getImageData(0, 0, 1, 1).data);
    }
    function composite(foreground: number[], background: number[]) {
      const alpha = foreground[3]! / 255;
      return background.map((channel, index) => foreground[index]! * alpha + channel * (1 - alpha));
    }
    const ancestors: Element[] = [];
    let node: Element | null = checkOutline ? element.parentElement : element;
    while (node) {
      ancestors.unshift(node);
      node = node.parentElement;
    }
    const background = ancestors.reduce((color, ancestor) => composite(rgba(getComputedStyle(ancestor).backgroundColor), color), [255, 255, 255]);
    const style = getComputedStyle(element);
    const foreground = composite(rgba(checkOutline ? style.outlineColor : style.color), background);
    function luminance(color: number[]) {
      return color.reduce((sum, channel, index) => {
        const value = channel / 255;
        return sum + (value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4) * [0.2126, 0.7152, 0.0722][index]!;
      }, 0);
    }
    const first = luminance(foreground);
    const second = luminance(background);
    return {
      contrast: (Math.max(first, second) + 0.05) / (Math.min(first, second) + 0.05),
      outlineStyle: style.outlineStyle,
      outlineWidth: parseFloat(style.outlineWidth),
      outlineOffset: parseFloat(style.outlineOffset),
      focusVisible: element.matches(":focus-visible"),
    };
  }, outline);
}

async function expectStrongProjectFocus(page: Page, locator: Locator) {
  await locator.focus();
  await page.keyboard.press("Tab");
  await page.keyboard.press("Shift+Tab");
  await expect(locator).toBeFocused();
  const focus = await visualContrast(locator, true);
  expect(focus.focusVisible).toBe(true);
  expect(focus.outlineStyle).toBe("solid");
  expect(focus.outlineWidth).toBeGreaterThanOrEqual(2);
  expect(focus.outlineOffset).toBeGreaterThanOrEqual(2);
  await expect.poll(async () => (await visualContrast(locator, true)).contrast, {
    message: `${await locator.getAttribute("aria-label") ?? await locator.innerText()} keyboard focus contrast`,
  }).toBeGreaterThanOrEqual(3);
}

test("all project navigation, recovery and dialog controls have strong keyboard focus", async ({ page }) => {
  const project = { id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "missing" as const };
  await installFoundryMock(page, { projects: {
    entries: [project],
    conversations: [{ title: "Field notes", projectId: 21 }],
    picks: [{ id: 22, name: "Design", folderPath: "/Users/demo/Design", bindingRevision: 1, availability: "available", created: true }, { ...project, created: false }],
  } });
  await openProjectTestSession(page);
  await expectStrongProjectFocus(page, page.getByRole("button", { name: "Projects", exact: true }));
  await page.keyboard.press("Enter");
  await expectStrongProjectFocus(page, page.getByRole("button", { name: "New project", exact: true }));
  await expectStrongProjectFocus(page, page.getByRole("button", { name: "Conversations", exact: true }));
  await expectStrongProjectFocus(page, page.getByRole("button", { name: /Research.*Users/ }));
  await page.keyboard.press("Enter");
  for (const name of ["All projects", "Locate folder", "Rename project", "Remove project"]) {
    await expectStrongProjectFocus(page, page.getByRole("button", { name, exact: true }));
  }
  await page.getByRole("button", { name: "Rename project", exact: true }).click();
  let dialog = page.getByRole("dialog", { name: "Rename project", exact: true });
  for (const name of ["Close", "Cancel", "Rename"]) {
    await expectStrongProjectFocus(page, dialog.getByRole("button", { name, exact: true }));
  }
  await page.keyboard.press("Escape");
  await page.getByText("Conversation actions", { exact: true }).click();
  await expectStrongProjectFocus(page, page.getByRole("button", { name: "Move conversation", exact: true }));
  await page.keyboard.press("Enter");
  dialog = page.getByRole("dialog", { name: "Move conversation", exact: true });
  for (const name of ["Close", "Cancel", "Move conversation"]) {
    await expectStrongProjectFocus(page, dialog.getByRole("button", { name, exact: true }));
  }
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "New project", exact: true }).click();
  dialog = page.getByRole("dialog", { name: "New project", exact: true });
  for (const name of ["Close", "Cancel", "Create project"]) {
    await expectStrongProjectFocus(page, dialog.getByRole("button", { name, exact: true }));
  }
  await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
  await page.getByRole("button", { name: "New project", exact: true }).click();
  dialog = page.getByRole("dialog", { name: "Folder already registered", exact: true });
  for (const name of ["Close", "Cancel", "Open existing project"]) {
    await expectStrongProjectFocus(page, dialog.getByRole("button", { name, exact: true }));
  }
});

test("project list retry has strong keyboard focus", async ({ page }) => {
  await installFoundryMock(page, { projects: { failures: { projects_list: ["ProjectStorage"] } } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await expectStrongProjectFocus(page, page.getByRole("button", { name: "Retry projects", exact: true }));
});

test("project navigation, notices and empty guidance keep functional text readable", async ({ page }) => {
  const project = { id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available" as const };
  await installFoundryMock(page, { projects: { picks: [{ ...project, created: true }, { ...project, created: false }] } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  for (const locator of [page.getByRole("button", { name: "Conversations", exact: true }), page.getByText("No projects yet", { exact: true }), page.getByText("Choose a folder to organize conversations in a project.", { exact: true })]) {
    expect.soft((await visualContrast(locator)).contrast).toBeGreaterThanOrEqual(4.5);
  }
  await page.getByRole("button", { name: "New project", exact: true }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Create project", exact: true }).click();
  expect.soft((await visualContrast(page.getByRole("button", { name: "All projects", exact: true }))).contrast).toBeGreaterThanOrEqual(4.5);
  await page.getByRole("button", { name: "Rename project", exact: true }).click();
  expect.soft((await visualContrast(page.getByRole("dialog").getByRole("button", { name: "Cancel", exact: true }))).contrast).toBeGreaterThanOrEqual(4.5);
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "New project", exact: true }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Open existing project", exact: true }).click();
  expect.soft((await visualContrast(page.getByRole("status").filter({ hasText: "This folder already belongs to a project" }))).contrast).toBeGreaterThanOrEqual(4.5);
  await page.getByRole("button", { name: "New conversation", exact: true }).click();
  await page.getByText("Conversation actions", { exact: true }).click();
  expect.soft((await visualContrast(page.getByRole("button", { name: "Move conversation", exact: true }))).contrast).toBeGreaterThanOrEqual(4.5);
  await page.getByRole("button", { name: "Move conversation", exact: true }).click();
  expect.soft((await visualContrast(page.getByRole("dialog").getByRole("button", { name: "Cancel", exact: true }))).contrast).toBeGreaterThanOrEqual(4.5);
});

test("critical project context and dialog instructions retain readable contrast", async ({ page }) => {
  await installFoundryMock(page, { projects: {
    entries: [{ id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "missing" }],
    conversations: [{ title: "Field notes", projectId: 21, content: "Readable project history" }],
  } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  for (const locator of [page.getByTitle("/Users/demo/Research", { exact: true }), page.getByText("Folder not found", { exact: true })]) {
    expect.soft((await visualContrast(locator)).contrast).toBeGreaterThanOrEqual(4.5);
  }
  await page.getByRole("button", { name: /Research.*Users/ }).click();
  for (const locator of [page.getByLabel("Project folder"), page.getByText("Review changes", { exact: true }), page.getByText("Folder not found", { exact: true }), page.getByRole("button", { name: "Locate folder", exact: true }), page.getByText("History remains available. Folder-dependent actions require an available folder.", { exact: true })]) {
    expect.soft((await visualContrast(locator)).contrast).toBeGreaterThanOrEqual(4.5);
  }
  await page.getByRole("button", { name: "Rename project", exact: true }).click();
  expect.soft((await visualContrast(page.getByText("Only the project name in this app changes. The folder is not renamed.", { exact: true }))).contrast).toBeGreaterThanOrEqual(4.5);
  expect.soft((await visualContrast(page.getByText("Project name", { exact: true }))).contrast).toBeGreaterThanOrEqual(4.5);
  await page.keyboard.press("Escape");
  await page.getByText("Conversation actions", { exact: true }).click();
  await page.getByRole("button", { name: "Move conversation", exact: true }).click();
  expect.soft((await visualContrast(page.getByText(/^History and attachments retain information/))).contrast).toBeGreaterThanOrEqual(4.5);
  expect.soft((await visualContrast(page.getByText("Destination", { exact: true }))).contrast).toBeGreaterThanOrEqual(4.5);
});

test("project rename and move inputs expose a solid high-contrast keyboard focus indicator", async ({ page }) => {
  await installFoundryMock(page, { projects: {
    entries: [{ id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available" }],
    conversations: [{ title: "Field notes", projectId: 21 }],
  } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: /Research.*Users/ }).click();
  await page.getByRole("button", { name: "Rename project", exact: true }).focus();
  await page.keyboard.press("Enter");
  const renameFocus = await visualContrast(page.getByRole("textbox", { name: "Project name" }), true);
  expect.soft(renameFocus.focusVisible).toBe(true);
  expect.soft(renameFocus.outlineStyle).toBe("solid");
  expect.soft(renameFocus.outlineWidth).toBeGreaterThanOrEqual(2);
  expect.soft(renameFocus.outlineOffset).toBeGreaterThanOrEqual(2);
  expect.soft(renameFocus.contrast).toBeGreaterThanOrEqual(3);
  await page.keyboard.press("Escape");
  await page.getByText("Conversation actions", { exact: true }).focus();
  await page.keyboard.press("Enter");
  await page.getByRole("button", { name: "Move conversation", exact: true }).focus();
  await page.keyboard.press("Enter");
  const moveFocus = await visualContrast(page.getByRole("combobox", { name: "Destination" }), true);
  expect.soft(moveFocus.focusVisible).toBe(true);
  expect.soft(moveFocus.outlineStyle).toBe("solid");
  expect.soft(moveFocus.outlineWidth).toBeGreaterThanOrEqual(2);
  expect.soft(moveFocus.outlineOffset).toBeGreaterThanOrEqual(2);
  expect.soft(moveFocus.contrast).toBeGreaterThanOrEqual(3);
});

test("populated project history loads parameter controls without command failure toasts", async ({ page }) => {
  await installFoundryMock(page, { projects: {
    entries: [{ id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available" }],
    conversations: [{ title: "Field notes", projectId: 21, content: "Readable project history" }],
  } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: /Research.*Users/ }).click();
  await expect(page.getByText("Readable project history", { exact: true })).toBeVisible();
  const unhandled = await page.evaluate(() => (window as unknown as { __foundryUnhandledCalls: string[] }).__foundryUnhandledCalls);
  await test.info().attach("unhandled-ipc-commands", { body: JSON.stringify(unhandled), contentType: "application/json" });
  expect(unhandled).toEqual([]);
  await expect(page.getByText("The Foundry Local command failed.", { exact: true })).toHaveCount(0);
  const parametersButton = page.getByRole("button", { name: "Generation parameters", exact: true });
  if (!(await parametersButton.isVisible())) {
    await page.getByText("Conversation actions", { exact: true }).click();
  }
  await parametersButton.click();
  const parameters = page.getByRole("dialog", { name: "Generation parameters", exact: true });
  await expect(parameters.getByRole("spinbutton", { name: "Temperature", exact: true })).toBeEnabled();
  await expect(parameters.getByRole("spinbutton", { name: "Temperature", exact: true })).toHaveValue("");
  await expect(page.getByText("The Foundry Local command failed.", { exact: true })).toHaveCount(0);
});

test("projects extend the chat context column and preserve native picker cancellation", async ({ page }) => {
  await installFoundryMock(page);
  await openProjectTestSession(page);
  if (process.env.WAVE6_BASELINE) {
    for (const [width, height] of [[1440, 900], [960, 640]]) {
      await page.setViewportSize({ width, height });
      await expect(page.getByRole("button", { name: "New conversation", exact: true })).toBeVisible();
      await page.screenshot({ path: `checks/wave6/baseline/chat-${width}.png` });
    }
    return;
  }
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await expect(page.getByText("No projects yet", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "New project", exact: true }).click();
  await expect(page.getByText("No projects yet", { exact: true })).toBeVisible();
});

test("conversation filtering stays within the selected project and keeps its folder visible", async ({ page }) => {
  await installFoundryMock(page, { projects: { entries: [
    { id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available" },
    { id: 22, name: "Design", folderPath: "/Users/demo/Design", bindingRevision: 1, availability: "available" },
  ], conversations: [{ title: "Field notes", projectId: 21 }, { title: "Other project notes", projectId: 22 }] } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: /Research.*Users/ }).click();
  await page.getByRole("button", { name: "Filter conversations", exact: true }).click();
  await page.getByRole("textbox", { name: "Filter conversations", exact: true }).fill("Other project notes");
  await expect(page.getByText("No conversations found", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: /Other project notes.*qwen/ })).toHaveCount(0);
  await expect(page.getByLabel("Project folder")).toHaveText("/Users/demo/Research");
  await page.getByRole("button", { name: "Clear search", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Field notes", level: 1 })).toBeVisible();
});

test("opening a project refreshes its current metadata and reports a removed project", async ({ page }) => {
  await installFoundryMock(page, { projects: { entries: [{ id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available" }], failures: { projects_get: ["ProjectNotFound"] } } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: /Research.*Users/ }).click();
  await expect(page.getByRole("alert")).toContainText("This project no longer exists");
  await expect(page.getByLabel("Project folder")).toHaveCount(0);
});

test("a backend busy rejection does not create or select a fictional conversation", async ({ page }) => {
  await installFoundryMock(page, { projects: { entries: [{ id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available" }], conversations: [{ title: "Field notes", projectId: 21 }], failures: { projects_create_conversation: ["ProjectBusy"] } } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: /Research.*Users/ }).click();
  await page.getByRole("button", { name: "New conversation", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("An execution is active");
  await expect(page.getByRole("heading", { name: "Field notes", level: 1 })).toBeVisible();
  await expect(page.getByRole("button", { name: /Untitled conversation.*qwen/ })).toHaveCount(0);
});

test("project creation does not require or load a chat model", async ({ page }) => {
  await installFoundryMock(page, { onboarding: true, projects: { picks: [{ id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available", created: true }] } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: "New project", exact: true }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Create project", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Research", level: 1 })).toBeVisible();
  await expect(page.getByLabel("Project folder")).toHaveText("/Users/demo/Research");
  await expect(page.getByRole("button", { name: "New conversation", exact: true })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Rename project", exact: true })).toBeEnabled();
  await expect(page.getByRole("button", { name: "Remove project", exact: true })).toBeEnabled();
  expect(await page.evaluate(() => (window as unknown as { __foundryCalls: Array<{ command: string }> }).__foundryCalls.filter((call) => call.command === "model_load"))).toEqual([]);
});

test("a failed project list is retryable and is not reported as an empty collection", async ({ page }) => {
  await installFoundryMock(page, { projects: { entries: [{ id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available" }], failures: { projects_list: ["ProjectStorage"] } } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("Could not update projects");
  await expect(page.getByText("No projects yet", { exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "Retry projects", exact: true }).click();
  await expect(page.getByRole("button", { name: /Research.*Users/ })).toBeVisible();
});

test("move cancellation and backend conflict preserve scope and keyboard focus", async ({ page }) => {
  await installFoundryMock(page, { projects: { entries: [{ id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available" }], conversations: [{ title: "Field notes", projectId: 21, content: "Keep the source context" }], moves: ["cancel"], failures: { projects_move_conversation: ["ProjectConflict"] } } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: /Research.*Users/ }).click();
  const actions = page.getByText("Conversation actions", { exact: true });
  await actions.focus();
  await page.keyboard.press("Enter");
  await page.getByRole("button", { name: "Move conversation", exact: true }).focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("combobox", { name: "Destination" })).toBeFocused();
  await page.getByRole("dialog").getByRole("button", { name: "Move conversation", exact: true }).click();
  await expect(page.getByRole("dialog").getByRole("alert")).toContainText("already bound to another project");
  await expect(page.getByLabel("Project folder")).toHaveText("/Users/demo/Research");
  await page.getByRole("dialog").getByRole("button", { name: "Move conversation", exact: true }).click();
  await expect(page.getByLabel("Project folder")).toHaveText("/Users/demo/Research");
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(actions).toBeFocused();
});

test("active generation blocks context mutations but keeps other histories readable", async ({ page }) => {
  await installFoundryMock(page, { projects: { entries: [{ id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available" }], conversations: [{ title: "Reference notes", projectId: 21, content: "Read this while another conversation generates" }, { title: "Work in progress", projectId: 21 }], holdGeneration: true } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: /Research.*Users/ }).click();
  await page.getByPlaceholder(/Message /).fill("Keep generating");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByRole("button", { name: "Remove project", exact: true })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Rename project", exact: true })).toBeDisabled();
  await page.getByRole("button", { name: /Reference notes.*qwen/ }).click();
  await expect(page.getByRole("heading", { name: "Reference notes", level: 1 })).toBeVisible();
  await expect(page.getByText("Read this while another conversation generates", { exact: true })).toBeVisible();
  await page.getByText("Conversation actions", { exact: true }).click();
  await expect(page.getByRole("button", { name: "Move conversation", exact: true })).toBeDisabled();
});

test("global message search opens the conversation in its actual project scope", async ({ page }) => {
  await installFoundryMock(page, { projects: { entries: [{ id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available" }], conversations: [{ title: "Project evidence", projectId: 21, content: "Evidence from another project" }, { title: "Independent conversation", projectId: null }] } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Search messages", exact: true }).click();
  await page.getByRole("radio", { name: "All conversations", exact: true }).click();
  await page.getByRole("textbox", { name: "Search messages", exact: true }).fill("Evidence");
  await page.getByRole("search").getByRole("button", { name: /Project evidence.*Evidence from another project/ }).click();
  await expect(page.getByRole("heading", { name: "Project evidence", level: 1 })).toBeVisible();
  await expect(page.getByLabel("Project folder")).toHaveText("/Users/demo/Research");
  await page.getByRole("button", { name: "Close search", exact: true }).click();
  await expect(page.getByText("Evidence from another project", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: /Independent conversation/ })).toHaveCount(0);
});

test("project navigation, long names and rename dialogs work by keyboard at the desktop minimum", async ({ page }) => {
  const name = "Research notes for the international accessibility and language team";
  const folderPath = "/Users/demo/Work/International projects/Research notes with a deliberately long folder name";
  await page.setViewportSize({ width: 960, height: 640 });
  await installFoundryMock(page, { projects: { entries: [{ id: 21, name, folderPath, bindingRevision: 1, availability: "available" }] } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).focus();
  await page.keyboard.press("Enter");
  await page.getByRole("button", { name: new RegExp(name) }).focus();
  await page.keyboard.press("Enter");
  await expect(page.getByLabel("Project folder")).toHaveAttribute("title", folderPath);
  const rename = page.getByRole("button", { name: "Rename project", exact: true });
  await rename.focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("textbox", { name: "Project name" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(rename).toBeFocused();
  await expect(page.getByRole("heading", { name, level: 1 })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(960);
});

test("busy and stale rename errors are localized without optimistic project mutations", async ({ page }) => {
  await installFoundryMock(page, { projects: { entries: [{ id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available" }], failures: { projects_rename: ["ProjectBusy", "ProjectStaleContext"] } } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: /Research.*Users/ }).click();
  await page.getByRole("button", { name: "Rename project", exact: true }).click();
  await page.getByRole("textbox", { name: "Project name" }).fill("Field notes");
  await page.getByRole("textbox", { name: "Project name" }).press("Enter");
  await expect(page.getByRole("dialog").getByRole("alert")).toContainText("An execution is active");
  await expect(page.getByRole("heading", { name: "Research", level: 1 })).toBeVisible();
  await page.getByRole("textbox", { name: "Project name" }).press("Enter");
  await expect(page.getByRole("dialog").getByRole("alert")).toContainText("The context changed");
  await expect(page.getByLabel("Project folder")).toHaveText("/Users/demo/Research");
  await page.getByRole("textbox", { name: "Project name" }).press("Enter");
  await expect(page.getByRole("heading", { name: "Field notes", level: 1 })).toBeVisible();
});

for (const [availability, label] of [["missing", "Folder not found"], ["changed", "Folder identity changed"], ["denied", "Folder access denied"], ["unsupported", "Folder not supported"]] as const) {
  test(`${availability} folder preserves history and relocates only after a committed native selection`, async ({ page }) => {
    const project = { id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability };
    await installFoundryMock(page, { onboarding: true, projects: { entries: [project], conversations: [{ title: "Field notes", projectId: 21, content: "History is still readable" }], relocations: [null, { ...project, folderPath: "/Users/demo/Recovered", bindingRevision: 2, availability: "available" }] } });
    await openProjectTestSession(page);
    await page.getByRole("button", { name: "Projects", exact: true }).click();
    await page.getByRole("button", { name: /Research.*Users/ }).click();
    await expect(page.getByText(label, { exact: true })).toBeVisible();
    await expect(page.getByText("History is still readable", { exact: true })).toBeVisible();
    await page.getByRole("button", { name: "Locate folder", exact: true }).click();
    await expect(page.getByLabel("Project folder")).toHaveText("/Users/demo/Research");
    await page.getByRole("button", { name: "Locate folder", exact: true }).click();
    await expect(page.getByLabel("Project folder")).toHaveText("/Users/demo/Recovered");
    await expect(page.getByText(label, { exact: true })).toHaveCount(0);
    await expect(page.getByText("History is still readable", { exact: true })).toBeVisible();
  });
}

for (const outcome of ["keep", "delete", "cancel"] as const) {
  test(`native project removal ${outcome} applies only its committed receipt`, async ({ page }) => {
    await installFoundryMock(page, { onboarding: true, projects: { entries: [{ id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available" }], conversations: [{ title: "Field notes", projectId: 21, content: "Keep the project history" }], removals: [outcome] } });
    await openProjectTestSession(page);
    await page.getByRole("button", { name: "Projects", exact: true }).click();
    await page.getByRole("button", { name: /Research.*Users/ }).click();
    await page.getByRole("button", { name: "Remove project", exact: true }).click();
    if (outcome === "cancel") {
      await expect(page.getByLabel("Project folder")).toHaveText("/Users/demo/Research");
      await expect(page.getByText("Keep the project history", { exact: true })).toBeVisible();
    } else {
      await expect(page.getByText("The folder and its files were not deleted.", { exact: true })).toBeVisible();
      await expect(page.getByLabel("Project folder")).toHaveCount(0);
      if (outcome === "keep") {
        await expect(page.getByRole("heading", { name: "Field notes", level: 1 })).toBeVisible();
        await expect(page.getByText("Keep the project history", { exact: true })).toBeVisible();
      } else {
        await expect(page.getByText("Keep the project history", { exact: true })).toHaveCount(0);
      }
      await page.getByRole("button", { name: "Projects", exact: true }).click();
      await expect(page.getByText("No projects yet", { exact: true })).toBeVisible();
    }
  });
}

test("moving a conversation between projects and out preserves its readable history", async ({ page }) => {
  await installFoundryMock(page, { projects: { entries: [
    { id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available" },
    { id: 22, name: "Design", folderPath: "/Users/demo/Design", bindingRevision: 1, availability: "available" },
  ], conversations: [{ title: "Field notes", projectId: 21, content: "Keep the original research history" }] } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: /Research.*Users/ }).click();
  await page.getByText("Conversation actions", { exact: true }).click();
  await page.getByRole("button", { name: "Move conversation", exact: true }).click();
  await expect(page.getByRole("dialog")).toContainText("History and attachments retain information from the previous context");
  await page.getByRole("combobox", { name: "Destination" }).selectOption("22");
  await page.getByRole("button", { name: "Move conversation", exact: true }).last().click();
  await expect(page.getByLabel("Project folder")).toHaveText("/Users/demo/Design");
  await expect(page.getByText("Keep the original research history", { exact: true })).toBeVisible();
  await page.getByText("Conversation actions", { exact: true }).click();
  await page.getByRole("button", { name: "Move conversation", exact: true }).click();
  await page.getByRole("combobox", { name: "Destination" }).selectOption("none");
  await page.getByRole("button", { name: "Move conversation", exact: true }).last().click();
  await expect(page.getByLabel("Project folder")).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "Field notes", level: 1 })).toBeVisible();
  await expect(page.getByText("Keep the original research history", { exact: true })).toBeVisible();
});

test("renaming a project changes its display name, never the bound folder", async ({ page }) => {
  await installFoundryMock(page, { onboarding: true, projects: { entries: [{ id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available" }] } });
  await openProjectTestSession(page);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: /Research.*Users/ }).click();
  await page.getByRole("button", { name: "Rename project", exact: true }).click();
  await expect(page.getByRole("dialog", { name: "Rename project" })).toBeVisible();
  await page.getByRole("textbox", { name: "Project name" }).fill("Field notes");
  await page.getByRole("textbox", { name: "Project name" }).press("Enter");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.getByRole("heading", { name: "Field notes", level: 1 })).toBeVisible();
  await expect(page.getByLabel("Project folder")).toHaveText("/Users/demo/Research");
});

test("new and duplicate folders open their scoped conversations without mixing histories", async ({ page }) => {
  const project = { id: 21, name: "Research", folderPath: "/Users/demo/Research", bindingRevision: 1, availability: "available" as const };
  await installFoundryMock(page, { projects: { entries: [project], conversations: [{ title: "Independent conversation", projectId: null }, { title: "Research notes", projectId: 21, content: "Project history remains readable" }], picks: [{ ...project, created: false }, { id: 22, name: "Design", folderPath: "/Users/demo/Design", bindingRevision: 1, availability: "available", created: true }] } });
  await openProjectTestSession(page);
  await expect(page.getByRole("heading", { name: "Independent conversation" })).toBeVisible();
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: "New project", exact: true }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Open existing project", exact: true }).click();
  await expect(page.getByRole("status").filter({ hasText: "This folder already belongs to a project" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Research notes" })).toBeVisible();
  await expect(page.getByText("Project history remains readable", { exact: true })).toBeVisible();
  await expect(page.getByLabel("Project folder")).toHaveText("/Users/demo/Research");
  await page.getByRole("button", { name: "New conversation", exact: true }).click();
  await expect(page.getByText("Review changes", { exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Untitled conversation", level: 1 })).toBeVisible();
  await page.getByRole("button", { name: "Conversations", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Independent conversation" })).toBeVisible();
  await expect(page.getByRole("button", { name: /Research notes/ })).toHaveCount(0);
  await page.getByRole("button", { name: "Projects", exact: true }).click();
  await page.getByRole("button", { name: "New project", exact: true }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Create project", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Design", exact: true, level: 1 })).toBeVisible();
  await expect(page.getByText("No conversations yet", { exact: true })).toBeVisible();
});
