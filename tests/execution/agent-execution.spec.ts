import { expect, test, type Page } from "@playwright/test";
import { readFile } from "node:fs/promises";
import { resolve, extname } from "node:path";
import { installFoundryMock } from "../smoke/helpers/installFoundryMock";

async function setup(page: Page, scenario: "idle" | "pending" | "blocker" | "uncertain" | "incompatible" | "approval" | "opaque" | "native-cancel" = "idle") {
  const root = resolve("dist");
  await page.route("http://foundry.test/**", async (route) => {
    const path = new URL(route.request().url()).pathname;
    const file = resolve(root, path === "/" ? "index.html" : `.${path}`);
    if (!file.startsWith(`${root}/`)) return route.abort();
    const types: Record<string, string> = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".svg": "image/svg+xml", ".woff2": "font/woff2", ".png": "image/png" };
    try {
      await route.fulfill({ body: await readFile(file), contentType: types[extname(file)] ?? "application/octet-stream" });
    } catch { await route.fulfill({ status: 404, body: "" }); }
  });
  await installFoundryMock(page, { language: "en", workspace: "chat" });
  await page.addInitScript((scenario) => {
    localStorage.setItem("foundryui.onboarding.usage.completed", "1");
    localStorage.setItem("foundryui.onboarding.usage.neverShow", "1");
    type Payload = Record<string, unknown>;
    const host = window as unknown as { __TAURI_INTERNALS__: { invoke: (command: string, payload?: Payload) => Promise<unknown> }; __executionCalls: { command: string; payload: Payload }[] };
    const original = host.__TAURI_INTERNALS__.invoke;
    const calls: typeof host.__executionCalls = [];
    host.__executionCalls = calls;
    let mode = "review";
    let cancelled = false;
    let started = false;
    let decided = false;
    const liveApproval = ["approval", "opaque", "native-cancel"].includes(scenario);
    host.__TAURI_INTERNALS__.invoke = async (command, payload = {}) => {
      if (command.startsWith("agent_") || command === "authorization_decide") calls.push({ command, payload });
      if (command === "agent_execution_status") return {
        conversationId: payload.conversationId, mode, contextRevision: 1, authorizationRevision: 0, modeRevision: mode === "review" ? 0 : 1,
        execution: scenario === "pending" || scenario === "uncertain" || started || liveApproval ? {
          run: { id: 41, conversation_id: payload.conversationId, request_key: "fixture", state: cancelled || scenario === "uncertain" ? "cancelled" : started || decided ? "complete" : "prepared" },
          writerRemaining: scenario === "uncertain",
          events: [{ sequence: 10, event: "run_prepared", actionId: null }, { sequence: 12, event: cancelled ? "run_cancelled" : started || decided ? "run_completed" : "authorization_pending", actionId: 7 }],
        } : null,
        blocker: scenario === "blocker" ? { run_id: 99, conversation_id: 22, state: "running" } : null,
        blockerTitle: scenario === "blocker" ? "Other task" : null,
        continuationAvailable: liveApproval,
        pendingRequests: liveApproval && !decided ? [{
          id: 71, conversationId: payload.conversationId, actionId: 7, runId: 41, contextRevision: 1, authorizationRevision: 0, modeRevision: 0, status: "pending", remembered: false,
          requirements: [{ kind: "standalone_root", target: "Synthetic authorized folder", constraintsFingerprint: "a".repeat(64), rememberEligible: true, recoveryGuaranteed: false, previewAvailable: scenario !== "opaque" }],
        }] : [],
      };
      if (command === "authorization_decide") {
        if (scenario === "native-cancel") return null;
        decided = true;
        return { requestId: 71, status: "ready", grantId: null, dispatchStarted: false };
      }
      if (command === "agent_execution_mode") { mode = String(payload.mode); return null; }
      if (command === "agent_run_cancel") {
        if (scenario === "blocker") throw new Error("termination failed");
        cancelled = true;
        return { writer_remaining: false };
      }
      if (command === "agent_run_start") {
        started = true;
        const now = Date.now();
        const conversation = { id: payload.conversationId, title: "Agent task", modelAlias: "qwen3-0.6b", modelVariantId: null, createdAt: now, updatedAt: now, favorite: false, archived: false };
        const message = { conversationId: payload.conversationId, modelAlias: "qwen3-0.6b", reasoning: "", params: null, usage: null, error: null, status: "complete", attachments: [], createdAt: now, updatedAt: now };
        return { requestId: "fixture", conversation, userMessage: { ...message, id: 501, seq: 1, role: "user", content: payload.content }, assistantMessage: { ...message, id: 502, seq: 2, role: "assistant", content: "Tools reply" }, removed: [] };
      }
      const result = await original(command, payload);
      if (scenario === "incompatible" && command === "workspace_snapshot") {
        const snapshot = result as { catalog?: { supportsToolCalling: boolean }[] };
        snapshot.catalog?.forEach((model) => { model.supportsToolCalling = false; });
      }
      return result;
    };
  }, scenario);
  await page.goto("http://foundry.test/");
  await page.getByRole("button", { name: "New conversation", exact: true }).click();
  await expect(page.getByRole("region", { name: "Agent execution" })).toBeVisible();
}

