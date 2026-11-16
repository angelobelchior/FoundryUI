import { useEffect, useState, type KeyboardEvent, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { CloseIcon, SearchIcon } from "../../components/Icon";
import { IconButton } from "../../components/IconButton";
import { SegmentedControl } from "../../components/SegmentedControl";
import { Spinner } from "../../components/Spinner";
import type { SearchHit } from "../../lib/chat";
import { cx } from "../../lib/cx";
import type { FoundryCommandError } from "../../lib/foundry";
import { minimumSearchQuery, searchResultLimit, snippetParts } from "../../lib/search";

export type SearchScope = "conversation" | "all";

type SearchPanelProps = {
  inputRef: RefObject<HTMLInputElement | null>;
  query: string;
  scope: SearchScope;
  results: SearchHit[];
  searching: boolean;
  error: FoundryCommandError | null;
  onQueryChange: (value: string) => void;
  onScopeChange: (scope: SearchScope) => void;
  onSelect: (hit: SearchHit) => void;
  onClose: () => void;
};

export function SearchPanel({
  inputRef,
  query,
  scope,
  results,
  searching,
  error,
  onQueryChange,
  onScopeChange,
  onSelect,
  onClose,
}: SearchPanelProps) {
  const { t } = useTranslation();
  const [activeIndex, setActiveIndex] = useState(0);
  const ready = query.trim().length >= minimumSearchQuery;

  useEffect(() => {
    setActiveIndex(0);
  }, [results]);

  function onKeyDown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();

      return;
    }

    if (!results.length) {
      return;
    }

    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActiveIndex((current) => (current + 1) % results.length);
      return;
    }

    if (event.key === "ArrowUp") {
      event.preventDefault();
      setActiveIndex((current) => (current - 1 + results.length) % results.length);
      return;
    }

    if (event.key === "Enter") {
      event.preventDefault();

      const hit = results[Math.min(activeIndex, results.length - 1)];

      if (hit) {
        onSelect(hit);
      }
    }
  }

  return (
    <div className="shrink-0 border-b border-line bg-subtle px-3 py-2 sm:px-4" role="search">
      <div className="mx-auto flex max-w-[760px] flex-col gap-1.5">
        <div className="flex items-center gap-2">
          <SearchIcon size={14} className="shrink-0 text-tertiary" />
          <input
            ref={inputRef}
            autoFocus
            type="text"
            value={query}
            onChange={(event) => onQueryChange(event.target.value)}
            onKeyDown={onKeyDown}
            aria-label={t("chat.search.label")}
            placeholder={scope === "all" ? t("chat.search.placeholderAll") : t("chat.search.placeholder")}
            className="h-7 min-w-0 flex-1 bg-transparent text-[12.5px] text-primary outline-none placeholder:text-tertiary"
          />
          {searching ? <Spinner size={11} className="shrink-0 text-tertiary" /> : null}
          <IconButton label={t("chat.search.close")} onClick={onClose} className="h-6 w-6 shrink-0">
            <CloseIcon size={13} />
          </IconButton>
        </div>

        <div className="flex flex-wrap items-center justify-between gap-2 pl-6">
          {ready && !searching && !error ? (
            <span className="min-w-0 text-[10.5px] tabular-nums text-tertiary">
              {results.length === 0
                ? t("chat.search.empty", { query: query.trim() })
                : results.length >= searchResultLimit
                  ? t("chat.search.resultsCapped", { count: results.length })
                  : t("chat.search.results", { count: results.length })}
            </span>
          ) : null}
          <SegmentedControl
            label={t("chat.search.label")}
            value={scope}
            options={[
              { value: "conversation", label: t("chat.search.scopeConversation") },
              { value: "all", label: t("chat.search.scopeAll") },
            ]}
            onChange={onScopeChange}
          />
        </div>

        {!ready ? (
          <p className="pl-6 text-[10.5px] text-tertiary">{t("chat.search.hint")}</p>
        ) : error ? (
          <p className="pl-6 text-[10.5px] text-danger">{t("chat.search.failed")}</p>
        ) : results.length ? (
          <ul className="-mx-1 max-h-[240px] overflow-y-auto pt-0.5">
            {results.map((hit, index) => (
              <li key={hit.messageId}>
                <button
                  type="button"
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => onSelect(hit)}
                  onMouseEnter={() => setActiveIndex(index)}
                  className={cx(
                    "flex w-full flex-col gap-0.5 rounded-md px-2 py-1.5 text-left transition-colors",
                    index === activeIndex ? "bg-hover" : "hover:bg-hover",
                  )}
                >
                  <span className="flex min-w-0 items-baseline gap-2 text-[10.5px] text-tertiary">
                    <span className="shrink-0 font-medium text-secondary">
                      {t(`chat.roles.${hit.role}`)}
                    </span>
                    {scope === "all" ? (
                      <span className="truncate">{hit.conversationTitle || t("chat.untitled")}</span>
                    ) : null}
                  </span>
                  <span className="line-clamp-2 break-words text-[12px] leading-[1.5] text-secondary">
                    {snippetParts(hit.snippet).map((part, partIndex) =>
                      part.highlighted ? (
                        <mark
                          key={partIndex}
                          className="rounded-[3px] bg-accent/15 px-0.5 text-primary"
                        >
                          {part.text}
                        </mark>
                      ) : (
                        <span key={partIndex}>{part.text}</span>
                      ),
                    )}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        ) : null}
      </div>
    </div>
  );
}
