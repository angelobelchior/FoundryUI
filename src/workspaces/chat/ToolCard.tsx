import { useState } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../../lib/cx";
import type { AgentToolCard as AgentToolCardModel } from "../../lib/agent";

export function AgentActivity({ tools }: { tools: AgentToolCardModel[] }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState<Record<string, boolean>>({});

  if (!tools.length) {
    return null;
  }

  return (
    <div className="mt-2 flex flex-col gap-1.5">
      {tools.map((tool) => (
        <ToolCard
          key={tool.callId}
          tool={tool}
          open={Boolean(open[tool.callId])}
          onToggle={() => setOpen((current) => ({ ...current, [tool.callId]: !current[tool.callId] }))}
          labels={{
            arguments: t("agent.arguments"),
            result: t("agent.result"),
            diff: t("agent.diff"),
          }}
          outcomeLabels={{
            succeeded: t("agent.outcomeSucceeded"),
            failed: t("agent.outcomeFailed"),
            cancelled: t("agent.outcomeCancelled"),
            pending: t("agent.outcomePending"),
          }}
        />
      ))}
    </div>
  );
}

type ToolCardProps = {
  tool: AgentToolCardModel;
  open: boolean;
  onToggle: () => void;
  labels: { arguments: string; result: string; diff: string };
  outcomeLabels: { succeeded: string; failed: string; cancelled: string; pending: string };
};

function ToolCard({ tool, open, onToggle, labels, outcomeLabels }: ToolCardProps) {
  const outcome = tool.outcome;
  const badge = outcome === "succeeded"
    ? { className: "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300", label: outcomeLabels.succeeded }
    : outcome === "failed"
      ? { className: "bg-danger/15 text-danger", label: outcomeLabels.failed }
      : outcome === "cancelled"
        ? { className: "bg-subtle text-tertiary", label: outcomeLabels.cancelled }
        : { className: "bg-accent/15 text-accent", label: outcomeLabels.pending };

  return (
    <div className="overflow-hidden rounded-lg border border-line bg-subtle/40">
      <button
        type="button"
        onClick={onToggle}
        aria-expanded={open}
        className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left hover:bg-hover"
      >
        <span aria-hidden="true" className="text-[10px] text-tertiary">{open ? "▾" : "▸"}</span>
        <span className="min-w-0 truncate font-mono text-[11.5px] font-medium text-primary">{tool.tool}</span>
        <span className={cx("ml-auto inline-flex shrink-0 items-center gap-1 rounded-full px-1.5 py-0.5 text-[10px] font-medium", badge.className)}>
          <span className="h-1 w-1 rounded-full bg-current" aria-hidden="true" />
          {badge.label}
        </span>
      </button>
      {open ? (
        <div className="space-y-2 border-t border-line px-2.5 py-2">
          <Field label={labels.arguments}>
            <pre className="whitespace-pre-wrap break-words font-mono text-[11px] leading-relaxed text-secondary">
              {formatValue(tool.arguments)}
            </pre>
          </Field>
          {tool.diff ? (
            <Field label={labels.diff}>
              <pre className="overflow-x-auto whitespace-pre font-mono text-[11px] leading-relaxed text-secondary">{tool.diff}</pre>
            </Field>
          ) : null}
          {tool.summary && tool.summary.status ? (
            <Field label={labels.result}>
              <pre className="whitespace-pre-wrap break-words font-mono text-[11px] leading-relaxed text-secondary">
                {formatValue(tool.summary)}
              </pre>
            </Field>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <p className="mb-0.5 text-[10px] font-semibold uppercase tracking-[0.05em] text-tertiary">{label}</p>
      {children}
    </div>
  );
}

function formatValue(value: unknown): string {
  if (value === null || value === undefined) {
    return "—";
  }

  if (typeof value === "string") {
    return value;
  }

  return JSON.stringify(value, null, 2);
}
