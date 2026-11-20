import type { HighlighterCore } from "shiki/core";

const languageLoaders = {
  typescript: () => import("shiki/dist/langs/typescript.mjs"),
  tsx: () => import("shiki/dist/langs/tsx.mjs"),
  javascript: () => import("shiki/dist/langs/javascript.mjs"),
  jsx: () => import("shiki/dist/langs/jsx.mjs"),
  json: () => import("shiki/dist/langs/json.mjs"),
  jsonc: () => import("shiki/dist/langs/jsonc.mjs"),
  html: () => import("shiki/dist/langs/html.mjs"),
  css: () => import("shiki/dist/langs/css.mjs"),
  scss: () => import("shiki/dist/langs/scss.mjs"),
  markdown: () => import("shiki/dist/langs/markdown.mjs"),
  mdx: () => import("shiki/dist/langs/mdx.mjs"),
  shellscript: () => import("shiki/dist/langs/shellscript.mjs"),
  powershell: () => import("shiki/dist/langs/powershell.mjs"),
  python: () => import("shiki/dist/langs/python.mjs"),
  rust: () => import("shiki/dist/langs/rust.mjs"),
  go: () => import("shiki/dist/langs/go.mjs"),
  java: () => import("shiki/dist/langs/java.mjs"),
  kotlin: () => import("shiki/dist/langs/kotlin.mjs"),
  swift: () => import("shiki/dist/langs/swift.mjs"),
  csharp: () => import("shiki/dist/langs/csharp.mjs"),
  sql: () => import("shiki/dist/langs/sql.mjs"),
  yaml: () => import("shiki/dist/langs/yaml.mjs"),
  toml: () => import("shiki/dist/langs/toml.mjs"),
  ini: () => import("shiki/dist/langs/ini.mjs"),
  diff: () => import("shiki/dist/langs/diff.mjs"),
  dockerfile: () => import("shiki/dist/langs/dockerfile.mjs"),
  xml: () => import("shiki/dist/langs/xml.mjs"),
  graphql: () => import("shiki/dist/langs/graphql.mjs"),
  lua: () => import("shiki/dist/langs/lua.mjs"),
  php: () => import("shiki/dist/langs/php.mjs"),
  elixir: () => import("shiki/dist/langs/elixir.mjs"),
  dart: () => import("shiki/dist/langs/dart.mjs"),
  scala: () => import("shiki/dist/langs/scala.mjs"),
  vue: () => import("shiki/dist/langs/vue.mjs"),
  svelte: () => import("shiki/dist/langs/svelte.mjs"),
  astro: () => import("shiki/dist/langs/astro.mjs"),
  latex: () => import("shiki/dist/langs/latex.mjs"),
  mermaid: () => import("shiki/dist/langs/mermaid.mjs"),
} as const;

type LoadableLanguage = keyof typeof languageLoaders;

export type { LoadableLanguage };

const languageAliases: Partial<Record<string, LoadableLanguage>> = {
  ts: "typescript",
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  yml: "yaml",
  md: "markdown",
  sh: "shellscript",
  shell: "shellscript",
  bash: "shellscript",
  zsh: "shellscript",
  py: "python",
  cs: "csharp",
  "c#": "csharp",
  ps1: "powershell",
  docker: "dockerfile",
};

const extensionLanguages: Record<string, LoadableLanguage> = {
  ts: "typescript",
  tsx: "tsx",
  js: "javascript",
  jsx: "jsx",
  mjs: "javascript",
  cjs: "javascript",
  json: "json",
  jsonc: "jsonc",
  html: "html",
  htm: "html",
  css: "css",
  scss: "scss",
  md: "markdown",
  mdx: "mdx",
  sh: "shellscript",
  bash: "shellscript",
  zsh: "shellscript",
  py: "python",
  rs: "rust",
  go: "go",
  java: "java",
  kt: "kotlin",
  kts: "kotlin",
  swift: "swift",
  cs: "csharp",
  sql: "sql",
  yaml: "yaml",
  yml: "yaml",
  toml: "toml",
  ini: "ini",
  diff: "diff",
  patch: "diff",
  dockerfile: "dockerfile",
  xml: "xml",
  graphql: "graphql",
  gql: "graphql",
  lua: "lua",
  php: "php",
  ex: "elixir",
  exs: "elixir",
  dart: "dart",
  scala: "scala",
  vue: "vue",
  svelte: "svelte",
  astro: "astro",
  tex: "latex",
  mmd: "mermaid",
  ps1: "powershell",
};

