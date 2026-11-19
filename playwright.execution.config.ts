import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/execution",
  timeout: 30_000,
  workers: 1,
  use: { viewport: { width: 1440, height: 1000 }, trace: "retain-on-failure" },
});
