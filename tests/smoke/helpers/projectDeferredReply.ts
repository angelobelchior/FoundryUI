import type { Page } from "@playwright/test";

export async function deferProjectReply(page: Page, command: string, projectId: number | null, occurrence = 1, payloadKey = "projectId") {
  await page.evaluate(({ command, projectId, occurrence, payloadKey }) => {
    const host = window as unknown as {
      __TAURI_INTERNALS__: { invoke: (command: string, payload?: Record<string, unknown>) => Promise<unknown> };
      __projectReplyGate: { entered: boolean; completed: boolean; release: () => void };
    };
    const original = host.__TAURI_INTERNALS__.invoke.bind(host.__TAURI_INTERNALS__);
    let seen = 0;
    let release!: () => void;
    const pending = new Promise<void>((resolve) => { release = resolve; });
    host.__projectReplyGate = { entered: false, completed: false, release };
    host.__TAURI_INTERNALS__.invoke = async (name, payload = {}) => {
      const result = await original(name, payload);
      if (name === command && payload[payloadKey] === projectId && ++seen === occurrence) {
        host.__projectReplyGate.entered = true;
        await pending;
        host.__projectReplyGate.completed = true;
      }
      return result;
    };
  }, { command, projectId, occurrence, payloadKey });
}

export async function projectReplyEntered(page: Page) {
  return page.evaluate(() => (window as unknown as { __projectReplyGate: { entered: boolean } }).__projectReplyGate.entered);
}

export async function releaseProjectReply(page: Page) {
  await page.evaluate(() => (window as unknown as { __projectReplyGate: { release: () => void } }).__projectReplyGate.release());
  await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
}
