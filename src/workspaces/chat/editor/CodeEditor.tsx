import { useEffect, useMemo, useRef, useState } from "react";
import { highlightCodeTheme, type LoadableLanguage } from "../../../lib/highlight";

type CodeEditorProps = {
  value: string;
  language: LoadableLanguage | null;
  onChange: (value: string) => void;
};

export function CodeEditor({ value, language, onChange }: CodeEditorProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const highlightRef = useRef<HTMLDivElement>(null);
  const gutterRef = useRef<HTMLDivElement>(null);
  const requestRef = useRef(0);
  const [html, setHtml] = useState<string | null>(null);

  const lineCount = useMemo(() => value.split("\n").length, [value]);
  const lineNumbers = useMemo(
    () => Array.from({ length: lineCount }, (_, index) => index + 1),
    [lineCount],
  );

  useEffect(() => {
    if (!language) {
      setHtml(null);
      return;
    }

    const dark = document.documentElement.dataset.theme === "dark";
    const theme = dark ? "github-dark" : "github-light";
    const request = ++requestRef.current;
    const timer = window.setTimeout(() => {
      highlightCodeTheme(value, language, theme)
        .then((result) => {
          if (requestRef.current === request) {
            setHtml(result);
          }
        })
        .catch(() => undefined);
    }, 120);

    return () => window.clearTimeout(timer);
  }, [value, language]);

  function syncScroll() {
    const textarea = textareaRef.current;
    if (!textarea) return;

    if (highlightRef.current) {
      highlightRef.current.style.transform = `translate(${-textarea.scrollLeft}px, ${-textarea.scrollTop}px)`;
    }
    if (gutterRef.current) {
      gutterRef.current.scrollTop = textarea.scrollTop;
    }
  }

  function handleKeyDown(event: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key !== "Tab") return;
    event.preventDefault();
    const textarea = event.currentTarget;
    const start = textarea.selectionStart;
    const end = textarea.selectionEnd;
    const next = `${value.slice(0, start)}  ${value.slice(end)}`;
    onChange(next);
    window.requestAnimationFrame(() => {
      textarea.selectionStart = textarea.selectionEnd = start + 2;
    });
  }

  return (
    <div className="flex h-full min-h-0 flex-1">
      <div
        ref={gutterRef}
        aria-hidden
        className="editor-gutter shrink-0 select-none overflow-hidden border-r border-line bg-subtle/40 text-right"
      >
        {lineNumbers.map((number) => (
          <div key={number} className="editor-gutter-line text-right">
            {number}
          </div>
        ))}
      </div>
      <div className="relative min-h-0 min-w-0 flex-1 overflow-hidden">
        {html ? (
          <div
            ref={highlightRef}
            aria-hidden
            className="editor-highlight pointer-events-none absolute left-0 top-0 min-w-full will-change-transform"
            dangerouslySetInnerHTML={{ __html: html }}
          />
        ) : null}
        <textarea
          ref={textareaRef}
          value={value}
          onChange={(event) => onChange(event.target.value)}
          onScroll={syncScroll}
          onKeyDown={handleKeyDown}
          wrap="off"
          spellCheck={false}
          autoCapitalize="off"
          autoComplete="off"
          autoCorrect="off"
          className="editor-input absolute inset-0 h-full w-full resize-none overflow-auto bg-transparent outline-none"
        />
      </div>
    </div>
  );
}
