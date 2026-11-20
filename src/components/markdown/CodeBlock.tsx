import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { HighlighterCore } from "shiki/core";
import { CopyIcon } from "../Icon";
import { useToast } from "../Toast";
import { copyText } from "../../lib/copy";

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

const highlightCache = new Map<string, string>();
const maxCachedBlocks = 200;
const plainLanguages = new Set(["text", "plaintext", "txt", "none"]);

let highlighterPromise: Promise<HighlighterCore> | null = null;

function isLoadableLanguage(language: string): language is LoadableLanguage {
  return Object.hasOwn(languageLoaders, language);
}

function resolveLanguage(language: string): LoadableLanguage | null {
  const alias = languageAliases[language];

  if (alias) {
    return alias;
  }

  return isLoadableLanguage(language) ? language : null;
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

async function highlight(code: string, language: LoadableLanguage): Promise<string | null> {
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

function cacheKey(code: string, language: string) {
  return `${language}\u0000${code}`;
}

type CodeBlockProps = {
  code: string;
  language: string | null;
};

export function CodeBlock({ code, language }: CodeBlockProps) {
  const { t } = useTranslation();
  const { push } = useToast();
  const normalized = language?.toLowerCase() ?? null;
  const resolved = normalized && !plainLanguages.has(normalized) ? resolveLanguage(normalized) : null;
  const key = resolved ? cacheKey(code, resolved) : null;
  const [html, setHtml] = useState<string | null>(() => (key ? (highlightCache.get(key) ?? null) : null));

  useEffect(() => {
    if (!key || !resolved) {
      return;
    }

    const cached = highlightCache.get(key);

    if (cached) {
      setHtml(cached);
      return;
    }

    let live = true;

    highlight(code, resolved)
      .then((result) => {
        if (live && result) {
          setHtml(result);
        }
      })
      .catch(() => undefined);

    return () => {
      live = false;
    };
  }, [code, key, resolved]);

  async function copyCode() {
    const copied = await copyText(code);

    push(
      copied ? t("markdown.copied") : t("markdown.copyFailed"),
      copied ? "neutral" : "danger",
    );
  }

  return (
    <div className="md-code my-3 overflow-hidden rounded-lg border border-line bg-subtle">
      <div className="flex h-8 items-center justify-between gap-2 border-b border-line px-2.5">
        <span className="truncate font-mono text-[10px] uppercase tracking-[0.08em] text-tertiary">
          {normalized ?? ""}
        </span>
        <button
          type="button"
          aria-label={t("markdown.copyCode")}
          onClick={() => void copyCode()}
          className="inline-flex h-6 shrink-0 items-center gap-1 rounded-md px-1.5 text-[10.5px] text-secondary transition-colors hover:bg-hover hover:text-primary"
        >
          <CopyIcon size={12} />
          <span>{t("markdown.copy")}</span>
        </button>
      </div>
      {html ? (
        <div dangerouslySetInnerHTML={{ __html: html }} />
      ) : (
        <pre className="md-code-plain">
          <code>{code}</code>
        </pre>
      )}
    </div>
  );
}
