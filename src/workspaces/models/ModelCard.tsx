import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Modal } from "../../components/Modal";
import type { CatalogModel, Declared } from "../../lib/foundry";
import { KindBadge, OriginMark } from "./ModelBadges";
import { ModelActions, type ModelActionsProps } from "./ModelActions";
import {
  CapabilitiesList,
  ModelStats,
  ProvenanceNote,
  SectionTitle,
  Stat,
  VariantList,
} from "./ModelParts";
import { describeModel, formatNumber, limitationsFor } from "./format";

type ModelCardProps = Pick<
  ModelActionsProps,
  "job" | "busy" | "error" | "onDownload" | "onCancelDownload" | "onLoad" | "onUnload" | "onRemove" | "onRetry"
> & {
  model: CatalogModel;
  open: boolean;
  onClose: () => void;
};

export function ModelCard({
  model,
  open,
  job,
  busy,
  error,
  onClose,
  onDownload,
  onCancelDownload,
  onLoad,
  onUnload,
  onRemove,
  onRetry,
}: ModelCardProps) {
  const { t, i18n } = useTranslation();

  return (
    <Modal open={open} title={model.alias} onClose={onClose} size="lg">
      <div className="flex items-center gap-2">
        <KindBadge kind={model.kind} />
        <p className="truncate font-mono text-[10.5px] text-tertiary">{model.id}</p>
      </div>
      <p className="mt-2 text-[13px] leading-relaxed text-secondary">
        {describeModel(model, t, i18n.language)}
      </p>

      <ModelStats
        model={model}
        job={job}
        fourth={
          <Stat label={t("models.fields.family")}>
            {model.family ?? (
              <span className="font-normal text-tertiary">{t("models.notDeclared")}</span>
            )}
          </Stat>
        }
      />

      <ModelActions
        model={model}
        job={job}
        busy={busy}
        error={error}
        onDownload={onDownload}
        onCancelDownload={onCancelDownload}
        onLoad={onLoad}
        onUnload={onUnload}
        onRemove={onRemove}
        onRetry={onRetry}
      />

      <CapabilitiesList model={model} />

      <section className="mt-6">
        <SectionTitle>{t("models.sections.specifications")}</SectionTitle>
        <dl className="mt-3 divide-y divide-line overflow-hidden rounded-[8px] border border-line">
          <DeclaredRow
            label={t("models.fields.context")}
            declared={model.contextLength}
            render={(value) =>
              t("models.tokens", { value: formatNumber(value, i18n.language) })
            }
          />
          {model.kind === "embedding" || model.embeddingDimensions.value !== null ? (
            <DeclaredRow
              label={t("models.fields.dimensions")}
              declared={model.embeddingDimensions}
              render={(value) => formatNumber(value, i18n.language)}
            />
          ) : null}
          <DeclaredRow
            label={t("models.fields.quantization")}
            declared={model.quantization}
            render={(value) => value}
          />
          <PlainRow label={t("models.fields.license")} value={model.license} />
          <PlainRow label={t("models.fields.publisher")} value={model.publisher} />
        </dl>
      </section>

      <section className="mt-6">
        <SectionTitle>{t("models.sections.variants")}</SectionTitle>
        <div className="mt-3">
          <VariantList variants={model.variants} />
        </div>
      </section>

      <section className="mt-6">
        <SectionTitle>{t("models.sections.limitations")}</SectionTitle>
        <ul className="mt-2.5 flex flex-col gap-1.5">
          {limitationsFor(model, t).map((limitation) => (
            <li key={limitation} className="flex items-start gap-2 text-[12.5px] text-secondary">
              <span className="mt-[7px] h-1 w-1 shrink-0 rounded-full bg-tertiary" />
              {limitation}
            </li>
          ))}
        </ul>
      </section>

      <div className="mt-6">
        <ProvenanceNote source={model.overlaySource} />
      </div>
    </Modal>
  );
}

function DeclaredRow<T>({
  label,
  declared,
  render,
}: {
  label: string;
  declared: Declared<T>;
  render: (value: T) => ReactNode;
}) {
  const { t } = useTranslation();

  return (
    <div className="flex items-baseline justify-between gap-6 px-3 py-2 text-[12.5px]">
      <dt className="shrink-0 text-tertiary">{label}</dt>
      <dd className="min-w-0 text-right text-secondary">
        {declared.value === null ? (
          <span className="text-tertiary">{t("models.notDeclared")}</span>
        ) : (
          render(declared.value)
        )}
        <OriginMark declared={declared} />
      </dd>
    </div>
  );
}

function PlainRow({ label, value }: { label: string; value: string | null }) {
  const { t } = useTranslation();

  return (
    <div className="flex items-baseline justify-between gap-6 px-3 py-2 text-[12.5px]">
      <dt className="shrink-0 text-tertiary">{label}</dt>
      <dd className="min-w-0 break-words text-right text-secondary">
        {value ?? <span className="text-tertiary">{t("models.notDeclared")}</span>}
      </dd>
    </div>
  );
}
