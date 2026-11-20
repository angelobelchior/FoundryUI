import { expect, test, type Page } from "@playwright/test";
import { readFile } from "node:fs/promises";
import { resolve, extname } from "node:path";
import { installFoundryMock } from "../smoke/helpers/installFoundryMock";

async function setup(page: Page, failSave = false) {
  const root = resolve("dist");
  await page.route("http://foundry.test/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    const file = resolve(root, path === "/" ? "index.html" : `.${path}`);
    if (!file.startsWith(`${root}/`)) return route.abort();
    const types: Record<string, string> = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".svg": "image/svg+xml", ".woff2": "font/woff2", ".png": "image/png" };
    try { await route.fulfill({ body: await readFile(file), contentType: types[extname(file)] ?? "application/octet-stream" }); }
    catch { await route.fulfill({ status: 404, body: "" }); }
  });
  await installFoundryMock(page, { language: "en", workspace: "chat", projects: { conversations: [
    { title: "First conversation", projectId: null }, { title: "Second conversation", projectId: null },
  ] } });
  await page.addInitScript((failSave) => {
    localStorage.setItem("foundryui.onboarding.usage.completed", "1");
    localStorage.setItem("foundryui.onboarding.usage.neverShow", "1");
    type Payload = Record<string, unknown>;
    const host = window as unknown as { __TAURI_INTERNALS__: { invoke: (command: string, payload?: Payload) => Promise<unknown> }; __accessCalls: { command: string; payload: Payload }[]; __accessCopied: string };
    const original = host.__TAURI_INTERNALS__.invoke;
    host.__accessCalls = [];
    host.__accessCopied = "";
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async (value: string) => { host.__accessCopied = value; } } });
    host.__TAURI_INTERNALS__.invoke = async (command, payload = {}) => {
      if (command.startsWith("access_")) host.__accessCalls.push({ command, payload });
      if (command === "agent_execution_status") return { conversationId: payload.conversationId, mode: "review", contextRevision: 1, authorizationRevision: 0, modeRevision: 0, execution: null, blocker: null, blockerTitle: null, pendingRequests: [], continuationAvailable: false };
      if (command === "access_read_preference") return { allowed: localStorage.getItem("synthetic.readAllowed") !== "false" };
      if (command === "access_set_read_preference") {
        if (failSave) throw new Error("synthetic-save-failure");
        localStorage.setItem("synthetic.readAllowed", String(payload.allowed));
        return { allowed: payload.allowed };
      }
      if (command === "access_network_log") {
        if (payload.conversationId !== 1) return { entries: [], hasMore: false, nextBefore: null };
        const states = payload.beforeId ? ["blocked", "redirected", "completed", "failed", "cancelled"] : ["attempted", "requested"];
        return { entries: states.map((state, index) => ({ id: (payload.beforeId ? 5 : 7) - index, runId: 31, actionId: 41, destination: "https://synthetic-user:synthetic-password@example.invalid/private-source?token=synthetic-token#synthetic-fragment", state, httpStatus: state === "completed" ? 200 : null, createdAt: 1795046400000 })), hasMore: !payload.beforeId, nextBefore: payload.beforeId ? null : 6 };
      }
      return original(command, payload);
    };
  }, failSave);
  await page.goto("http://foundry.test/");
  await page.getByRole("button", { name: /First conversation.*qwen/ }).click();
  await page.getByRole("button", { name: "Tools", exact: true }).click();
  return page.getByRole("tabpanel", { name: "Reading and network" });
}

test("the global preference defaults on and persists across conversations and reload", async ({ page }) => {
  let panel = await setup(page);
  const preference = page.getByRole("checkbox", { name: "Allow new file reads in all conversations" });
  await expect(preference).toBeChecked();
  await expect(panel.getByText(/sensitive, hidden and Git-ignored/)).toBeVisible();
  await expect(panel.getByText(/does not erase content/)).toBeVisible();
  await preference.uncheck();
  await expect(preference).not.toBeChecked();
  await page.screenshot({ path: "checks/wave11/read-network/global-reading-off-1440.png" });
  await page.getByRole("button", { name: /Second conversation.*qwen/ }).click();
  await page.getByRole("button", { name: "Tools", exact: true }).click();
  await expect(preference).not.toBeChecked();
  panel = page.getByRole("tabpanel", { name: "Reading and network" });
  await expect(panel.getByText("No recorded network access in this conversation.")).toBeVisible();
  await page.reload();
  await page.getByRole("button", { name: /First conversation.*qwen/ }).click();
  await page.getByRole("button", { name: "Tools", exact: true }).click();
  await expect(preference).not.toBeChecked();
});

test("network phases are distinct and only sanitized destinations can be copied", async ({ page }) => {
  const panel = await setup(page);
  await expect(panel.getByText("Attempted; outcome not confirmed", { exact: true })).toBeVisible();
  await expect(panel.getByText("Requested; no access confirmed", { exact: true })).toBeVisible();
  await expect(panel.getByText("Completed", { exact: true })).toHaveCount(0);
  await panel.getByRole("button", { name: "Load older records" }).click();
  for (const state of ["Blocked or denied", "Redirect refused; target not accessed", "Completed", "Failed", "Cancelled; completion not confirmed"]) {
    await expect(panel.getByText(state, { exact: true })).toBeAttached();
  }
  await panel.getByRole("button", { name: "Copy sanitized destination" }).first().click();
  await expect(panel.getByText("Sanitized destination copied.")).toBeAttached();
  const copied = await page.evaluate(() => (window as unknown as { __accessCopied: string }).__accessCopied);
  expect(copied).toBe("https://example.invalid/[redacted]");
  const rendered = await panel.innerHTML();
  for (const secret of ["synthetic-user", "synthetic-password", "private-source", "synthetic-token", "synthetic-fragment"]) expect(rendered).not.toContain(secret);
  expect(await panel.locator("a").count()).toBe(0);
  await page.screenshot({ path: "checks/wave11/read-network/history-1440.png" });
  await page.setViewportSize({ width: 960, height: 900 });
  await page.screenshot({ path: "checks/wave11/read-network/history-960.png" });
});

test("a failed preference change is not presented as saved", async ({ page }) => {
  const panel = await setup(page, true);
  const preference = panel.getByRole("checkbox", { name: "Allow new file reads in all conversations" });
  await expect(preference).toBeChecked();
  await preference.click();
  await expect(panel.getByRole("alert")).toHaveText(/Could not update reading preference/);
  await expect(preference).toBeChecked();
  await panel.getByRole("button", { name: "Retry", exact: true }).click();
  await expect(preference).toBeChecked();
  const writes = await page.evaluate(() => (window as unknown as { __accessCalls: { command: string; payload: Record<string, unknown> }[] }).__accessCalls.filter((call) => call.command === "access_set_read_preference"));
  expect(writes).toHaveLength(1);
  expect(writes[0]?.payload).toEqual({ allowed: false });
});
