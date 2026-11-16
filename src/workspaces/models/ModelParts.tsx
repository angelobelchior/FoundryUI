import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { CatalogModel, Declared, ModelJob, ModelVariant } from "../../lib/foundry";
import {
  deviceLabel,
  executionProviderLabel,
  formatSize,
  modalityLabel,
  stateLabel,
} from "./format";
import { ModalityChip, OriginMark, StateBadge, StateDot } from "./ModelBadges";

export function SectionTitle({ children }: { children: ReactNode }) {
  return (
    <h3 className="text-[11px] font-semibold uppercase tracking-[0.055em] text-tertiary">{children}</h3>
  );
}

export function Stat({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="min-w-0 bg-canvas px-3 py-2.5">
      <dt className="truncate text-[10px] font-medium uppercase tracking-[0.045em] text-tertiary">
        {label}
      </dt>
      <dd className="mt-1 truncate text-[13.5px] font-semibold text-primary">{children}</dd>
    </div>
  );
}

export function ModelStats({
  model,
  job,
  fourth,
}: {
  model: CatalogModel;
  job?: ModelJob | undefined;
  fourth: ReactNode;
}) {
  const { t, i18n } = useTranslation();
  const downloading = job?.status === "running";

  return (
    <dl className="mt-4 grid grid-cols-1 gap-px overflow-hidden rounded-[9px] border border-line bg-line min-[420px]:grid-cols-2 lg:grid-cols-4">
      <Stat label={t("models.fields.size")}>
        {model.fileSizeMb === null ? (
          <span className="font-normal text-tertiary">{t("models.notDeclared")}</span>
        ) : (
          formatSize(model.fileSizeMb, i18n.language)
        )}
      </Stat>
      <Stat label={t("models.fields.device")}>
        {model.device ? (
          deviceLabel(model.device)
        ) : (
          <span className="font-normal text-tertiary">{t("models.notDeclared")}</span>
        )}
      </Stat>
      <Stat label={t("models.fields.state")}>
        <StateBadge
          loaded={model.loaded}
          cached={model.cached}
          label={stateLabel(model, t, job)}
          active={downloading}
        />
      </Stat>
      {fourth}
    </dl>
  );
}

export function CapabilitiesList({ model }: { model: CatalogModel }) {
  const { t } = useTranslation();

  return (
    <section className="mt-6">
      <SectionTitle>{t("models.sections.capabilities")}</SectionTitle>
      <dl className="mt-3 grid grid-cols-1 items-start gap-y-2 text-[12.5px] sm:grid-cols-[136px_1fr] sm:gap-y-2.5">
        <dt className="text-tertiary sm:pt-0.5">{t("models.fields.input")}</dt>
        <dd>
          <DeclaredChips declared={model.inputModalities} />
        </dd>
        <dt className="text-tertiary sm:pt-0.5">{t("models.fields.output")}</dt>
        <dd>
          <DeclaredChips declared={model.outputModalities} />
        </dd>
        <dt className="text-tertiary sm:pt-0.5">{t("models.fields.tools")}</dt>
        <dd className="text-secondary">
          {model.supportsToolCalling ? t("models.yes") : t("models.no")}
        </dd>
      </dl>
    </section>
  );
}

export function DeclaredChips({ declared }: { declared: Declared<string[]> }) {
  const { t } = useTranslation();

  if (!declared.value?.length) {
    return <span className="text-[12.5px] text-tertiary">{t("models.notDeclared")}</span>;
  }

  return (
    <span className="flex flex-wrap items-center gap-1">
      {declared.value.map((modality) => (
        <ModalityChip key={modality}>{modalityLabel(modality, t)}</ModalityChip>
      ))}
      <OriginMark declared={declared} />
    </span>
  );
}

export function VariantList({ variants }: { variants: ModelVariant[] }) {
  const { t, i18n } = useTranslation();

  if (!variants.length) {
    return <p className="text-[12.5px] text-tertiary">{t("models.notDeclared")}</p>;
  }

  return (
    <ul className="overflow-hidden rounded-[8px] border border-line">
      {variants.map((variant) => {
        const device = deviceLabel(variant.device);
        const provider = executionProviderLabel(variant.executionProvider);
        const meta = [
          device,
          provider !== device ? provider : null,
          variant.fileSizeMb === null ? null : formatSize(variant.fileSizeMb, i18n.language),
        ]
          .filter((value): value is string => Boolean(value))
          .join(" · ");

        return (
          <li
            key={variant.id}
            className="flex flex-col gap-1.5 border-b border-line bg-canvas px-3 py-2.5 last:border-b-0 sm:flex-row sm:items-center sm:justify-between sm:gap-3"
          >
            <div className="min-w-0">
              <p className="truncate font-mono text-[10.5px] text-secondary">
                {variant.displayName ?? variant.id}
              </p>
              <p className="mt-0.5 truncate text-[10px] text-tertiary">{meta}</p>
            </div>
            <StateDot
              loaded={false}
              cached={variant.cached}
              label={variant.cached ? t("models.state.cached") : t("models.state.notCached")}
            />
          </li>
        );
      })}
    </ul>
  );
}

export function ProvenanceNote({ source }: { source: string | null }) {
  const { t } = useTranslation();

  return (
    <div className="border-t border-line pt-3 text-[10.5px] leading-relaxed text-tertiary">
      <p>{t("models.provenance.live")}</p>
      <p className="mt-1">
        {source
          ? t("models.provenance.curated", { source })
          : t("models.provenance.noOverlay")}
      </p>
    </div>
  );
}
