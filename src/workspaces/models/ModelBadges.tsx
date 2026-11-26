import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ChatIcon, EmbeddingsIcon, ModelsIcon, SpeechIcon, ToolsIcon } from "../../components/Icon";
import { Tooltip } from "../../components/Tooltip";
import { cx } from "../../lib/cx";
import type { Declared, ModelKind } from "../../lib/foundry";

export const kindIcon: Record<ModelKind, (props: { size?: number; className?: string }) => ReactNode> = {
  chat: ChatIcon,
  multimodal: ModelsIcon,
  speech: SpeechIcon,
  embedding: EmbeddingsIcon,
};

export function KindBadge({ kind }: { kind: ModelKind }) {
  const { t } = useTranslation();
  const Icon = kindIcon[kind];

  return (
    <span className="inline-flex h-[19px] shrink-0 items-center gap-1 rounded-[5px] bg-subtle px-1.5 text-[9.5px] font-medium uppercase tracking-[0.045em] text-secondary">
      <span className="inline-flex h-3.5 w-3.5 items-center justify-center rounded-[4px] bg-elevated text-tertiary">
        <Icon size={10} />
      </span>
      {t(`models.kinds.${kind}`)}
    </span>
  );
}

export function ModalityChip({ children }: { children: ReactNode }) {
  return (
    <span className="inline-flex h-[21px] items-center rounded-[5px] bg-subtle px-2 text-[11px] font-medium text-secondary">
      {children}
    </span>
  );
}

export function ToolBadge({ supports }: { supports: boolean }) {
  const { t } = useTranslation();

  return (
    <span
      title={supports ? t("models.tools.supported") : t("models.tools.notSupported")}
      className={cx(
        "inline-flex h-[17px] shrink-0 items-center gap-1 rounded-[5px] px-1.5 text-[9px] font-medium uppercase tracking-[0.04em]",
        supports ? "bg-accent/10 text-accent-text" : "bg-subtle text-tertiary",
      )}
    >
      <ToolsIcon size={9} className={supports ? "text-accent" : "text-tertiary"} />
      {supports ? t("models.tools.supported") : t("models.tools.notSupported")}
    </span>
  );
}

export function StateDot({
  loaded,
  cached,
  label,
  active = false,
  danger = false,
  title,
}: {
  loaded: boolean;
  cached: boolean;
  label: string;
  active?: boolean;
  danger?: boolean;
  title?: string | undefined;
}) {
  return (
    <span
      className="inline-flex shrink-0 items-center gap-1 text-[10.5px] text-tertiary"
      title={title}
    >
      <span
        className={cx(
          "h-1.5 w-1.5 rounded-full",
          danger
            ? "bg-danger"
            : active || loaded
              ? active
                ? "animate-pulse bg-accent"
                : "bg-accent"
              : cached
                ? "bg-success"
                : "bg-tertiary",
        )}
      />
      {label}
    </span>
  );
}

export function StateBadge({
  loaded,
  cached,
  label,
  active = false,
}: {
  loaded: boolean;
  cached: boolean;
  label: string;
  active?: boolean;
}) {
  return (
    <span className="inline-flex min-w-[88px] flex-col gap-1">
      <span className="inline-flex h-[21px] items-center gap-1.5 rounded-[5px] bg-subtle px-2 text-[11px] font-medium text-secondary">
        <span
          className={cx(
            "h-1.5 w-1.5 rounded-full",
            active ? "animate-pulse bg-accent" : loaded ? "bg-accent" : cached ? "bg-success" : "bg-tertiary",
          )}
        />
        {label}
      </span>
      {active ? (
        <span
          className="h-1 overflow-hidden rounded-full bg-active"
          role="progressbar"
          aria-label={label}
          aria-valuemin={0}
          aria-valuemax={100}
        >
          <span className="block h-full w-2/5 rounded-full bg-accent indeterminate-progress" />
        </span>
      ) : null}
    </span>
  );
}

export function OriginMark({ declared }: { declared: Declared<unknown> }) {
  const { t } = useTranslation();

  if (declared.origin !== "curated") {
    return null;
  }

  return (
    <Tooltip
      title={t("models.curatedSourceTitle")}
      label={t("models.curatedSource", {
        source: declared.source ?? t("models.curatedUnknownSource"),
      })}
      side="bottom"
    >
      <span className="ml-1.5 cursor-default text-[9px] font-medium uppercase tracking-[0.04em] text-tertiary underline decoration-tertiary/50 decoration-dotted underline-offset-2">
        {t("models.curated")}
      </span>
    </Tooltip>
  );
}
