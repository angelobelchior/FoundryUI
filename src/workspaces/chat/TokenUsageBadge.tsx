import { useTranslation } from "react-i18next";
import { Tooltip } from "../../components/Tooltip";
import type { TokenUsage } from "../../lib/chat";

type TokenUsageBadgeProps = {
  usage: TokenUsage;
};

export function TokenUsageBadge({ usage }: TokenUsageBadgeProps) {
  const { t, i18n } = useTranslation();
  const compact = new Intl.NumberFormat(i18n.language, {
    notation: "compact",
    maximumFractionDigits: 1,
  });
  const exact = new Intl.NumberFormat(i18n.language);

  if (usage.totalTokens <= 0) {
    return null;
  }

  const label = t("chat.tokensTooltip", {
    prompt: exact.format(usage.promptTokens),
    completion: exact.format(usage.completionTokens),
    total: exact.format(usage.totalTokens),
  });

  return (
    <Tooltip label={label} side="bottom">
      <span
        role="img"
        aria-label={label}
        className="inline-flex h-[18px] shrink-0 items-center rounded-[5px] border border-line px-1.5 text-[10.5px] leading-none tabular-nums text-tertiary"
      >
        {t("chat.usage", {
          prompt: compact.format(usage.promptTokens),
          completion: compact.format(usage.completionTokens),
        })}
      </span>
    </Tooltip>
  );
}