async function mutations(page: Page) {
  return page.evaluate(() => (window as unknown as { __executionCalls: { command: string; payload: Record<string, unknown> }[] }).__executionCalls.filter((call) => call.command !== "agent_execution_status"));
}

test("review mode persists without authorizing or cancelling anything", async ({ page }) => {
  await setup(page);
  const panel = page.getByRole("region", { name: "Agent execution" });
  await expect(panel.getByRole("combobox")).toHaveValue("review");
  await panel.getByRole("combobox").selectOption("automatic");
  await expect(panel.getByRole("combobox")).toHaveValue("automatic");
  const calls = await mutations(page);
  expect(calls.map((call) => call.command)).toEqual(["agent_execution_mode"]);
  expect(calls[0]?.payload).toMatchObject({ contextRevision: 1, modeRevision: 0, mode: "automatic" });
  await page.screenshot({ path: "checks/wave10/execution/ready-1440.png" });
  await page.setViewportSize({ width: 960, height: 900 });
  await expect(panel.getByRole("combobox")).toBeVisible();
  await page.screenshot({ path: "checks/wave10/execution/ready-960.png" });
});

test("pending execution restores ordered history and stops only the selected run", async ({ page }) => {
  await setup(page, "pending");
  const panel = page.getByRole("region", { name: "Agent execution" });
  await expect(panel.getByText(/no longer has a live continuation/)).toBeVisible();
  await panel.getByText(/Execution history/).click();
  await expect(panel.getByRole("listitem")).toHaveCount(2);
  await page.reload();
  await page.getByRole("button", { name: "New conversation", exact: true }).click();
  await expect(panel.getByText(/no longer has a live continuation/)).toBeVisible();
  await panel.getByRole("button", { name: "Stop run", exact: true }).click();
  await expect(panel.getByRole("status")).toHaveText("Cancelled");
  const calls = await mutations(page);
  expect(calls.filter((call) => call.command === "agent_run_cancel")).toHaveLength(1);
  expect(calls.find((call) => call.command === "agent_run_cancel")?.payload.runId).toBe(41);
});

test("another conversation requires confirmation and failed termination stays visible", async ({ page }) => {
  await setup(page, "blocker");
  const panel = page.getByRole("region", { name: "Agent execution" });
  await expect(panel.getByText(/Resources occupied by Other task/)).toBeVisible();
  await panel.getByRole("button", { name: "Wait", exact: true }).click();
  expect(await mutations(page)).toEqual([]);
  await panel.getByRole("button", { name: "Request interruption" }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByText(/Completed effects will not be undone/)).toBeVisible();
  await dialog.getByRole("button", { name: "Stop run", exact: true }).click();
  await expect(page.getByRole("alert").filter({ hasText: /Could not update execution state/ })).toBeVisible();
  await expect(dialog).toBeVisible();
  expect((await mutations(page))[0]?.payload).toEqual({ conversationId: 22, runId: 99 });
  await page.screenshot({ path: "checks/wave10/execution/interruption-failed.png" });
});

