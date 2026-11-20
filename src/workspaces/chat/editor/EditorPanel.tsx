import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { CloseIcon } from "../../../components/Icon";
import { Spinner } from "../../../components/Spinner";
import { cx } from "../../../lib/cx";
import { detectLanguageFromPath } from "../../../lib/highlight";
import { readStored, writeStored } from "../../../lib/storage";
import { CodeEditor } from "./CodeEditor";
import type { useEditorTabs } from "./useEditorTabs";

const editorWidthKey = "foundryui.editor.width";
const minEditorWidth = 260;
const maxEditorWidth = 960;
const defaultEditorWidth = 460;

type EditorPanelProps = {
  editor: ReturnType<typeof useEditorTabs>;
};

export function EditorPanel({ editor }: EditorPanelProps) {
  const { t } = useTranslation();
  const { tabs, active, activeId, closeFile, closeAll, setActiveId, updateContent, save, isDirty } = editor;

  const [width, setWidth] = useState<number>(() => {
    const stored = Number(readStored(editorWidthKey));
    return Number.isFinite(stored)
      ? Math.min(maxEditorWidth, Math.max(minEditorWidth, Math.round(stored)))
      : defaultEditorWidth;
  });
  const widthRef = useRef(width);
  widthRef.current = width;

  const handlersRef = useRef({ save, closeFile, closeAll, activeId, hasTabs: tabs.length > 0 });
  handlersRef.current = { save, closeFile, closeAll, activeId, hasTabs: tabs.length > 0 };

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      const meta = event.metaKey || event.ctrlKey;
      if (!meta) return;
      const key = event.key.toLowerCase();
      const handlers = handlersRef.current;

      if (key === "s" && handlers.activeId) {
        event.preventDefault();
        void handlers.save(handlers.activeId);
        return;
      }

      if (key === "w" && handlers.hasTabs) {
        event.preventDefault();
        if (event.shiftKey) {
          handlers.closeAll();
        } else if (handlers.activeId) {
          handlers.closeFile(handlers.activeId);
        }
      }
    }

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  function startResize(event: React.PointerEvent<HTMLDivElement>) {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = widthRef.current;

    function onPointerMove(moveEvent: PointerEvent) {
      const next = Math.min(
        maxEditorWidth,
        Math.max(minEditorWidth, Math.round(startWidth + (moveEvent.clientX - startX))),
      );
      widthRef.current = next;
      setWidth(next);
    }

    function onPointerUp() {
      writeStored(editorWidthKey, String(widthRef.current));
      document.removeEventListener("pointermove", onPointerMove);
      document.removeEventListener("pointerup", onPointerUp);
    }

    document.addEventListener("pointermove", onPointerMove);
    document.addEventListener("pointerup", onPointerUp);
  }

  if (tabs.length === 0) {
    return null;
  }

  return (
    <section
      className="relative flex h-full min-h-0 shrink-0 flex-col border-r border-line bg-canvas"
      style={{ width }}
    >
      <div className="flex shrink-0 items-center overflow-x-auto border-b border-line bg-subtle/40 py-1 pl-1 pr-1">
        {tabs.map((tab) => {
          const selected = tab.id === activeId;
          return (
            <div
              key={tab.id}
              className={cx(
                "group flex shrink-0 items-center rounded-md text-[11.5px] transition-colors",
                selected
                  ? "border border-line bg-canvas text-primary"
                  : "border border-transparent text-secondary hover:bg-hover hover:text-primary",
              )}
            >
              <button
                type="button"
                onClick={() => setActiveId(tab.id)}
                className="flex min-w-0 items-center gap-1.5 px-2.5 py-1.5"
              >
                <span className="max-w-[160px] truncate">{tab.name}</span>
                {isDirty(tab.id) ? (
                  <span aria-hidden className="h-1.5 w-1.5 shrink-0 rounded-full bg-accent" />
                ) : null}
              </button>
              <button
                type="button"
                onClick={() => closeFile(tab.id)}
                aria-label={t("editor.close")}
                title={t("editor.close")}
                className="mr-0.5 inline-flex h-5 w-5 shrink-0 items-center justify-center rounded text-tertiary transition-colors hover:bg-active hover:text-primary"
              >
                <CloseIcon size={11} />
              </button>
            </div>
          );
        })}
        <button
          type="button"
          onClick={closeAll}
          aria-label={t("editor.closeAll")}
          title={t("editor.closeAll")}
          className="ml-1 inline-flex h-6 shrink-0 items-center gap-1 rounded-md px-1.5 text-[11px] text-tertiary transition-colors hover:bg-active hover:text-primary"
        >
          <CloseIcon size={11} />
          <span>{t("editor.closeAll")}</span>
        </button>
      </div>

      <div className="flex min-h-0 flex-1 flex-col">
        {active ? (
          active.status === "loading" ? (
            <div className="flex flex-1 items-center justify-center">
              <Spinner size={16} className="text-tertiary" />
            </div>
          ) : active.status === "error" ? (
            <p role="alert" className="px-4 py-6 text-[12px] leading-relaxed text-danger">
              {t("editor.loadError")}
            </p>
          ) : (
            <CodeEditor
              value={active.content}
              language={detectLanguageFromPath(active.path)}
              onChange={(value) => updateContent(active.id, value)}
            />
          )
        ) : null}
      </div>

      {active && active.status === "ready" ? (
        <div className="flex shrink-0 items-center justify-between gap-3 border-t border-line px-3 py-1.5">
          <span className="min-w-0 truncate font-mono text-[11px] text-tertiary" title={active.path}>
            {active.path}
          </span>
          <button
            type="button"
            onClick={() => void save(active.id)}
            disabled={!isDirty(active.id)}
            className="inline-flex h-6 shrink-0 items-center rounded-md border border-line px-2 text-[11px] font-medium text-secondary transition-colors hover:border-line-strong hover:text-primary disabled:cursor-not-allowed disabled:opacity-50"
          >
            {t("editor.save")}
          </button>
        </div>
      ) : null}

      <div
        role="separator"
        aria-orientation="vertical"
        aria-label={t("editor.resize")}
        onPointerDown={startResize}
        className="absolute bottom-0 right-0 top-0 z-10 w-1.5 -translate-x-1/2 cursor-col-resize touch-none select-none"
      >
        <div className="pointer-events-none mx-auto h-full w-px bg-line transition-colors hover:bg-line-strong" />
      </div>
    </section>
  );
}
