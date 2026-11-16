const available = (() => {
  try {
    const probe = "foundryui.probe";
    window.localStorage.setItem(probe, "1");
    window.localStorage.removeItem(probe);
    return true;
  } catch {
    return false;
  }
})();

export function readStored(key: string): string | null {
  if (!available) {
    return null;
  }

  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function writeStored(key: string, value: string) {
  if (!available) {
    return;
  }

  try {
    window.localStorage.setItem(key, value);
  } catch {
    return;
  }
}