test("uncertain effects keep the writer lock visible", async ({ page }) => {
  await setup(page, "uncertain");
  const panel = page.getByRole("region", { name: "Agent execution" });
  await expect(panel.getByText(/Resources remain locked/)).toBeVisible();
  await expect(panel.getByRole("checkbox", { name: "Use tools" })).toBeDisabled();
  await expect(panel.getByRole("status").first()).toHaveText("Effects uncertain");
});

test("explicit tool opt-in uses the agent command, regular chat stays default", async ({ page }) => {
  await setup(page);
  const checkbox = page.getByRole("checkbox", { name: "Use tools" });
  await expect(checkbox).not.toBeChecked();
  await checkbox.check();
  await page.getByPlaceholder(/Message /).fill("Read the project");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByText("Tools reply").first()).toBeVisible();
  expect((await mutations(page)).some((call) => call.command === "agent_run_start")).toBe(true);
});

test("an incompatible model keeps regular chat available", async ({ page }) => {
  await setup(page, "incompatible");
  const panel = page.getByRole("region", { name: "Agent execution" });
  await expect(panel.getByRole("checkbox", { name: "Use tools" })).toBeDisabled();
  await expect(panel.getByText(/You can continue chatting/)).toBeVisible();
  await page.getByPlaceholder(/Message /).fill("Hello without tools");
  await page.getByRole("button", { name: "Send", exact: true }).click();
  await expect(page.getByText("Mock reply to: Hello without tools").first()).toBeVisible();
  expect((await mutations(page)).some((call) => call.command === "agent_run_start")).toBe(false);
});

test("a live pending run uses the existing native approval command without restarting", async ({ page }) => {
  await setup(page, "approval");
  const panel = page.getByRole("region", { name: "Agent execution" });
  await expect(panel.getByText(/same run continues only/)).toBeVisible();
  await panel.getByRole("button", { name: "Review and allow once" }).click();
  await expect(panel.getByRole("status")).toHaveText("Completed");
  const calls = await mutations(page);
  expect(calls.map((call) => call.command)).toEqual(["authorization_decide"]);
  expect(calls[0]?.payload).toMatchObject({ requestId: 71, decision: { decision: "approve", remember: false } });
});

test("incomplete previews cannot be approved but can be denied", async ({ page }) => {
  await setup(page, "opaque");
  const panel = page.getByRole("region", { name: "Agent execution" });
  await expect(panel.getByRole("button", { name: "Review and allow once" })).toBeDisabled();
  await expect(panel.getByText(/complete preview is unavailable/)).toBeVisible();
  await page.screenshot({ path: "checks/wave10/approvals/incomplete-preview-1440.png" });
  await panel.getByRole("button", { name: "Deny", exact: true }).click();
  await expect(panel.getByRole("status")).toHaveText("Completed");
  expect((await mutations(page))[0]?.payload.decision).toEqual({ decision: "deny" });
});

test("cancelling native confirmation leaves the same request pending", async ({ page }) => {
  await setup(page, "native-cancel");
  const panel = page.getByRole("region", { name: "Agent execution" });
  await panel.getByRole("button", { name: "Review and allow once" }).click();
  await expect(panel.getByRole("status")).toHaveText("Awaiting authorization");
  await expect(panel.getByText("Authorization request 71")).toBeVisible();
  expect((await mutations(page)).map((call) => call.command)).toEqual(["authorization_decide"]);
});

test("changing mode while waiting does not decide or cancel the pending action", async ({ page }) => {
  await setup(page, "approval");
  const panel = page.getByRole("region", { name: "Agent execution" });
  await expect(panel.getByText("Authorization request 71")).toBeVisible();
  await panel.getByRole("combobox").selectOption("automatic");
  await expect(panel.getByRole("combobox")).toHaveValue("automatic");
  expect((await mutations(page)).map((call) => call.command)).toEqual(["agent_execution_mode"]);
});
