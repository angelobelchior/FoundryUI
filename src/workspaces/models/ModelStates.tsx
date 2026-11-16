import { useTranslation } from "react-i18next";
import { AlertIcon, RefreshIcon } from "../../components/Icon";
import { Spinner } from "../../components/Spinner";

export function CatalogSkeleton() {
  return (
    <div className="flex flex-col gap-1 px-2 pt-2" aria-hidden="true">
      {Array.from({ length: 8 }, (_, index) => (
        <div key={index} className="flex flex-col gap-1.5 px-1 py-1.5">
          <span className="h-3 w-[62%] animate-pulse rounded bg-subtle" />
          <span className="h-2 w-[40%] animate-pulse rounded bg-subtle" />
        </div>
      ))}
    </div>
  );
}

export function CatalogLoading() {
  const { t } = useTranslation();

  return (
    <div className="flex h-full flex-col items-center justify-center px-6 py-10 text-center sm:px-8 sm:py-12">
      <Spinner size={22} className="text-tertiary" />
      <h3 className="mt-3.5 text-[15px] font-semibold tracking-[-0.01em] text-primary">
        {t("models.loading.title")}
      </h3>
      <p className="mt-1.5 max-w-[46ch] text-[12.5px] leading-relaxed text-secondary">
        {t("models.loading.body")}
      </p>
    </div>
  );
}

export function CatalogError({
  message,
  detail,
  onRetry,
}: {
  message: string;
  detail: string | null;
  onRetry: () => void;
}) {
  const { t } = useTranslation();

  return (
    <div className="flex h-full flex-col items-center justify-center px-6 py-10 text-center sm:px-8 sm:py-12">
      <AlertIcon size={30} className="text-danger" />
      <h3 className="mt-3.5 text-[15px] font-semibold tracking-[-0.01em] text-primary">
        {t("models.errorTitle")}
      </h3>
      <p className="mt-1.5 max-w-[46ch] text-[12.5px] leading-relaxed text-secondary">{message}</p>
      {detail ? <p className="mt-1 break-words font-mono text-[10.5px] text-tertiary">{detail}</p> : null}
      <button
        type="button"
        onClick={onRetry}
        className="mt-4 inline-flex h-7 w-full items-center justify-center gap-1.5 rounded-[7px] border border-line bg-elevated px-3 text-[11.5px] font-medium text-primary shadow-[var(--shadow-raised)] transition-colors hover:bg-hover sm:w-auto"
      >
        <RefreshIcon size={13} />
        {t("models.retry")}
      </button>
    </div>
  );
}

export function DetailError({
  message,
  detail,
  onRetry,
}: {
  message: string;
  detail: string | null;
  onRetry: () => void;
}) {
  const { t } = useTranslation();

  return (
    <div className="flex items-start gap-2.5 rounded-[8px] border border-line bg-subtle px-3 py-2 text-[11.5px] text-secondary">
      <AlertIcon size={14} className="mt-0.5 shrink-0 text-danger" />
      <div className="min-w-0 flex-1">
        <p>{message}</p>
        {detail ? <p className="mt-0.5 break-words font-mono text-[10px] text-tertiary">{detail}</p> : null}
      </div>
      <button
        type="button"
        onClick={onRetry}
        className="h-6 shrink-0 rounded-[6px] border border-line bg-elevated px-2 text-[11px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary"
      >
        {t("models.retry")}
      </button>
    </div>
  );
}
