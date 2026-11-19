import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { accessApi, mergeNetworkEntries, safeNetworkDestination, type NetworkLogPage } from "../../lib/access";
import { projectControlFocus } from "./projectFocus";

export function ReadNetworkPanel({ conversationId, onPreferenceChanged }: { conversationId: number; onPreferenceChanged: () => void }) {
  const { t, i18n } = useTranslation();
  const [open, setOpen] = useState(false);
  const [preference, setPreference] = useState<boolean | null>(null);
  const [log, setLog] = useState<NetworkLogPage>({ entries: [], hasMore: false, nextBefore: null });
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [olderBusy, setOlderBusy] = useState(false);
  const [error, setError] = useState(false);
  const [actionError, setActionError] = useState(false);
  const [copyStatus, setCopyStatus] = useState<"copied" | "copyFailed" | null>(null);
  const mounted = useRef(true);
  const generation = useRef(0);
  const loadedOlder = useRef(false);
  const savingRef = useRef(false);
  const preferenceChanged = useRef(onPreferenceChanged);
  preferenceChanged.current = onPreferenceChanged;

  const refresh = useCallback(async () => {
    if (savingRef.current) return;
    const version = ++generation.current;
    try {
      const [preference, page] = await Promise.all([accessApi.readPreference(), accessApi.networkLog(conversationId)]);
      if (!mounted.current || generation.current !== version) return;
      setPreference(preference.allowed);
      setLog((current) => loadedOlder.current ? { ...current, entries: mergeNetworkEntries(current.entries, page.entries) } : page);
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
    const timer = window.setInterval(() => void refresh(), 2000);
    const subscription = listen("access-read-preference-changed", () => {
      void refresh();
      preferenceChanged.current();
    }).catch(() => () => {});
    return () => { window.clearInterval(timer); void subscription.then((unlisten) => unlisten()); };
  }, [open, refresh]);

  async function changeReading(allowed: boolean) {
    if (saving) return;
    savingRef.current = true;
    generation.current += 1;
    setSaving(true);
    try {
      const next = await accessApi.setReadPreference(allowed);
      if (!mounted.current) return;
      setPreference(next.allowed);
      setError(false);
      setActionError(false);
      preferenceChanged.current();
    } catch {
      if (mounted.current) setActionError(true);
    } finally {
      savingRef.current = false;
      if (mounted.current) { setSaving(false); void refresh(); }
    }
  }

  async function loadOlder() {
    if (olderBusy || log.nextBefore === null) return;
    setOlderBusy(true);
    loadedOlder.current = true;
    try {
      const page = await accessApi.networkLog(conversationId, log.nextBefore);
      if (!mounted.current) return;
      setLog((current) => ({ ...page, entries: mergeNetworkEntries(current.entries, page.entries) }));
    } catch {
      if (mounted.current) setActionError(true);
    } finally {
      if (mounted.current) setOlderBusy(false);
    }
  }

  async function copy(destination: string) {
    try {
      await navigator.clipboard.writeText(destination);
      if (mounted.current) setCopyStatus("copied");
    } catch {
      if (mounted.current) setCopyStatus("copyFailed");
    }
  }

  const button = `rounded-md border border-line px-2 py-1 text-[13px] text-primary hover:bg-hover disabled:opacity-50 ${projectControlFocus}`;
  return <details className="mt-2" onToggle={(event) => setOpen(event.currentTarget.open)}>
    <summary className={`cursor-pointer ${projectControlFocus}`}>{t("access.title")}</summary>
    {open ? <div className="mt-2 max-h-80 space-y-3 overflow-y-auto border-t border-line pt-3 text-[14px] [&_p]:max-w-[75ch]" aria-label={t("access.title")}>
      <div>
        <h2 className="font-medium">{t("access.reading")}</h2>
        {loading ? <p role="status">{t("access.loading")}</p> : preference !== null ? <label className="mt-2 flex items-start gap-2">
          <input type="checkbox" checked={preference} disabled={saving || error || actionError} onChange={(event) => void changeReading(event.target.checked)} className={`mt-1 ${projectControlFocus}`} />
          <span>{t("access.allow")}</span>
        </label> : null}
        <p className="mt-2">{t("access.scope")}</p>
        <p className="mt-1">{t("access.limits")}</p>
        <p className="mt-1">{t("access.retained")}</p>
        {saving ? <p role="status">{t("access.saving")}</p> : null}
      </div>
      <div>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h2 className="font-medium">{t("access.network")}</h2>
          <button type="button" className={button} onClick={() => void refresh()} disabled={saving}>{t("access.refresh")}</button>
        </div>
        <p className="mt-1">{t("access.sanitized")}</p>
        <p className="mt-1">{t("access.blocked")}</p>
        {!loading && !error && !actionError && !log.entries.length ? <p className="mt-2">{t("access.empty")}</p> : null}
        <ol className="mt-2 divide-y divide-line">
          {log.entries.map((entry) => {
            const destination = safeNetworkDestination(entry.destination);
            const timestamp = new Date(entry.createdAt);
            return <li key={entry.id} className="py-2">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <span className="break-all font-medium">{destination ?? t("access.unavailable")}</span>
                {destination ? <button type="button" className={button} onClick={() => void copy(destination)}>{t("access.copy")}</button> : null}
              </div>
              <p>{t(`access.states.${entry.state}`)}</p>
              <p className="flex flex-wrap gap-x-3">
                <span>{entry.actionId !== null ? t("access.origin", { id: entry.actionId }) : t("access.recorded")}</span>
                {entry.runId !== null ? <span>{t("access.run", { id: entry.runId })}</span> : null}
                {entry.httpStatus !== null ? <span>{t("access.status", { status: entry.httpStatus })}</span> : null}
                {Number.isFinite(timestamp.getTime()) ? <time dateTime={timestamp.toISOString()}>{timestamp.toLocaleString(i18n.language)}</time> : null}
              </p>
            </li>;
          })}
        </ol>
        {log.hasMore ? <button type="button" className={button} disabled={olderBusy} onClick={() => void loadOlder()}>{t("access.more")}</button> : null}
        {copyStatus ? <p role="status">{t(`access.${copyStatus}`)}</p> : null}
      </div>
      {error || actionError ? <p role="alert">{t("access.error")} <button type="button" className={button} disabled={saving} onClick={() => { setActionError(false); void refresh(); }}>{t("access.retry")}</button></p> : null}
    </div> : null}
  </details>;
}
