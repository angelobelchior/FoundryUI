import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { snapshotApi, type ConversationSnapshot, type SnapshotCandidate } from "../../lib/snapshots";
import { projectControlFocus } from "./projectFocus";

export function SnapshotPanel({ conversationId }: { conversationId: number }) {
  const { t, i18n } = useTranslation();
  const [attached, setAttached] = useState<ConversationSnapshot[] | null>(null);
  const [query, setQuery] = useState("");
  const [candidates, setCandidates] = useState<SnapshotCandidate[]>([]);
  const [searching, setSearching] = useState(false);
  const [attaching, setAttaching] = useState<number | null>(null);
  const [removing, setRemoving] = useState<number | null>(null);
  const [viewing, setViewing] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  const [actionError, setActionError] = useState(false);
  const mounted = useRef(true);
  const generation = useRef(0);
  const searchTimer = useRef<number | null>(null);

  const refresh = useCallback(async () => {
    const version = ++generation.current;
    try {
      const list = await snapshotApi.list(conversationId);
      if (!mounted.current || generation.current !== version) return;
      setAttached(list);
      setError(false);
    } catch {
      if (mounted.current && generation.current === version) setError(true);
    } finally {
      if (mounted.current && generation.current === version) setLoading(false);
    }
  }, [conversationId]);

  const search = useCallback(async (value: string) => {
    const version = ++generation.current;
    setSearching(true);
    try {
      const list = await snapshotApi.candidates(value || null);
      if (!mounted.current || generation.current !== version) return;
      setCandidates(list);
    } catch {
      if (mounted.current && generation.current === version) setActionError(true);
    } finally {
      if (mounted.current && generation.current === version) setSearching(false);
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; generation.current += 1; if (searchTimer.current !== null) window.clearTimeout(searchTimer.current); };
  }, []);

  useEffect(() => {
    void refresh();
    if (searchTimer.current !== null) window.clearTimeout(searchTimer.current);
    searchTimer.current = window.setTimeout(() => void search(query), 200);
    return () => { if (searchTimer.current !== null) window.clearTimeout(searchTimer.current); };
  }, [refresh, search, query]);

  async function capture(sourceId: number) {
    if (attaching !== null) return;
    setAttaching(sourceId);
    try {
      const captured = await snapshotApi.capture(sourceId, conversationId);
      if (!mounted.current) return;
      setActionError(false);
      if (captured) await refresh();
    } catch {
      if (mounted.current) setActionError(true);
    } finally {
      if (mounted.current) setAttaching(null);
    }
  }

  async function remove(id: number) {
    if (removing !== null) return;
    setRemoving(id);
    try {
      await snapshotApi.remove(conversationId, id);
      if (!mounted.current) return;
      setActionError(false);
      await refresh();
    } catch {
      if (mounted.current) setActionError(true);
    } finally {
      if (mounted.current) setRemoving(null);
    }
  }

  const button = `rounded-md border border-line px-2 py-1 text-[13px] text-primary hover:bg-hover disabled:opacity-50 ${projectControlFocus}`;
  const input = `rounded-md border border-line bg-canvas px-2 py-1 text-primary focus-visible:outline-2 focus-visible:outline-offset-2 ${projectControlFocus}`;

  return <div className="space-y-4 text-[14px] [&_p]:max-w-[75ch]">
      <div>
        <label className="flex items-center gap-2">
          <span>{t("snapshots.search")}</span>
          <input type="text" value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t("snapshots.searchPlaceholder")} className={input} />
          {searching ? <span role="status">{t("snapshots.searching")}</span> : null}
        </label>
        <p className="mt-1">{t("snapshots.searchHint")}</p>
        {!query ? null : <ul className="mt-2 divide-y divide-line">
          {candidates.length ? candidates.map((candidate) => <li key={candidate.origin.conversationId} className="flex flex-wrap items-center justify-between gap-2 py-2">
            <span className="min-w-0">
              <span className="font-medium">{candidate.origin.title}</span>
              {candidate.origin.projectName ? <span className="text-secondary"> · {candidate.origin.projectName}</span> : null}
              {candidate.archived ? <span className="text-secondary"> · archived</span> : null}
            </span>
            <button type="button" className={button} disabled={attaching !== null} onClick={() => void capture(candidate.origin.conversationId)}>{attaching === candidate.origin.conversationId ? t("snapshots.attaching") : t("snapshots.attach")}</button>
          </li>) : <li className="py-2 text-secondary">{t("snapshots.candidatesEmpty")}</li>}
        </ul>}
      </div>
      <div>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h2 className="font-medium">{t("snapshots.title")}</h2>
          <button type="button" className={button} onClick={() => void refresh()}>{t("snapshots.refresh")}</button>
        </div>
        {loading ? <p role="status">{t("snapshots.loading")}</p> : null}
        {!loading && !error && !attached?.length ? <p className="mt-2">{t("snapshots.empty")}</p> : null}
        <ul className="mt-2 divide-y divide-line">
          {attached?.map((snapshot) => <li key={snapshot.id} className="py-2">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <span className="min-w-0 break-all font-medium">{snapshot.sourceTitle}</span>
              <span className="flex flex-wrap gap-2">
                <button type="button" className={button} disabled={removing !== null} onClick={() => setViewing((current) => (current === snapshot.id ? null : snapshot.id))}>{viewing === snapshot.id ? t("snapshots.hide") : t("snapshots.view")}</button>
                <button type="button" className={button} disabled={removing !== null} onClick={() => void remove(snapshot.id)}>{removing === snapshot.id ? t("snapshots.removing") : t("snapshots.remove")}</button>
              </span>
            </div>
            <p className="mt-1 text-[12px] text-secondary">{t("snapshots.capturedAt", { time: new Date(snapshot.capturedAt).toLocaleString(i18n.language) })}</p>
            {snapshot.crossedProjectBoundary ? <p className="mt-1">{t("snapshots.crossed")}</p> : null}
            <p className="mt-1">{t("snapshots.independent")}</p>
            {snapshot.omittedAttachmentCount || snapshot.omissions.length ? <p className="mt-1">{t("snapshots.omissions", { count: snapshot.omittedAttachmentCount + snapshot.omissions.length })}</p> : null}
            <p className="mt-1 text-[12px] text-secondary">{t("snapshots.removeHint")}</p>
            {viewing === snapshot.id ? <ol className="mt-2 max-h-48 overflow-y-auto divide-y divide-line">
              {snapshot.entries.map((entry) => <li key={entry.sequence} className="py-1 text-[12px]">
                {entry.content.kind === "historical_text" ? <p className="whitespace-pre-wrap break-words">{entry.content.content}</p>
                  : entry.content.kind === "historical_tool_call" ? <p className="text-secondary">{entry.content.sourceTool}</p>
                    : entry.content.kind === "historical_tool_result" ? <p className="text-secondary">{JSON.stringify(entry.content.summary)}</p>
                      : <p className="text-secondary">{entry.content.event}</p>}
              </li>)}
            </ol> : null}
          </li>)}
        </ul>
      </div>
      {error || actionError ? <p role="alert">{t("snapshots.error")} <button type="button" className={button} onClick={() => { setActionError(false); void refresh(); }}>{t("snapshots.retry")}</button></p> : null}
  </div>;
}
