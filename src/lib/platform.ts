export type AppPlatform = "macos" | "windows" | "linux" | "unknown";

function platformText() {
  const maybeUserAgentData = navigator as Navigator & {
    userAgentData?: {
      platform?: string;
    };
  };

  return (maybeUserAgentData.userAgentData?.platform ?? navigator.platform ?? navigator.userAgent).toLowerCase();
}

export function detectAppPlatform(): AppPlatform {
  const platform = platformText();

  if (/mac|iphone|ipad|ipod/.test(platform)) {
    return "macos";
  }

  if (/win/.test(platform)) {
    return "windows";
  }

  if (/linux/.test(platform)) {
    return "linux";
  }

  return "unknown";
}
