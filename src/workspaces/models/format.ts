import type { TFunction } from "i18next";
import {
  isFoundryErrorCode,
  type CatalogModel,
  type FoundryCommandError,
  type ModelJob,
  type ModelKind,
} from "../../lib/foundry";

const knownModalities = ["text", "image", "audio", "video", "embedding"] as const;

export function formatSize(fileSizeMb: number, locale: string): string {
  const gigabytes = fileSizeMb >= 1024;
  const formatter = new Intl.NumberFormat(locale, {
    maximumFractionDigits: gigabytes ? 1 : 0,
    minimumFractionDigits: gigabytes ? 1 : 0,
  });
  const value = gigabytes ? fileSizeMb / 1024 : fileSizeMb;

  return `${formatter.format(value)} ${gigabytes ? "GB" : "MB"}`;
}

export function formatNumber(value: number, locale: string): string {
  return new Intl.NumberFormat(locale).format(value);
}

export function errorLabel(error: FoundryCommandError, t: TFunction): string {
  return isFoundryErrorCode(error.code) ? t(`errors.${error.code}`) : error.message;
}

export function formatBytes(bytes: number, locale: string): string {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return formatSize(0, locale);
  }

  return formatSize(bytes / (1024 * 1024), locale);
}

export function formatSpeed(bytesPerSecond: number, locale: string): string | null {
  if (!Number.isFinite(bytesPerSecond) || bytesPerSecond <= 0) {
    return null;
  }

  return `${formatBytes(bytesPerSecond, locale)}/s`;
}

export function formatDuration(seconds: number, t: TFunction): string {
  const total = Math.max(0, Math.round(seconds));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const rest = total % 60;

  if (hours > 0) {
    return t("models.time.hoursMinutes", { hours, minutes });
  }

  if (minutes > 0) {
    return t("models.time.minutesSeconds", { minutes, seconds: rest });
  }

  return t("models.time.seconds", { seconds: rest });
}

export function formatElapsed(milliseconds: number, locale: string): string {
  const seconds = Math.max(0, Math.round(milliseconds / 1000));

  if (seconds < 45) {
    return new Intl.RelativeTimeFormat(locale, { numeric: "auto" }).format(0, "second");
  }

  if (seconds < 3600) {
    return new Intl.RelativeTimeFormat(locale, { numeric: "auto" }).format(
      -Math.round(seconds / 60),
      "minute",
    );
  }

  return new Intl.RelativeTimeFormat(locale, { numeric: "auto" }).format(
    -Math.round(seconds / 3600),
    "hour",
  );
}

export function jobPercent(job: ModelJob): number {
  if (job.status === "succeeded") {
    return 100;
  }

  if (job.totalBytes <= 0) {
    return 0;
  }

  return Math.min(99, Math.max(0, Math.round((job.receivedBytes / job.totalBytes) * 100)));
}

export function sortModels(models: CatalogModel[], locale: string): CatalogModel[] {
  const collator = new Intl.Collator(locale, { numeric: true, sensitivity: "base" });
  const familyOf = (model: CatalogModel) => model.family ?? "\uFFFF";

  return [...models].sort((left, right) => {
    const family = collator.compare(familyOf(left), familyOf(right));

    return family === 0 ? collator.compare(left.alias, right.alias) : family;
  });
}

export function deviceLabel(device: string | null): string | null {
  return device ? device.toUpperCase() : null;
}

export function executionProviderLabel(provider: string | null): string | null {
  return provider ? provider.replace(/ExecutionProvider$/, "") : null;
}

export function kindLabel(kind: ModelKind, t: TFunction): string {
  return t(`models.kinds.${kind}`);
}

export function stateLabel(model: CatalogModel, t: TFunction, job?: ModelJob): string {
  if (job?.status === "running") {
    return t("models.jobs.downloading");
  }

  if (job?.status === "failed") {
    return t("models.jobs.failed");
  }

  if (job?.status === "cancelled") {
    return t("models.jobs.cancelled");
  }

  if (model.loaded) {
    return t("models.state.loaded");
  }

  return model.cached ? t("models.state.cached") : t("models.state.notCached");
}

export function modalityLabel(modality: string, t: TFunction): string {
  return isKnownModality(modality) ? t(`models.modalities.${modality}`) : modality;
}

export function describeModel(model: CatalogModel, t: TFunction, locale: string): string {
  const publisher = model.publisher ?? t("models.notDeclared");
  const inputs = model.inputModalities.value;
  const outputs = model.outputModalities.value;

  if (!inputs?.length || !outputs?.length) {
    return t("models.descriptionIncomplete", { publisher });
  }

  const list = new Intl.ListFormat(locale, { style: "long", type: "conjunction" });

  return t("models.description", {
    publisher,
    inputs: list.format(inputs.map((modality) => modalityLabel(modality, t))),
    outputs: list.format(outputs.map((modality) => modalityLabel(modality, t))),
  });
}

export function limitationsFor(model: CatalogModel, t: TFunction): string[] {
  const inputs = model.inputModalities.value;
  const outputs = model.outputModalities.value;

  if (!inputs?.length) {
    return [t("models.limitations.unknown")];
  }

  const items: string[] = [];

  if (!inputs.includes("image") && !inputs.includes("video")) {
    items.push(t("models.limitations.noImage"));
  }

  if (!inputs.includes("audio")) {
    items.push(t("models.limitations.noAudio"));
  }

  if (!model.supportsToolCalling) {
    items.push(t("models.limitations.noTools"));
  }

  if (model.kind === "embedding") {
    items.push(t("models.limitations.noChat"));
  } else if (model.kind === "speech") {
    items.push(t("models.limitations.noTextInput"));
    items.push(t("models.limitations.noSpeechOutput"));
  } else if (!outputs?.length) {
    items.push(t("models.limitations.outputUnknown"));
  } else if (!outputs.includes("audio")) {
    items.push(t("models.limitations.noSpeechOutput"));
  }

  return items;
}

function isKnownModality(value: string): value is (typeof knownModalities)[number] {
  return (knownModalities as readonly string[]).includes(value);
}
