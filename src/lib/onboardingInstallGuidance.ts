import { readStored, writeStored } from "./storage";
import { detectAppPlatform, type AppPlatform } from "./platform";

const installConfigUrl = "https://raw.githubusercontent.com/angelobelchior/FoundryUI/main/config/foundry-install.json";
const cacheValueKey = "foundryui.onboarding.installGuidance.cache";
const cacheUpdatedAtKey = "foundryui.onboarding.installGuidance.updatedAt";
const cacheTtlMs = 24 * 60 * 60 * 1000;

type InstallPlatformConfig = {
  command: string | null;
  installerUrl: string;
  docsUrl: string;
};

type InstallConfig = {
  version: number;
  platforms: Partial<Record<AppPlatform, InstallPlatformConfig>>;
};

export type InstallGuidance = InstallPlatformConfig & {
  platform: AppPlatform;
  source: "remote" | "cache" | "fallback";
};

function defaultConfig(): InstallConfig {
  return {
    version: 1,
    platforms: {
      macos: {
        command: "brew tap microsoft/foundrylocal && brew install foundrylocal",
        installerUrl: "https://aka.ms/foundry-local-installer",
        docsUrl: "https://learn.microsoft.com/en-us/azure/foundry-local/how-to/how-to-use-foundry-local-cli",
      },
      windows: {
        command: "winget install Microsoft.FoundryLocal",
        installerUrl: "https://aka.ms/foundry-local-installer",
        docsUrl: "https://learn.microsoft.com/en-us/azure/foundry-local/how-to/how-to-use-foundry-local-cli",
      },
      linux: {
        command: null,
        installerUrl: "https://aka.ms/foundry-local-installer",
        docsUrl: "https://learn.microsoft.com/en-us/azure/foundry-local/how-to/how-to-use-foundry-local-cli",
      },
      unknown: {
        command: null,
        installerUrl: "https://aka.ms/foundry-local-installer",
        docsUrl: "https://learn.microsoft.com/en-us/azure/foundry-local/how-to/how-to-use-foundry-local-cli",
      },
    },
  };
}

function isHttpsUrl(value: unknown) {
  if (typeof value !== "string" || value.trim().length === 0) {
    return false;
  }

  try {
    return new URL(value).protocol === "https:";
  } catch {
    return false;
  }
}

function parsePlatformConfig(value: unknown): InstallPlatformConfig | null {
  if (!value || typeof value !== "object") {
    return null;
  }

  const typed = value as Record<string, unknown>;

  const command = typed.command;
  const installerUrl = typed.installerUrl;
  const docsUrl = typed.docsUrl;

  if (!(command === null || typeof command === "string")) {
    return null;
  }

  if (!isHttpsUrl(installerUrl) || !isHttpsUrl(docsUrl)) {
    return null;
  }

  return {
    command: typeof command === "string" ? command : null,
    installerUrl: installerUrl as string,
    docsUrl: docsUrl as string,
  };
}

function parseInstallConfig(value: unknown): InstallConfig | null {
  if (!value || typeof value !== "object") {
    return null;
  }

  const typed = value as Record<string, unknown>;

  if (typed.version !== 1) {
    return null;
  }

  if (!typed.platforms || typeof typed.platforms !== "object") {
    return null;
  }

  const platformsValue = typed.platforms as Record<string, unknown>;
  const next: Partial<Record<AppPlatform, InstallPlatformConfig>> = {};

  for (const key of ["macos", "windows", "linux", "unknown"] as const) {
    const parsed = parsePlatformConfig(platformsValue[key]);

    if (parsed) {
      next[key] = parsed;
    }
  }

  return Object.keys(next).length > 0
    ? {
        version: 1,
        platforms: next,
      }
    : null;
}

function readCachedConfig() {
  const raw = readStored(cacheValueKey);
  const updatedAt = Number(readStored(cacheUpdatedAtKey));

  if (!raw) {
    return null;
  }

  try {
    const parsed = parseInstallConfig(JSON.parse(raw));

    if (!parsed) {
      return null;
    }

    return {
      config: parsed,
      updatedAt: Number.isFinite(updatedAt) ? updatedAt : 0,
    };
  } catch {
    return null;
  }
}

function writeCachedConfig(config: InstallConfig) {
  writeStored(cacheValueKey, JSON.stringify(config));
  writeStored(cacheUpdatedAtKey, String(Date.now()));
}

async function fetchRemoteConfig() {
  const allowedHost = "raw.githubusercontent.com";
  const url = new URL(installConfigUrl);

  if (url.protocol !== "https:" || url.host !== allowedHost) {
    return null;
  }

  const controller = new AbortController();
  const timeout = window.setTimeout(() => controller.abort(), 3000);

  try {
    const response = await fetch(installConfigUrl, {
      method: "GET",
      cache: "no-store",
      signal: controller.signal,
    });

    if (!response.ok) {
      return null;
    }

    const body = (await response.json()) as unknown;

    return parseInstallConfig(body);
  } catch {
    return null;
  } finally {
    window.clearTimeout(timeout);
  }
}

function guidanceFrom(config: InstallConfig, source: InstallGuidance["source"], platform: AppPlatform): InstallGuidance {
  const platformConfig = config.platforms[platform] ?? config.platforms.unknown ?? defaultConfig().platforms.unknown;

  return {
    platform,
    command: platformConfig?.command ?? null,
    installerUrl: platformConfig?.installerUrl ?? "https://aka.ms/foundry-local-installer",
    docsUrl: platformConfig?.docsUrl ?? "https://learn.microsoft.com/en-us/azure/foundry-local/how-to/how-to-use-foundry-local-cli",
    source,
  };
}

export function fallbackInstallGuidance(platform = detectAppPlatform()) {
  return guidanceFrom(defaultConfig(), "fallback", platform);
}

export async function loadInstallGuidance(options?: { forceRefresh?: boolean; platform?: AppPlatform }) {
  const platform = options?.platform ?? detectAppPlatform();
  const forceRefresh = Boolean(options?.forceRefresh);
  const cached = readCachedConfig();
  const now = Date.now();

  if (!forceRefresh && cached && now - cached.updatedAt <= cacheTtlMs) {
    return guidanceFrom(cached.config, "cache", platform);
  }

  const remote = await fetchRemoteConfig();

  if (remote) {
    writeCachedConfig(remote);
    return guidanceFrom(remote, "remote", platform);
  }

  if (cached) {
    return guidanceFrom(cached.config, "cache", platform);
  }

  return fallbackInstallGuidance(platform);
}
