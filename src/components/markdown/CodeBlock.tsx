import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { CopyIcon } from "../Icon";
import { useToast } from "../Toast";
import { copyText } from "../../lib/copy";
import { cachedHighlight, highlightCode, isPlainLanguage, resolveLanguage } from "../../lib/highlight";

type CodeBlockProps = {
  code: string;
  language: string | null;
};

export function CodeBlock({ code, language }: CodeBlockProps) {
  const { t } = useTranslation();
  const { push } = useToast();
  const normalized = language?.toLowerCase() ?? null;
  const resolved = normalized && !isPlainLanguage(normalized) ? resolveLanguage(normalized) : null;
  const [html, setHtml] = useState<string | null>(() => (resolved ? cachedHighlight(code, resolved) : null));

  useEffect(() => {
    if (!resolved) {
      return;
    }

    const cached = cachedHighlight(code, resolved);

    if (cached) {
      setHtml(cached);
      return;
    }

    let live = true;

    highlightCode(code, resolved)
      .then((result) => {
        if (live && result) {
          setHtml(result);
        }
      })
      .catch(() => undefined);

    return () => {
      live = false;
    };
  }, [code, resolved]);

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
