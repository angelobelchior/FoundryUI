import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDownIcon, CloseIcon, FolderIcon, SearchIcon } from "../../components/Icon";
import { Spinner } from "../../components/Spinner";
import { cx } from "../../lib/cx";
import { fileIcon } from "../../lib/fileIcon";
import { projectApi, type ProjectSearchResult } from "../../lib/projects";
import { projectControlFocus } from "./projectFocus";
import { FileTree } from "./FileTree";

const minSearchQuery = 2;

type ProjectFilesPanelProps = {
  projectId: number;
  onOpen: (path: string) => void;
  onAttachText: (path: string) => void;
};

export function ProjectFilesPanel({ projectId, onOpen, onAttachText }: ProjectFilesPanelProps) {
  const { t } = useTranslation();
  const [searchOpen, setSearchOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<ProjectSearchResult | null>(null);
  const [searching, setSearching] = useState(false);
  const [error, setError] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const anchorRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const trimmed = query.trim();
  const active = searchOpen && trimmed.length >= minSearchQuery;

  useEffect(() => {
    if (searchOpen) {
      inputRef.current?.focus();
    } else {
      setQuery("");
    }
  }, [searchOpen]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      const key = event.key.toLowerCase();
      if ((event.metaKey || event.ctrlKey) && key === "f") {
        if (document.querySelector('[role="dialog"]')) return;
        event.preventDefault();
        event.stopPropagation();
        setSearchOpen(true);
        return;
      }
      if (event.key === "Escape" && searchOpen && !document.querySelector('[role="dialog"]')) {
        event.preventDefault();
        setSearchOpen(false);
      }
    }

    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  }, [searchOpen]);

  useEffect(() => {
    if (!searchOpen) return;
    function onPointerDown(event: PointerEvent) {
      const target = event.target as Node | null;
      if (target && anchorRef.current?.contains(target)) return;
      setSearchOpen(false);
    }
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [searchOpen]);

  useEffect(() => {
    setActiveIndex(0);
  }, [results]);

  useEffect(() => {
    if (!active) {
      setResults(null);
      setError(false);
      setSearching(false);
      return;
    }

    let live = true;
    setSearching(true);

    const timer = window.setTimeout(() => {
      projectApi
        .search(projectId, trimmed)
        .then((next) => {
          if (live) {
            setResults(next);
            setError(false);
          }
        })
        .catch(() => {
          if (live) setError(true);
        })
        .finally(() => {
          if (live) setSearching(false);
        });
    }, 200);

    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [projectId, trimmed, active]);

  function toggleSearch(event: React.MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    setSearchOpen((current) => !current);
  }

  function openResult(path: string) {
    setSearchOpen(false);
    onOpen(path);
  }

  function handleSearchKeyDown(event: React.KeyboardEvent<HTMLInputElement>) {
    if (!results || results.hits.length === 0) {
      return;
    }

    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActiveIndex((current) => (current + 1) % results.hits.length);
      return;
    }

    if (event.key === "ArrowUp") {
      event.preventDefault();
      setActiveIndex((current) => (current - 1 + results.hits.length) % results.hits.length);
      return;
    }

    if (event.key === "Enter") {
      event.preventDefault();
      const hit = results.hits[Math.min(activeIndex, results.hits.length - 1)];
      if (hit && hit.kind === "file") {
        openResult(hit.path);
      }
    }
  }

  return (
    <details className="group border-b border-line">
      <summary
        className={cx(
          "flex cursor-pointer list-none items-center gap-1.5 px-3 py-2 text-[11.5px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary",
          projectControlFocus,
        )}
      >
        <ChevronDownIcon size={12} className="text-tertiary transition-transform group-open:rotate-180" />
        <FolderIcon size={13} className="shrink-0" />
        <span className="min-w-0 flex-1 truncate">{t("projects.files.title")}</span>
        <div ref={anchorRef} className="relative shrink-0">
          <button
            type="button"
            onClick={toggleSearch}
            aria-label={t("projects.files.search")}
            title={t("projects.files.search")}
            aria-expanded={searchOpen}
            className={cx(
              "inline-flex h-6 w-6 items-center justify-center rounded-[6px] transition-colors",
              searchOpen
                ? "bg-active text-primary"
                : "text-tertiary hover:bg-hover hover:text-primary",
            )}
          >
            <SearchIcon size={13} />
          </button>

          {searchOpen ? (
            <div
              className="absolute right-0 top-full z-30 mt-1 w-56 rounded-lg border border-line bg-elevated p-2 shadow-[var(--shadow-popover)]"
              onClick={(event) => event.stopPropagation()}
            >
              <div className="relative">
                <SearchIcon
                  size={13}
                  className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-tertiary"
                />
                <input
                  ref={inputRef}
                  type="text"
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                  onKeyDown={handleSearchKeyDown}
                  placeholder={t("projects.files.searchPlaceholder")}
                  aria-label={t("projects.files.searchPlaceholder")}
                  className="h-7 w-full rounded-[6px] border border-line bg-canvas pl-8 pr-7 text-[12px] text-primary outline-none transition-colors placeholder:text-tertiary focus:border-accent"
                />
                <button
                  type="button"
                  aria-label={t("projects.files.closeSearch")}
                  title={t("projects.files.closeSearch")}
                  onClick={() => setSearchOpen(false)}
                  className="absolute right-1.5 top-1/2 inline-flex h-5 w-5 -translate-y-1/2 items-center justify-center rounded text-tertiary transition-colors hover:bg-hover hover:text-primary"
                >
                  <CloseIcon size={12} />
                </button>
              </div>

              {active ? (
                <div className="-mx-1 mt-1.5 max-h-64 overflow-y-auto">
                  <SearchResults
                    results={results}
                    searching={searching}
                    error={error}
                    activeIndex={activeIndex}
                    onActiveChange={setActiveIndex}
                    onOpen={openResult}
                  />
                </div>
              ) : (
                <p className="px-1 pt-1.5 text-[10.5px] leading-relaxed text-tertiary">
                  {t("projects.files.searchHint")}
                </p>
              )}
            </div>
          ) : null}
        </div>
      </summary>

      <div className="max-h-72 overflow-y-auto pb-2">
        <div className="px-2">
          <FileTree projectId={projectId} onAttachText={onAttachText} onOpen={onOpen} />
        </div>
      </div>
    </details>
  );
}

