import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDownIcon, PaperclipIcon } from "../../components/Icon";
import { Spinner } from "../../components/Spinner";
import { projectApi, type ProjectEntry } from "../../lib/projects";
import { cx } from "../../lib/cx";
import { fileIcon } from "../../lib/fileIcon";

type ChildState = {
  status: "loading" | "loaded" | "error";
  entries: ProjectEntry[];
};

function entriesSignature(entries: ProjectEntry[]): string {
  return entries.map((entry) => `${entry.kind}:${entry.name}`).join("|");
}

type FileTreeProps = {
  projectId: number;
  onMention?: (path: string) => void;
  onAttachText: (path: string) => void;
  onOpen: (path: string) => void;
};

export function FileTree({ projectId, onMention, onAttachText, onOpen }: FileTreeProps) {
  const { t } = useTranslation();
  const [rootEntries, setRootEntries] = useState<ProjectEntry[]>([]);
  const [rootStatus, setRootStatus] = useState<"loading" | "loaded" | "error">("loading");
  const [children, setChildren] = useState<Record<string, ChildState>>({});
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const expandedRef = useRef(expanded);
  expandedRef.current = expanded;

  useEffect(() => {
    let live = true;
    setRootStatus("loading");
    setRootEntries([]);
    setChildren({});
    setExpanded({});
    projectApi
      .listEntries(projectId)
      .then((entries) => {
        if (live) {
          setRootEntries(entries);
          setRootStatus("loaded");
        }
      })
      .catch(() => {
        if (live) {
          setRootStatus("error");
        }
      });
    return () => {
      live = false;
    };
  }, [projectId]);

  useEffect(() => {
    let live = true;

    async function refreshRoot() {
      try {
        const entries = await projectApi.listEntries(projectId);
        if (!live) return;
        setRootEntries((current) =>
          entriesSignature(current) === entriesSignature(entries) ? current : entries,
        );
        setRootStatus("loaded");
      } catch {
        // keep the current listing on a transient failure
      }
    }

    async function refreshChildren(path: string) {
      try {
        const entries = await projectApi.listEntries(projectId, path);
        if (!live) return;
        setChildren((current) => {
          const existing = current[path];
          if (existing?.status === "loaded" && entriesSignature(existing.entries) === entriesSignature(entries)) {
            return current;
          }
          return { ...current, [path]: { status: "loaded", entries } };
        });
      } catch {
        // keep the current listing on a transient failure
      }
    }

    function tick() {
      void refreshRoot();
      for (const [path, isOpen] of Object.entries(expandedRef.current)) {
        if (isOpen) void refreshChildren(path);
      }
    }

    const timer = window.setInterval(tick, 2000);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [projectId]);

  async function toggleDirectory(path: string) {
    const opening = !expanded[path];
    setExpanded((current) => ({ ...current, [path]: opening }));
    if (!opening) {
      return;
    }
    if (!children[path]) {
      setChildren((current) => ({ ...current, [path]: { status: "loading", entries: [] } }));
    }
    try {
      const entries = await projectApi.listEntries(projectId, path);
      setChildren((current) => ({ ...current, [path]: { status: "loaded", entries } }));
    } catch {
      setChildren((current) => ({ ...current, [path]: { status: "error", entries: [] } }));
    }
  }

  function renderEntry(entry: ProjectEntry, parentPath: string | null, depth: number): ReactNode {
    const path = parentPath ? `${parentPath}/${entry.name}` : entry.name;
    const isDirectory = entry.kind === "directory";
    const isFile = entry.kind === "file";

    if (isDirectory) {
      const isOpen = Boolean(expanded[path]);
      const child = children[path];
      const dirStyle = fileIcon(entry.name, "directory");
      const DirIcon = dirStyle.icon;
      return (
        <li key={path}>
          <button
            type="button"
            onClick={() => void toggleDirectory(path)}
            style={{ paddingLeft: `${depth * 12 + 6}px` }}
            className="flex w-full items-center gap-1.5 rounded-[6px] py-1 pr-2 text-left text-[12px] text-primary transition-colors hover:bg-hover"
          >
            <ChevronDownIcon
              size={12}
              className={cx("shrink-0 text-tertiary transition-transform", !isOpen && "-rotate-90")}
            />
            <DirIcon size={13} className={cx("shrink-0", dirStyle.color ?? "text-secondary")} />
            <span className="min-w-0 flex-1 truncate">{entry.name}</span>
          </button>
          {isOpen ? (
            child?.status === "loaded" ? (
              <ul className="list-none">
                {child.entries.map((nested) => renderEntry(nested, path, depth + 1))}
              </ul>
            ) : child?.status === "error" ? (
              <div
                style={{ paddingLeft: `${(depth + 1) * 12 + 6}px` }}
                className="py-1 text-[11px] text-danger"
              >
                {t("projects.files.loadError")}
              </div>
            ) : (
              <div
                style={{ paddingLeft: `${(depth + 1) * 12 + 6}px` }}
                className="flex items-center py-1"
              >
                <Spinner size={11} className="text-tertiary" />
              </div>
            )
          ) : null}
        </li>
      );
    }

    const fileStyle = fileIcon(entry.name, "file");
    const FileEntryIcon = fileStyle.icon;
    return (
      <li
        key={path}
        style={{ paddingLeft: `${depth * 12 + 6}px` }}
        className="group flex items-center gap-1 rounded-[6px] pr-1 transition-colors hover:bg-hover"
      >
        <button
          type="button"
          onClick={() => (isFile && onMention ? onMention(path) : undefined)}
          onDoubleClick={() => (isFile ? onOpen(path) : undefined)}
          disabled={!isFile}
          title={isFile ? t("projects.files.open") : undefined}
          className="flex min-w-0 flex-1 items-center gap-1.5 py-1 text-left text-[12px] text-primary disabled:opacity-70"
        >
          <FileEntryIcon size={13} className={cx("shrink-0", fileStyle.color ?? "text-secondary")} />
          <span className="min-w-0 flex-1 truncate">{entry.name}</span>
        </button>
        {isFile ? (
          <button
            type="button"
            onClick={() => onAttachText(path)}
            title={t("projects.files.attach")}
            aria-label={t("projects.files.attach")}
            className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-md text-tertiary opacity-0 transition-opacity hover:bg-active hover:text-primary focus-visible:opacity-100 group-hover:opacity-100"
          >
            <PaperclipIcon size={12} />
          </button>
        ) : null}
      </li>
    );
  }

  if (rootStatus === "loading") {
    return (
      <div className="flex justify-center py-4">
        <Spinner size={14} className="text-tertiary" />
      </div>
    );
  }

  if (rootStatus === "error") {
    return (
      <p role="alert" className="py-3 text-[12px] leading-relaxed text-danger">
        {t("projects.files.loadError")}
      </p>
    );
  }

  if (!rootEntries.length) {
    return <p className="py-3 text-[12px] text-tertiary">{t("projects.files.empty")}</p>;
  }

  return <ul className="list-none">{rootEntries.map((entry) => renderEntry(entry, null, 0))}</ul>;
}
