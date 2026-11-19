import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Modal } from "../../components/Modal";
import { executionStage } from "../../lib/execution";
import { projectControlFocus } from "./projectFocus";
import type { useAgentExecution } from "./useAgentExecution";
import { ReadNetworkPanel } from "./ReadNetworkPanel";

export function AgentExecution({ agent, compatible, disabled, hasAttachments }: {
  agent: ReturnType<typeof useAgentExecution>;
  compatible: boolean;
  disabled: boolean;
  hasAttachments: boolean;
}) {
  const { t } = useTranslation();
  const [confirm, setConfirm] = useState<{ owner: number; run: number; title: string } | null>(null);
  const { status } = agent;
  const stage = status ? executionStage(status) : "idle";
  const blocker = status?.blocker;
  const control = `rounded-md border border-line px-2 py-1 text-[13px] text-primary hover:bg-hover disabled:opacity-50 ${projectControlFocus}`;

  return (
    <section aria-label={t("execution.title")} className="shrink-0 border-t border-line px-5 py-3 text-[13px] leading-relaxed text-primary">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
        <label className="inline-flex items-center gap-2">
          <input type="checkbox" checked={agent.enabled} disabled={disabled || agent.busy || (!agent.enabled && (!compatible || hasAttachments || !status || agent.error))}
            onChange={(event) => agent.setEnabled(event.target.checked)} className={projectControlFocus} />
          {t("execution.enabled")}
        </label>
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
      {agent.enabled && hasAttachments ? <p className="mt-2">{t("execution.attachments")}</p> : null}
      {agent.enabled || status?.execution ? <p className="mt-2">{t("execution.limits")}</p> : null}
      {stage === "pending" ? <p className="mt-2">{t(status?.continuationAvailable ? "execution.livePendingHint" : "execution.pendingHint")}</p> : null}
      {status?.pendingRequests?.length ? <div className="mt-2 max-h-48 overflow-y-auto">
        {status.pendingRequests.map((request) => {
          const previewAvailable = request.requirements.length > 0 && request.requirements.every((requirement) => requirement.previewAvailable);
          return <div key={request.id} className="border-t border-line py-2">
            <p className="font-medium">{t("execution.request", { id: request.id })}</p>
            {request.requirements.map((requirement, index) => <p key={`${requirement.constraintsFingerprint}-${index}`} className="whitespace-pre-wrap break-words">{requirement.target}</p>)}
            {!previewAvailable ? <p className="mt-1">{t("execution.noPreview")}</p> : null}
            <div className="mt-2 flex flex-wrap gap-2">
              <button type="button" className={control} disabled={agent.deciding !== null || agent.stopping || !status.continuationAvailable || !previewAvailable}
                onClick={() => void agent.decide(request.id, { decision: "approve", remember: false })}>{t("execution.allowOnce")}</button>
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
      {status ? <ReadNetworkPanel key={status.conversationId} conversationId={status.conversationId} onPreferenceChanged={() => void agent.refresh()} /> : null}
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
