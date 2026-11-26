import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { ToolsIcon } from "../../components/Icon";
import { cx } from "../../lib/cx";
import type { CatalogModel, FoundryCommandError, ModelJob } from "../../lib/foundry";
import { kindIcon, StateDot } from "./ModelBadges";
import {
  deviceLabel,
  errorLabel,
  formatSize,
  sortModels,
  stateLabel,
} from "./format";

type ModelListProps = {
  models: CatalogModel[];
  jobs: Record<string, ModelJob>;
  errors: Record<string, FoundryCommandError>;
  busyAlias: string | null;
  selectedAlias: string | null;
  onSelect: (alias: string) => void;
};

export function ModelList({
  models,
  jobs,
  errors,
  busyAlias,
  selectedAlias,
  onSelect,
}: ModelListProps) {
  const { t, i18n } = useTranslation();

  const groups = useMemo(() => {
    const grouped = new Map<string, CatalogModel[]>();

    for (const model of sortModels(models, i18n.language)) {
      const family = model.family ?? t("models.familyOther");
      const entries = grouped.get(family);

      if (entries) {
        entries.push(model);
      } else {
        grouped.set(family, [model]);
      }
    }

    return Array.from(grouped.entries()).map(([family, entries]) => ({
      family,
      models: entries,
    }));
  }, [models, i18n.language, t]);

  return (
    <div className="flex flex-col gap-1 pb-2">
      {groups.map((group) => (
        <section key={group.family} aria-label={group.family}>
          <h3 className="flex items-center gap-2 px-2 pb-1 pt-2.5 text-[10.5px] font-semibold uppercase tracking-[0.055em] text-secondary first:pt-1">
            <span className="truncate">{group.family}</span>
            <span
              aria-hidden="true"
              className="ml-auto rounded-full bg-subtle px-1.5 py-px text-[9px] font-medium tabular-nums tracking-normal text-tertiary"
            >
              {group.models.length}
            </span>
          </h3>
          <ul className="flex flex-col gap-0.5">
            {group.models.map((model) => {
              const selected = model.alias === selectedAlias;
              const KindIcon = kindIcon[model.kind];
              const meta = [
                model.fileSizeMb === null ? null : formatSize(model.fileSizeMb, i18n.language),
                deviceLabel(model.device),
              ]
                .filter((value): value is string => Boolean(value))
                .join(" · ");
              const job = jobs[model.alias];
              const error = errors[model.alias] ?? null;
              const downloading = job?.status === "running";
              const state = downloading ? t("models.jobs.downloading") : stateLabel(model, t, job);
              const busy = busyAlias === model.alias;
              const failed = job?.status === "failed" || error !== null;

              return (
                <li key={model.alias}>
                  <button
                    type="button"
                    onClick={() => onSelect(model.alias)}
                    aria-current={selected ? "true" : undefined}
                    className={cx(
                      "flex w-full flex-col gap-1 rounded-[var(--item-radius)] border px-[var(--item-pad-x)] py-[var(--item-pad-y)] text-left transition-colors",
                      selected
                        ? "border-line-strong bg-elevated shadow-[var(--shadow-thumb)]"
                        : "border-transparent hover:border-line hover:bg-hover",
                    )}
                  >
                    <span className="flex w-full min-w-0 items-center gap-2">
                      <span className="min-w-0 flex-1 truncate text-[12.5px] font-medium text-primary">
                        {model.alias}
                      </span>
                      <span className="flex shrink-0 items-center gap-1 text-tertiary">
                        <KindIcon size={13} />
                        {model.supportsToolCalling ? <ToolsIcon size={13} /> : null}
                      </span>
                    </span>
                    <span className="flex w-full min-w-0 items-center justify-between gap-2">
                      <span className="min-w-0 truncate text-[10.5px] tabular-nums text-secondary">{meta}</span>
                      <StateDot
                        loaded={model.loaded}
                        cached={model.cached}
                        label={state}
                        active={downloading || busy}
                        danger={failed}
                        title={error ? errorLabel(error, t) : undefined}
                      />
                    </span>
                    {selected && downloading ? (
                      <span className="h-1 w-full overflow-hidden rounded-full bg-active" aria-hidden="true">
                        <span className="block h-full w-2/5 rounded-full bg-accent indeterminate-progress" />
                      </span>
                    ) : null}
                  </button>
                </li>
              );
            })}
          </ul>
        </section>
      ))}
    </div>
  );
}
