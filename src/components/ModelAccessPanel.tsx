import { AlertIcon } from "./Icon";
import { Spinner } from "./Spinner";

type ModelAccessPanelProps = {
  title: string;
  body: string;
  listTitle?: string;
  models: readonly string[];
  loadLabel: string;
  loadingLabel: string;
  openModelsLabel: string;
  loadingAlias: string | null;
  onLoad: (alias: string) => void;
  onOpenModels: () => void;
};

export function ModelAccessPanel({
  title,
  body,
  listTitle,
  models,
  loadLabel,
  loadingLabel,
  openModelsLabel,
  loadingAlias,
  onLoad,
  onOpenModels,
}: ModelAccessPanelProps) {
  return (
    <section className="rounded-[9px] border border-line bg-subtle px-3 py-3" role="status" aria-live="polite">
      <div className="flex items-start gap-2">
        <AlertIcon size={14} className="mt-0.5 shrink-0 text-secondary" />
        <div className="min-w-0 flex-1">
          <p className="text-[12px] font-medium text-primary">{title}</p>
          <p className="mt-1 text-[11.5px] leading-relaxed text-secondary">{body}</p>
        </div>
      </div>

      {models.length > 0 ? (
        <div className="mt-3 border-t border-line pt-3">
          {listTitle ? <p className="mb-2 text-[10.5px] text-tertiary">{listTitle}</p> : null}
          <ul className="space-y-1.5">
            {models.map((alias) => {
              const loading = loadingAlias === alias;

              return (
                <li key={alias} className="flex items-center justify-between gap-2 rounded-[7px] border border-line bg-canvas px-2 py-1.5">
                  <span className="min-w-0 truncate text-[11.5px] text-primary">{alias}</span>
                  <button
                    type="button"
                    onClick={() => onLoad(alias)}
                    disabled={loadingAlias !== null}
                    className="inline-flex h-7 shrink-0 items-center gap-1.5 rounded-[7px] border border-line px-2.5 text-[11px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary disabled:opacity-50"
                  >
                    {loading ? <Spinner size={10} /> : null}
                    {loading ? loadingLabel : loadLabel}
                  </button>
                </li>
              );
            })}
          </ul>
        </div>
      ) : null}

      <div className="mt-3 flex justify-end">
        <button
          type="button"
          onClick={onOpenModels}
          className="inline-flex h-7 items-center rounded-[7px] border border-line px-2.5 text-[11px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary"
        >
          {openModelsLabel}
        </button>
      </div>
    </section>
  );
}
