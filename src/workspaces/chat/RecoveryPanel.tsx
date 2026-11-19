import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Modal } from "../../components/Modal";
import { recoveryApi, type RecoverableChange } from "../../lib/recovery";
import { projectControlFocus } from "./projectFocus";

type Notice = { kind: "conflict" | "undone" | "unavailable" | "noProtection" | "rootUnavailable" | "actionError"; path: string } | null;

export function RecoveryPanel({ conversationId, onChanged }: { conversationId: number; onChanged: () => void }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [changes, setChanges] = useState<RecoverableChange[] | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState(false);
  const [undone, setUndone] = useState<Set<number>>(new Set());
  const [undoing, setUndoing] = useState<number | null>(null);
  const [confirming, setConfirming] = useState<RecoverableChange | null>(null);
  const [notice, setNotice] = useState<Notice>(null);
  const mounted = useRef(true);
  const generation = useRef(0);
  const onChangedRef = useRef(onChanged);
  onChangedRef.current = onChanged;

  const refresh = useCallback(async () => {
    const version = ++generation.current;
    try {
      const list = await recoveryApi.list(conversationId, null);
      if (!mounted.current || generation.current !== version) return;
      setChanges(list);
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

  async function undo(change: RecoverableChange, confirmedDestructive: boolean) {
    if (undoing !== null) return;
    setUndoing(change.actionId);
    setNotice(null);
    try {
      await recoveryApi.undo(conversationId, change.actionId, confirmedDestructive);
      if (!mounted.current) return;
      setUndone((current) => new Set(current).add(change.actionId));
      setNotice({ kind: "undone", path: change.path });
      onChangedRef.current();
      await refresh();
    } catch (caught) {
      if (!mounted.current) return;
      const code = (caught as { code?: string })?.code ?? "actionError";
      setNotice({ kind: (code === "conflict" ? "conflict" : code === "no_protection" ? "noProtection" : code === "root_unavailable" ? "rootUnavailable" : code === "unavailable" ? "unavailable" : "actionError"), path: change.path });
      await refresh();
    } finally {
      if (mounted.current) setUndoing(null);
    }
  }

  const button = `rounded-md border border-line px-2 py-1 text-[13px] text-primary hover:bg-hover disabled:opacity-50 ${projectControlFocus}`;
  const operationLabels: Record<string, string> = {
    create: t("recovery.create"),
    edit: t("recovery.edit"),
    delete: t("recovery.delete"),
  };
  const noticeLabels: Record<NonNullable<Notice>["kind"], string> = {
    conflict: t("recovery.conflict"),
    undone: t("recovery.undone"),
    unavailable: t("recovery.unavailable"),
    noProtection: t("recovery.noProtection"),
    rootUnavailable: t("recovery.rootUnavailable"),
    actionError: t("recovery.actionError"),
  };
  const groups = new Map<number, RecoverableChange[]>();
  for (const change of changes ?? []) {
    const runId = change.runId ?? 0;
    const existing = groups.get(runId) ?? [];
    existing.push(change);
    groups.set(runId, existing);
  }

  return <details className="mt-2" onToggle={(event) => setOpen(event.currentTarget.open)}>
    <summary className={`cursor-pointer ${projectControlFocus}`}>{t("recovery.title")}</summary>
    {open ? <div className="mt-2 max-h-96 space-y-4 overflow-y-auto border-t border-line pt-3 text-[14px] [&_p]:max-w-[75ch]" aria-label={t("recovery.title")}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <button type="button" className={button} onClick={() => void refresh()}>{t("recovery.refresh")}</button>
        {loading ? <span role="status">{t("recovery.loading")}</span> : null}
      </div>
      {error ? <p role="alert">{t("recovery.error")} <button type="button" className={button} onClick={() => void refresh()}>{t("recovery.retry")}</button></p> : null}
      {!loading && !error && !changes?.length ? <p>{t("recovery.empty")}</p> : null}
      {[...groups.entries()].map(([runId, entries]) => <div key={runId} className="border-t border-line pt-3">
        {runId !== 0 ? <h2 className="font-medium">{t("recovery.run", { id: runId })}</h2> : null}
        <ul className="mt-1 divide-y divide-line">
          {entries.map((change) => <li key={change.actionId} className="py-2">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <span className="min-w-0 break-all">
                <span className="font-medium">{operationLabels[change.operation]}</span>
                <span className="text-secondary"> · {change.path}</span>
                <span className="text-secondary"> · +{change.additions}/−{change.deletions}</span>
              </span>
              {change.protected && !undone.has(change.actionId) ? <button type="button" className={button} disabled={undoing !== null} onClick={() => (change.destructive ? setConfirming(change) : void undo(change, false))}>{undoing === change.actionId ? t("recovery.undoing") : t("recovery.undo")}</button> : null}
            </div>
            {!change.protected ? <p className="mt-1 text-[13px] text-secondary">{t("recovery.noRecovery")}</p> : null}
            <pre className="mt-2 max-h-40 overflow-y-auto whitespace-pre-wrap break-words rounded-md border border-line bg-canvas p-2 text-[12px]">{change.diff}</pre>
            {change.diffTruncated ? <p className="mt-1 text-[12px] text-secondary">{t("recovery.truncated")}</p> : null}
          </li>)}
        </ul>
      </div>)}
      {notice ? <p role="status" className="mt-2">{noticeLabels[notice.kind]}</p> : null}
    </div> : null}
    <Modal open={confirming !== null} title={t("recovery.confirmTitle")} onClose={() => { if (undoing === null) setConfirming(null); }}>
      <p className="text-[14px] leading-relaxed text-primary">{t("recovery.confirmDestructive")}</p>
      <div className="mt-4 flex flex-wrap justify-end gap-2">
        <button type="button" className={button} disabled={undoing !== null} onClick={() => setConfirming(null)}>{t("recovery.cancel")}</button>
        <button type="button" className={button} disabled={undoing !== null} onClick={() => {
          const change = confirming;
          setConfirming(null);
          if (change) void undo(change, true);
        }}>{t("recovery.confirm")}</button>
      </div>
    </Modal>
  </details>;
}
