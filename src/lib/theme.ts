import { useEffect, useState } from "react";

export const themes = ["system", "light", "dark"] as const;
export type ThemePreference = (typeof themes)[number];
export type ResolvedTheme = "light" | "dark";
export const accents = ["blue", "green", "purple", "orange"] as const;
export type AccentPreference = (typeof accents)[number];
export const densities = ["compact", "normal", "comfortable"] as const;
export type DensityPreference = (typeof densities)[number];
export const typographies = ["system", "humanist", "serif"] as const;
export type TypographyPreference = (typeof typographies)[number];

const darkQuery = "(prefers-color-scheme: dark)";

export function resolveTheme(preference: ThemePreference, systemPrefersDark: boolean): ResolvedTheme {
  if (preference === "system") {
    return systemPrefersDark ? "dark" : "light";
  }

  return preference;
}

export function applyTheme(theme: ResolvedTheme) {
  const root = document.documentElement;
  root.dataset.theme = theme;
  root.style.colorScheme = theme;
}

export function applyAccent(accent: AccentPreference) {
  document.documentElement.dataset.accent = accent;
}

export function applyDensity(density: DensityPreference) {
  document.documentElement.dataset.density = density;
}

export function applyTypography(typography: TypographyPreference) {
  document.documentElement.dataset.typography = typography;
}

export function useResolvedThemeMode(): ResolvedTheme {
  const [mode, setMode] = useState<ResolvedTheme>(() => readThemeMode());

  useEffect(() => {
    const observer = new MutationObserver(() => setMode(readThemeMode()));

    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme"],
    });

    return () => observer.disconnect();
  }, []);

  return mode;
}

function readThemeMode(): ResolvedTheme {
  return document.documentElement.dataset.theme === "dark" ? "dark" : "light";
}

export function useSystemPrefersDark() {
  const [prefersDark, setPrefersDark] = useState(() => window.matchMedia(darkQuery).matches);

  useEffect(() => {
    const query = window.matchMedia(darkQuery);
    const onChange = (event: MediaQueryListEvent) => setPrefersDark(event.matches);

    query.addEventListener("change", onChange);

    return () => query.removeEventListener("change", onChange);
  }, []);

  return prefersDark;
}
