function isApplePlatform() {
  const maybeUserAgentData = navigator as Navigator & {
    userAgentData?: {
      platform?: string;
    };
  };
  const platform = (maybeUserAgentData.userAgentData?.platform ?? navigator.platform ?? navigator.userAgent).toLowerCase();

  return /mac|iphone|ipad|ipod/.test(platform);
}

function isWindowsPlatform() {
  const maybeUserAgentData = navigator as Navigator & {
    userAgentData?: {
      platform?: string;
    };
  };
  const platform = (maybeUserAgentData.userAgentData?.platform ?? navigator.platform ?? navigator.userAgent).toLowerCase();

  return /win/.test(platform);
}

function isLinuxPlatform() {
  const maybeUserAgentData = navigator as Navigator & {
    userAgentData?: {
      platform?: string;
    };
  };
  const platform = (maybeUserAgentData.userAgentData?.platform ?? navigator.platform ?? navigator.userAgent).toLowerCase();

  return /linux/.test(platform);
}

export function primaryShortcutModifier() {
  if (isApplePlatform()) {
    return "⌘";
  }

  if (isLinuxPlatform()) {
    return "Super";
  }

  if (isWindowsPlatform()) {
    return "Ctrl";
  }

  return "Ctrl";
}

export function shiftShortcutModifier() {
  return isApplePlatform() ? "⇧" : "Shift";
}

export function dictationShortcutComboLabel() {
  return `${primaryShortcutModifier()}+${shiftShortcutModifier()}+M`;
}