type SearchResultsProps = {
  results: ProjectSearchResult | null;
  searching: boolean;
  error: boolean;
  activeIndex: number;
  onActiveChange: (index: number) => void;
  onOpen: (path: string) => void;
};

function SearchResults({ results, searching, error, activeIndex, onActiveChange, onOpen }: SearchResultsProps) {
  const { t } = useTranslation();

  if (searching) {
    return (
      <div className="flex justify-center py-3">
        <Spinner size={14} className="text-tertiary" />
      </div>
    );
  }

  if (error) {
    return (
      <p role="alert" className="px-2 py-3 text-[12px] leading-relaxed text-danger">
        {t("projects.files.searchError")}
      </p>
    );
  }

  if (!results) {
    return null;
  }

  if (results.hits.length === 0) {
    return <p className="px-2 py-3 text-[12px] text-tertiary">{t("projects.files.searchEmpty")}</p>;
  }

  return (
    <div>
      <ul className="list-none">
        {results.hits.map((hit, index) => {
          const style = fileIcon(hit.path.split("/").pop() ?? hit.path, hit.kind);
          const Icon = style.icon;
          const isFile = hit.kind === "file";
          return (
            <li key={hit.path}>
              <button
                type="button"
                onClick={() => (isFile ? onOpen(hit.path) : undefined)}
                onMouseEnter={() => onActiveChange(index)}
                disabled={!isFile}
                title={isFile ? t("projects.files.open") : undefined}
                className={cx(
                  "flex w-full items-center gap-1.5 rounded-[6px] px-2 py-1 text-left text-[12px] transition-colors disabled:opacity-70",
                  index === activeIndex ? "bg-hover text-primary" : "text-primary hover:bg-hover",
                )}
              >
                <Icon size={13} className={cx("shrink-0", style.color ?? "text-secondary")} />
                <span className="min-w-0 flex-1 truncate">{hit.path}</span>
              </button>
              {hit.matches.length > 0 ? (
                <ul className="list-none pb-1 pl-7 pr-2">
                  {hit.matches.map((match) => (
                    <li key={match.line} className="flex gap-2 py-0.5 text-[11px] leading-relaxed">
                      <span className="shrink-0 tabular-nums text-tertiary">{match.line}</span>
                      <span className="min-w-0 flex-1 truncate font-mono text-secondary">
                        {match.snippet}
                      </span>
                    </li>
                  ))}
                </ul>
              ) : null}
            </li>
          );
        })}
      </ul>
      {results.truncated ? (
        <p className="px-2 py-2 text-[11px] text-tertiary">{t("projects.files.searchTruncated")}</p>
      ) : null}
    </div>
  );
}
