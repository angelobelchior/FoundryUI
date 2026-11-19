import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { contextApi, type ContextBudget } from "../../lib/context";
import { projectControlFocus } from "./projectFocus";

export function ContextPanel({ conversationId }: { conversationId: number }) {
  const { t, i18n } = useTranslation();
  const [open, setOpen] = useState(false);
  const [budgets, setBudgets] = useState<ContextBudget[] | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  const mounted = useRef(true);
  const generation = useRef(0);

  const refresh = useCallback(async () => {
    const version = ++generation.current;
    try {
      const next = await contextApi.budgets(conversationId);
      if (!mounted.current || generation.current !== version) return;
      setBudgets(next);
      setError(false);
    } catch {
      if (mounted.current && generation.current === version) setError(true);
    } finally {
      if (mounted.current && generation.current === version) setLoading(false);
    }
  }, [conversationId]);

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; generation.current += 1; };
  }, []);

  useEffect(() => {
    if (!open) return;
    void refresh();
  }, [open, refresh]);

  const button = `rounded-md border border-line px-2 py-1 text-[13px] text-primary hover:bg-hover disabled:opacity-50 ${projectControlFocus}`;
  const reason = (value: string) => (value === "budget" ? t("context.reasonBudget") : t("context.reasonOlder"));
  const fingerprint = (value: string) => value.slice(0, 12);

  return <details className="mt-2" onToggle={(event) => setOpen(event.currentTarget.open)}>
    <summary className={`cursor-pointer ${projectControlFocus}`}>{t("context.title")}</summary>
    {open ? <div className="mt-2 max-h-96 space-y-4 overflow-y-auto border-t border-line pt-3 text-[14px] [&_p]:max-w-[75ch]" aria-label={t("context.title")}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <button type="button" className={button} onClick={() => void refresh()}>{t("context.refresh")}</button>
        {loading ? <span role="status">{t("context.loading")}</span> : null}
      </div>
      {error ? <p role="alert">{t("context.error")} <button type="button" className={button} onClick={() => void refresh()}>{t("context.retry")}</button></p> : null}
      {!loading && !error && !budgets?.length ? <p>{t("context.empty")}</p> : null}
      {budgets?.map((budget) => {
        const manifest = budget.manifest;
        const timestamp = new Date(budget.createdAt);
        return <div key={budget.id} className="border-t border-line pt-3">
          <div className="flex flex-wrap items-baseline justify-between gap-2">
            <h2 className="font-medium">{t("context.call", { key: budget.key })}</h2>
            {Number.isFinite(timestamp.getTime()) ? <time dateTime={timestamp.toISOString()} className="text-[12px] text-secondary">{t("context.at", { time: timestamp.toLocaleString(i18n.language) })}</time> : null}
          </div>
          <p className="mt-1">{t("context.counts", { included: manifest.included_messages, summarized: manifest.summarized_messages, omitted: manifest.omitted_messages, original: manifest.original_messages })}</p>
          {manifest.warning ? <p className="mt-1">{t("context.conservative")}</p> : null}
          {manifest.truncated ? <p className="mt-1">{t("context.truncated")}</p> : null}
          <div className="mt-3">
            <h3 className="text-[13px] font-medium">{t("context.sources")}</h3>
            {!manifest.sources.length ? <p className="mt-1 text-[13px] text-secondary">{t("context.sourcesEmpty")}</p> : <ul className="mt-1 divide-y divide-line">
              {manifest.sources.map((source) => <li key={source.identity_key} className="flex flex-wrap items-center gap-x-3 py-1 text-[13px]">
                <span className="min-w-0 break-all">{source.identity_key}</span>
                <span className="text-secondary">{source.kind}</span>
                <span className="text-secondary">{source.mode === "summary" ? t("context.modeSummary") : t("context.modeOriginal")}</span>
                <span className="text-secondary">{source.tokens} tok</span>
                <span className="text-secondary" title={source.content_fingerprint}>sha256:{fingerprint(source.content_fingerprint)}</span>
              </li>)}
            </ul>}
          </div>
          <div className="mt-3">
            <h3 className="text-[13px] font-medium">{t("context.omissions")}</h3>
            {!manifest.omissions.length ? <p className="mt-1 text-[13px] text-secondary">{t("context.omissionsEmpty")}</p> : <ul className="mt-1 divide-y divide-line">
              {manifest.omissions.map((omission, index) => <li key={`${omission.identity_key}-${index}`} className="flex flex-wrap items-center gap-x-3 py-1 text-[13px]">
                <span className="min-w-0 break-all">{omission.identity_key}</span>
                <span className="text-secondary">{omission.kind}</span>
                <span className="text-secondary">{reason(omission.reason)}</span>
              </li>)}
            </ul>}
          </div>
        </div>;
      })}
    </div> : null}
  </details>;
}
