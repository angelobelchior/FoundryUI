import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Modal } from "../../components/Modal";
import { cx } from "../../lib/cx";
import { executionStage } from "../../lib/execution";
import { projectControlFocus } from "./projectFocus";
import type { useAgentExecution } from "./useAgentExecution";
import { ReadNetworkPanel } from "./ReadNetworkPanel";
import { PermissionsPanel } from "./PermissionsPanel";
import { ContextPanel } from "./ContextPanel";
import { SnapshotPanel } from "./SnapshotPanel";
import { RecoveryPanel } from "./RecoveryPanel";

type PanelTab = "access" | "permissions" | "context" | "snapshots" | "recovery";

export function AgentExecution({ agent, compatible, hasAttachments }: {
  agent: ReturnType<typeof useAgentExecution>;
  compatible: boolean;
  hasAttachments: boolean;
}) {
  const { t } = useTranslation();
  const [confirm, setConfirm] = useState<{ owner: number; run: number; title: string } | null>(null);
  const [tab, setTab] = useState<PanelTab>("access");
  const { status } = agent;
  const stage = status ? executionStage(status) : "idle";
  const blocker = status?.blocker;
  const control = `rounded-md border border-line px-2 py-1 text-[13px] text-primary hover:bg-hover disabled:opacity-50 ${projectControlFocus}`;
  const tabs: { id: PanelTab; label: string }[] = [
    { id: "access", label: t("access.tab") },
    { id: "permissions", label: t("permissions.tab") },
    { id: "context", label: t("context.tab") },
    { id: "snapshots", label: t("snapshots.tab") },
    { id: "recovery", label: t("recovery.tab") },
  ];

  return (
    <section aria-label={t("execution.title")} className="flex min-h-0 flex-1 flex-col text-[13px] leading-relaxed text-primary">
      <div className="shrink-0 px-3 py-2">
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <label className="inline-flex items-center gap-2">
            {t("execution.mode")}
            <select value={status?.mode ?? "review"} disabled={!status || agent.changingMode || agent.error}
              title={t("execution.modeHint")}
              onChange={(event) => void agent.mode(event.target.value === "automatic" ? "automatic" : "review")}
              className="rounded-md border border-line bg-canvas px-2 py-1 text-primary focus-visible:outline-2 focus-visible:outline-offset-2">
              <option value="review">{t("execution.review")}</option>
              <option value="automatic">{t("execution.automatic")}</option>
            </select>
          </label>
          <span role="status">{agent.stopping ? t("execution.stopping") : !status ? t("execution.loading") : t(`execution.states.${stage}`)}</span>
          {status?.execution && ["prepared", "running"].includes(status.execution.run.state) ? (
            <button type="button" className={control} disabled={agent.stopping} onClick={() => void agent.cancel()}>{t("execution.stop")}</button>
          ) : null}
        </div>
        {!compatible ? <p className="mt-2">{t("execution.incompatible")}</p> : null}
        {hasAttachments ? <p className="mt-2">{t("execution.attachments")}</p> : null}
        {status?.execution ? <p className="mt-2">{t("execution.limits")}</p> : null}
        {stage === "pending" ? <p className="mt-2">{t(status?.continuationAvailable ? "execution.livePendingHint" : "execution.pendingHint")}</p> : null}
        {status?.pendingRequests?.length ? <div className="mt-2 max-h-48 overflow-y-auto">
          {status.pendingRequests.map((request) => {
            const previewAvailable = request.requirements.length > 0 && request.requirements.every((requirement) => requirement.previewAvailable);
            const rememberEligible = request.requirements.length > 0 && request.requirements.every((requirement) => requirement.rememberEligible);
            return <div key={request.id} className="border-t border-line py-2">
              <p className="font-medium">{t("execution.request", { id: request.id })}</p>
              {request.requirements.map((requirement, index) => (
                <div key={`${requirement.constraintsFingerprint}-${index}`}>
                  <p className="whitespace-pre-wrap break-words">{requirement.target}</p>
                  {requirement.diff ? (
                    <pre className="mt-1 max-h-56 overflow-auto whitespace-pre rounded border border-line bg-subtle px-2 py-1.5 font-mono text-[11px] leading-relaxed text-secondary">{requirement.diff}</pre>
                  ) : null}
                </div>
              ))}
              {!previewAvailable ? <p className="mt-1">{t("execution.noPreview")}</p> : null}
              <div className="mt-2 flex flex-wrap gap-2">
                <button type="button" className={control} disabled={agent.deciding !== null || agent.stopping || !status.continuationAvailable || !previewAvailable}
                  onClick={() => void agent.decide(request.id, { decision: "approve", remember: false })}>{t("execution.allowOnce")}</button>
                {rememberEligible ? <button type="button" className={control} disabled={agent.deciding !== null || agent.stopping || !status.continuationAvailable || !previewAvailable}
                  onClick={() => void agent.decide(request.id, { decision: "approve", remember: true })}>{t("execution.remember")}</button> : null}
                <button type="button" className={control} disabled={agent.deciding !== null || agent.stopping}
                  onClick={() => void agent.decide(request.id, { decision: "deny" })}>{t("execution.deny")}</button>
                {agent.deciding === request.id ? <span role="status">{t("execution.waitingDecision")}</span> : null}
              </div>
            </div>;
          })}
        </div> : null}
        {stage === "uncertain" ? <p role="status" className="mt-2">{t("execution.uncertainHint")}</p> : null}
        {blocker ? <div className="mt-2 flex flex-wrap items-center gap-2">
          <span>{t("execution.occupied", { title: status.blockerTitle || t("chat.untitled"), id: blocker.conversation_id })}</span>
          <button type="button" className={control} onClick={() => void agent.refresh()}>{t("execution.wait")}</button>
          {["prepared", "running"].includes(blocker.state) ? <button type="button" className={control} onClick={() => setConfirm({ owner: blocker.conversation_id, run: blocker.run_id, title: status.blockerTitle || t("chat.untitled") })}>{t("execution.interrupt")}</button> : <span>{t("execution.uncertainHint")}</span>}
        </div> : null}
        {agent.error && confirm === null ? <p role="alert" className="mt-2">{t("execution.error")} <button type="button" className={control} onClick={agent.retry}>{t("execution.retry")}</button></p> : null}
        {status?.execution?.events.length ? <details className="mt-2">
          <summary className={`cursor-pointer ${projectControlFocus}`}>{t("execution.progress")} · {t("execution.run", { id: status.execution.run.id })}</summary>
          <ol className="mt-2 max-h-36 overflow-y-auto pl-4">
            {status.execution.events.map((event) => <li key={event.sequence} className="flex flex-wrap gap-x-3 py-0.5">
              <span className="tabular-nums">{event.sequence}</span>
              <span>{t(`execution.events.${event.event}`, { defaultValue: event.event })}</span>
              {event.actionId !== null ? <span>{t("execution.action", { id: event.actionId })}</span> : null}
            </li>)}
          </ol>
        </details> : null}
      </div>
      {status ? (
        <>
          <div role="tablist" aria-label={t("execution.title")} className="flex shrink-0 items-center gap-0.5 overflow-x-auto border-b border-line px-2 pb-1">
            {tabs.map((entry) => (
              <button
                key={entry.id}
                type="button"
                role="tab"
                id={`execution-tab-${entry.id}`}
                aria-selected={entry.id === tab}
                aria-controls={`execution-panel-${entry.id}`}
                onClick={() => setTab(entry.id)}
                className={cx(
                  "h-7 shrink-0 whitespace-nowrap rounded-[6px] px-2.5 text-[11.5px] font-medium transition-colors",
                  entry.id === tab ? "bg-subtle text-primary" : "text-secondary hover:bg-hover hover:text-primary",
                )}
              >
                {entry.label}
              </button>
            ))}
          </div>
          <div role="tabpanel" id={`execution-panel-${tab}`} aria-labelledby={`execution-tab-${tab}`} className="min-h-0 flex-1 overflow-y-auto px-3 py-2">
            {tab === "access" ? <ReadNetworkPanel key={status.conversationId} conversationId={status.conversationId} onPreferenceChanged={() => void agent.refresh()} /> : null}
            {tab === "permissions" ? <PermissionsPanel key={`permissions-${status.conversationId}`} conversationId={status.conversationId} onChanged={() => void agent.refresh()} /> : null}
            {tab === "context" ? <ContextPanel key={`context-${status.conversationId}`} conversationId={status.conversationId} /> : null}
            {tab === "snapshots" ? <SnapshotPanel key={`snapshots-${status.conversationId}`} conversationId={status.conversationId} /> : null}
            {tab === "recovery" ? <RecoveryPanel key={`recovery-${status.conversationId}`} conversationId={status.conversationId} onChanged={() => void agent.refresh()} /> : null}
          </div>
        </>
      ) : null}
      <Modal open={confirm !== null} title={t("execution.confirm", { title: confirm?.title ?? "" })} onClose={() => { if (!agent.stopping) setConfirm(null); }}>
        <p className="text-[14px] leading-relaxed text-primary">{t("execution.confirmHint")}</p>
        {agent.error ? <p role="alert" className="mt-3 text-[14px] leading-relaxed text-primary">{t("execution.error")}</p> : null}
        <div className="mt-4 flex flex-wrap justify-end gap-2">
          <button type="button" className={control} disabled={agent.stopping} onClick={() => setConfirm(null)}>{t("execution.cancel")}</button>
          <button type="button" className={control} disabled={agent.stopping} onClick={() => {
            if (confirm) void agent.cancel(confirm.owner, confirm.run).then((cancelled) => { if (cancelled) setConfirm(null); });
          }}>{agent.stopping ? t("execution.stopping") : t("execution.stop")}</button>
        </div>
      </Modal>
    </section>
  );
}