const highlightCache = new Map<string, string>();
const maxCachedBlocks = 200;
const plainLanguages = new Set(["text", "plaintext", "txt", "none"]);

let highlighterPromise: Promise<HighlighterCore> | null = null;

function isLoadableLanguage(language: string): language is LoadableLanguage {
  return Object.hasOwn(languageLoaders, language);
}

export function resolveLanguage(language: string): LoadableLanguage | null {
  const alias = languageAliases[language];

  if (alias) {
    return alias;
  }

  return isLoadableLanguage(language) ? language : null;
}

export function detectLanguageFromPath(path: string): LoadableLanguage | null {
  const extension = path.split(".").pop()?.toLowerCase() ?? "";
  const base = path.split("/").pop()?.toLowerCase() ?? "";

  if (base === "dockerfile" || base === "makefile" || base === "gemfile") {
    return base === "makefile" ? null : base === "dockerfile" ? "dockerfile" : null;
  }

  return extensionLanguages[extension] ?? null;
}

export function isPlainLanguage(language: string | null | undefined): boolean {
  return Boolean(language && plainLanguages.has(language.toLowerCase()));
}

function loadHighlighter() {
  highlighterPromise ??= Promise.all([
    import("shiki/core"),
    import("shiki/engine/javascript"),
    import("shiki/dist/themes/github-light.mjs"),
    import("shiki/dist/themes/github-dark.mjs"),
  ]).then(
    ([{ createHighlighterCore }, { createJavaScriptRegexEngine }, { default: light }, { default: dark }]) =>
      createHighlighterCore({
        themes: [light, dark],
        engine: createJavaScriptRegexEngine({ forgiving: true }),
      }),
  );

  return highlighterPromise;
}

function cacheKey(code: string, language: string) {
  return `${language}\u0000${code}`;
}

export async function highlightCode(code: string, language: LoadableLanguage): Promise<string | null> {
  const key = cacheKey(code, language);
  const cached = highlightCache.get(key);

  if (cached) {
    return cached;
  }

  const highlighter = await loadHighlighter();

  if (!highlighter.getLoadedLanguages().includes(language)) {
    const loaded = await languageLoaders[language]();
    await highlighter.loadLanguage(loaded.default as never);
  }

  const html = highlighter.codeToHtml(code, {
    lang: language,
    themes: { light: "github-light", dark: "github-dark" },
    defaultColor: false,
  });

  highlightCache.set(key, html);

  if (highlightCache.size > maxCachedBlocks) {
    const oldest = highlightCache.keys().next();

    if (!oldest.done) {
      highlightCache.delete(oldest.value);
    }
  }

  return html;
}

export function cachedHighlight(code: string, language: LoadableLanguage): string | null {
  return highlightCache.get(cacheKey(code, language)) ?? null;
}

const themeCache = new Map<string, string>();

export async function highlightCodeTheme(
  code: string,
  language: LoadableLanguage,
  theme: "github-light" | "github-dark",
): Promise<string> {
  const key = `${theme}\u0000${language}\u0000${code}`;
  const cached = themeCache.get(key);

  if (cached) {
    return cached;
  }

  const highlighter = await loadHighlighter();

  if (!highlighter.getLoadedLanguages().includes(language)) {
    const loaded = await languageLoaders[language]();
    await highlighter.loadLanguage(loaded.default as never);
  }

  const html = highlighter.codeToHtml(code, { lang: language, theme });

  themeCache.set(key, html);

  if (themeCache.size > maxCachedBlocks) {
    const oldest = themeCache.keys().next();

    if (!oldest.done) {
      themeCache.delete(oldest.value);
    }
  }

  return html;
}
