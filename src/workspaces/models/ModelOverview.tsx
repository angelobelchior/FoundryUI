import { useTranslation } from "react-i18next";
import { InfoIcon } from "../../components/Icon";
import { ParamsSection } from "../../components/ParamsSection";
import { Spinner } from "../../components/Spinner";
import type { CatalogModel, LoadedSummary } from "../../lib/foundry";
import { ModelActions, type ModelActionsProps } from "./ModelActions";
import { CapabilitiesList, ModelStats, SectionTitle, Stat, VariantList } from "./ModelParts";
import { describeModel, formatBytes, formatNumber } from "./format";

type ModelOverviewProps = Pick<
  ModelActionsProps,
  "job" | "busy" | "error" | "onDownload" | "onCancelDownload" | "onLoad" | "onUnload" | "onRemove" | "onRetry"
> & {
  model: CatalogModel;
  machine: LoadedSummary | null;
  loading: boolean;
  onOpenCard: () => void;
};

type Translate = (key: string, options?: Record<string, unknown>) => string;

export function ModelOverview({
  model,
  machine,
  loading,
  job,
  busy,
  error,
  onOpenCard,
  onDownload,
  onCancelDownload,
  onLoad,
  onUnload,
  onRemove,
  onRetry,
}: ModelOverviewProps) {
  const { t, i18n } = useTranslation();
  const context = model.contextLength.value;
  const dimensions = model.embeddingDimensions.value;
  const isEmbedding = model.kind === "embedding";
  const fourthLabel = isEmbedding ? t("models.fields.dimensions") : t("models.fields.context");
  const translate: Translate = (key, options) =>
    String(t(key as never, options as never));
  const compatibility = describeCompatibility(model, machine, i18n.language, translate);

  return (
    <div className="mx-auto w-full max-w-[760px] px-4 py-5 sm:px-6 sm:py-6">
      <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between sm:gap-6">
        <p className="max-w-[64ch] text-[13px] leading-relaxed text-secondary">
          {describeModel(model, t, i18n.language)}
        </p>
        {loading ? (
          <span className="inline-flex shrink-0 items-center gap-1.5 text-[10.5px] text-tertiary sm:pt-0.5">
            <Spinner size={11} />
            {t("models.loadingDetails")}
          </span>
        ) : null}
      </div>

      <ModelStats
        model={model}
        job={job}
        fourth={
          <Stat label={fourthLabel}>
            {isEmbedding ? (
              dimensions === null ? (
                <span className="font-normal text-tertiary">{t("models.notDeclared")}</span>
              ) : (
                formatNumber(dimensions, i18n.language)
              )
            ) : context === null ? (
              <span className="font-normal text-tertiary">{t("models.notDeclared")}</span>
            ) : (
              t("models.tokens", { value: formatNumber(context, i18n.language) })
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

      <section className="mt-6">
        <SectionTitle>{t("models.compatibility.title")}</SectionTitle>
        <div className="mt-3 rounded-[9px] border border-line bg-subtle px-3 py-2.5">
          <p className="text-[12px] font-medium text-primary">{compatibility.label}</p>
          {compatibility.help ? (
            <p className="mt-1 text-[11.5px] leading-relaxed text-tertiary">{compatibility.help}</p>
          ) : null}
        </div>
      </section>

      <CapabilitiesList model={model} />

      {isEmbedding || model.kind === "speech" ? null : (
        <section className="mt-6">
          <SectionTitle>{t("models.sections.parameters")}</SectionTitle>
          <div className="mt-3 max-w-[520px]">
            <ParamsSection
              scope="model"
              modelAlias={model.alias}
              help={t("models.parametersHelp")}
            />
          </div>
        </section>
      )}

      <section className="mt-6">
        <SectionTitle>{t("models.sections.variants")}</SectionTitle>
        <div className="mt-3">
          <VariantList variants={model.variants} />
        </div>
      </section>

      <div className="mt-6 flex flex-col gap-3 border-t border-line pt-3 sm:flex-row sm:items-center sm:justify-between sm:gap-4">
        <p className="max-w-[48ch] text-[10.5px] leading-relaxed text-tertiary">
          {t("models.provenance.short")}
        </p>
        <button
          type="button"
          onClick={onOpenCard}
          className="inline-flex h-7 shrink-0 items-center gap-1.5 rounded-[7px] border border-line px-2.5 text-[11.5px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary"
        >
          <InfoIcon size={13} />
          {t("models.openCard")}
        </button>
      </div>
    </div>
  );
}

function describeCompatibility(
  model: CatalogModel,
  machine: LoadedSummary | null,
  locale: string,
  t: Translate,
) {
  if (model.loaded) {
    return {
      label: t("models.compatibility.loaded"),
      help: t("models.compatibility.loadedHelp"),
    };
  }

  if (!machine) {
    return {
      label: t("models.compatibility.checking"),
      help: null,
    };
  }

  if (model.fileSizeMb === null) {
    return {
      label: t("models.compatibility.unknown"),
      help: t("models.compatibility.unknownHelp"),
    };
  }

  const modelBytes = Math.round(model.fileSizeMb * 1024 * 1024);
  const requiredBytes = Math.round(modelBytes * 1.2);
  const availableBytes = machine.availableMemoryBytes;
  const totalBytes = machine.totalMemoryBytes;

  if (availableBytes >= requiredBytes) {
    return {
      label: t("models.compatibility.ready"),
      help: t("models.compatibility.readyHelp", {
        required: formatBytes(requiredBytes, locale),
        available: formatBytes(availableBytes, locale),
      }),
    };
  }

  if (totalBytes >= requiredBytes) {
    return {
      label: t("models.compatibility.maybe"),
      help: t("models.compatibility.maybeHelp", {
        required: formatBytes(requiredBytes, locale),
        available: formatBytes(availableBytes, locale),
      }),
    };
  }

  return {
    label: t("models.compatibility.no"),
    help: t("models.compatibility.noHelp", {
      required: formatBytes(requiredBytes, locale),
      total: formatBytes(totalBytes, locale),
    }),
  };
}
