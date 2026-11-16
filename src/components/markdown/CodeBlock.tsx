import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { createHighlighter, type Highlighter } from "shiki";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";
import { CopyIcon } from "../Icon";
import { useToast } from "../Toast";
import { copyText } from "../../lib/copy";

const themes = ["github-light", "github-dark"] as const;

const languages = [
  "text",
  "typescript",
  "tsx",
  "javascript",
  "jsx",
  "json",
  "jsonc",
  "html",
  "css",
  "scss",
  "markdown",
  "mdx",
  "shellscript",
  "powershell",
  "python",
  "rust",
  "go",
  "java",
  "kotlin",
  "swift",
  "csharp",
  "cpp",
  "c",
  "objective-c",
  "sql",
  "yaml",
  "toml",
  "ini",
  "diff",
  "dockerfile",
  "xml",
  "graphql",
  "lua",
  "ruby",
  "php",
  "r",
  "perl",
  "haskell",
  "elixir",
  "dart",
  "scala",
  "vue",
  "svelte",
  "astro",
  "latex",
  "mermaid",
];

const highlightCache = new Map<string, string>();
const maxCachedBlocks = 200;
const plainLanguages = new Set(["text", "plaintext", "txt", "none"]);

let highlighterPromise: Promise<Highlighter> | null = null;

function loadHighlighter() {
  highlighterPromise ??= createHighlighter({
    themes: [...themes],
    langs: languages,
    engine: createJavaScriptRegexEngine({ forgiving: true }),
  });

  return highlighterPromise;
}

async function highlight(code: string, language: string): Promise<string | null> {
  const key = cacheKey(code, language);
  const cached = highlightCache.get(key);

  if (cached) {
    return cached;
  }

  const highlighter = await loadHighlighter();

  if (!highlighter.getLoadedLanguages().includes(language)) {
    return null;
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
  const key = normalized && !plainLanguages.has(normalized) ? cacheKey(code, normalized) : null;
  const [html, setHtml] = useState<string | null>(() => (key ? (highlightCache.get(key) ?? null) : null));

  useEffect(() => {
    if (!key || !normalized) {
      return;
    }

    const cached = highlightCache.get(key);

    if (cached) {
      setHtml(cached);
      return;
    }

    let live = true;

    highlight(code, normalized)
      .then((result) => {
        if (live && result) {
          setHtml(result);
        }
      })
      .catch(() => undefined);

    return () => {
      live = false;
    };
  }, [code, key, normalized]);

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
