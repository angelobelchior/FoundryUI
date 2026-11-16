import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertIcon, PlusIcon } from "../../components/Icon";
import { Modal } from "../../components/Modal";
import { Spinner } from "../../components/Spinner";
import { useToast } from "../../components/Toast";
import {
  localModels,
  registerModel,
  toFoundryCommandError,
  unregisterModel,
  type FoundryCommandError,
  type ModelSummary,
} from "../../lib/foundry";
import { ConfirmDialog } from "./ConfirmDialog";
import { errorLabel, formatSize } from "./format";

type LocalModelsPanelProps = {
  open: boolean;
  onClose: () => void;
  onChanged: () => void;
};

const tasks = [
  "chat-completion",
  "coding",
  "vision-language-chat",
  "automatic-speech-recognition",
  "embeddings",
] as const;

const devices = ["", "cpu", "gpu", "npu"] as const;

type FormState = {
  path: string;
  modelId: string;
  task: (typeof tasks)[number];
  displayName: string;
  device: (typeof devices)[number];
  executionProvider: string;
  contextLength: string;
  supportsToolCalling: boolean;
  supportsReasoning: boolean;
};

const emptyForm: FormState = {
  path: "",
  modelId: "",
  task: "chat-completion",
  displayName: "",
  device: "",
  executionProvider: "",
  contextLength: "",
  supportsToolCalling: false,
  supportsReasoning: false,
};

export function LocalModelsPanel({ open, onClose, onChanged }: LocalModelsPanelProps) {
  const { t, i18n } = useTranslation();
  const toast = useToast();
  const [models, setModels] = useState<ModelSummary[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<FoundryCommandError | null>(null);
  const [busy, setBusy] = useState(false);
  const [form, setForm] = useState<FormState>(emptyForm);
  const [unregisterTarget, setUnregisterTarget] = useState<ModelSummary | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);

    try {
      setModels(await localModels());
    } catch (next) {
      setError(toFoundryCommandError(next));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!open) {
      return;
    }

    setError(null);
    setUnregisterTarget(null);
    void refresh();
  }, [open, refresh]);

  function update<K extends keyof FormState>(key: K, value: FormState[K]) {
    setForm((current) => ({ ...current, [key]: value }));
  }

  async function submit() {
    setBusy(true);
    setError(null);

    const length = Number.parseInt(form.contextLength, 10);

    try {
      const next = await registerModel({
        path: form.path.trim(),
        modelId: form.modelId.trim(),
        task: form.task,
        displayName: form.displayName.trim() || null,
        device: form.device || null,
        executionProvider: form.executionProvider.trim() || null,
        contextLength: Number.isFinite(length) && length > 0 ? length : null,
        supportsToolCalling: form.supportsToolCalling,
        supportsReasoning: form.supportsReasoning,
      });

      setModels(next);
      setForm(emptyForm);
      toast.push(
        t("models.local.registered", { alias: form.modelId.trim() || form.path.trim() }),
      );
      onChanged();
    } catch (next) {
      setError(toFoundryCommandError(next));
    } finally {
      setBusy(false);
    }
  }

  async function unregister() {
    if (!unregisterTarget) {
      return;
    }

    setBusy(true);
    setError(null);

    try {
      setModels(await unregisterModel(unregisterTarget.alias));
      setUnregisterTarget(null);
      toast.push(t("models.local.unregistered"));
      onChanged();
    } catch (next) {
      setError(toFoundryCommandError(next));
    } finally {
      setBusy(false);
    }
  }

  const complete = form.path.trim() !== "" && form.modelId.trim() !== "";

  return (
    <>
      <Modal open={open} title={t("models.local.title")} onClose={onClose} size="lg">
        {error ? (
          <div className="mb-3 flex items-start gap-2 rounded-[8px] border border-line bg-subtle px-3 py-2 text-[11.5px] text-secondary">
            <AlertIcon size={14} className="mt-0.5 shrink-0 text-danger" />
            <p className="min-w-0 flex-1 break-words">{errorLabel(error, t)}</p>
          </div>
        ) : null}

        <section>
          <p className="text-[11px] font-semibold uppercase tracking-[0.055em] text-tertiary">
            {t("models.local.title")}
          </p>

          {models && models.length > 0 ? (
            <ul className="mt-2 overflow-hidden rounded-[8px] border border-line">
              {models.map((model) => (
                <li
                  key={model.id}
                  className="flex items-center gap-3 border-b border-line bg-canvas px-3 py-2 last:border-b-0"
                >
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-[12px] font-medium text-primary">{model.alias}</p>
                    <p className="mt-0.5 truncate font-mono text-[10px] text-tertiary">{model.id}</p>
                  </div>
                  <span className="shrink-0 text-[10.5px] tabular-nums text-tertiary">
                    {model.fileSizeMb === null
                      ? null
                      : formatSize(model.fileSizeMb, i18n.language)}
                  </span>
                  <button
                    type="button"
                    onClick={() => setUnregisterTarget(model)}
                    disabled={busy}
                    className="h-6 shrink-0 rounded-[6px] border border-line px-2 text-[11px] font-medium text-secondary transition-colors hover:bg-hover hover:text-primary disabled:opacity-50"
                  >
                    {t("models.local.unregister")}
                  </button>
                </li>
              ))}
            </ul>
          ) : null}

          {models && models.length === 0 && !loading ? (
            <div className="mt-2 flex flex-col items-center rounded-[8px] bg-subtle px-3 py-7 text-center">
              <p className="text-[12px] font-medium text-secondary">{t("models.local.emptyTitle")}</p>
              <p className="mt-1 max-w-[42ch] text-[11px] leading-relaxed text-tertiary">
                {t("models.local.emptyBody")}
              </p>
            </div>
          ) : null}

          {loading && !models ? (
            <div className="flex flex-col items-center justify-center gap-2.5 py-7 text-center text-[11.5px] text-tertiary">
              <Spinner size={14} />
              {t("models.loadingDetails")}
            </div>
          ) : null}
        </section>

        <section className="mt-5 border-t border-line pt-4">
          <div className="flex items-center gap-1.5">
            <PlusIcon size={14} className="text-tertiary" />
            <p className="text-[11px] font-semibold uppercase tracking-[0.055em] text-tertiary">
              {t("models.local.registerTitle")}
            </p>
          </div>

          <div className="mt-2.5 grid grid-cols-1 gap-2.5 sm:grid-cols-2">
            <Field label={t("models.local.path")} className="sm:col-span-2">
              <input
                type="text"
                value={form.path}
                onChange={(event) => update("path", event.target.value)}
                placeholder={t("models.local.pathPlaceholder")}
                className={inputClass}
              />
            </Field>
            <Field label={t("models.local.modelId")}>
              <input
                type="text"
                value={form.modelId}
                onChange={(event) => update("modelId", event.target.value)}
                placeholder={t("models.local.modelIdPlaceholder")}
                className={`${inputClass} font-mono`}
              />
            </Field>
            <Field label={t("models.local.task")}>
              <select
                value={form.task}
                onChange={(event) => update("task", event.target.value as FormState["task"])}
                className={inputClass}
              >
                {tasks.map((task) => (
                  <option key={task} value={task}>
                    {t(`models.local.tasks.${task}`)}
                  </option>
                ))}
              </select>
            </Field>
            <Field label={t("models.local.displayName")}>
              <input
                type="text"
                value={form.displayName}
                onChange={(event) => update("displayName", event.target.value)}
                placeholder={t("models.local.displayNamePlaceholder")}
                className={inputClass}
              />
            </Field>
            <Field label={t("models.local.device")}>
              <select
                value={form.device}
                onChange={(event) => update("device", event.target.value as FormState["device"])}
                className={inputClass}
              >
                {devices.map((device) => (
                  <option key={device} value={device}>
                    {device ? device.toUpperCase() : t("models.local.devicePlaceholder")}
                  </option>
                ))}
              </select>
            </Field>
            <Field label={t("models.local.executionProvider")}>
              <input
                type="text"
                value={form.executionProvider}
                onChange={(event) => update("executionProvider", event.target.value)}
                placeholder={t("models.local.executionProviderPlaceholder")}
                className={inputClass}
              />
            </Field>
            <Field label={t("models.local.contextLength")}>
              <input
                type="number"
                min={1}
                value={form.contextLength}
                onChange={(event) => update("contextLength", event.target.value)}
                placeholder={t("models.local.contextLengthPlaceholder")}
                className={inputClass}
              />
            </Field>
          </div>

          <div className="mt-2.5 flex flex-wrap items-center gap-4">
            <label className="inline-flex items-center gap-1.5 text-[11.5px] text-secondary">
              <input
                type="checkbox"
                checked={form.supportsToolCalling}
                onChange={(event) => update("supportsToolCalling", event.target.checked)}
                className="h-3.5 w-3.5 accent-[var(--accent)]"
              />
              {t("models.local.supportsToolCalling")}
            </label>
            <label className="inline-flex items-center gap-1.5 text-[11.5px] text-secondary">
              <input
                type="checkbox"
                checked={form.supportsReasoning}
                onChange={(event) => update("supportsReasoning", event.target.checked)}
                className="h-3.5 w-3.5 accent-[var(--accent)]"
              />
              {t("models.local.supportsReasoning")}
            </label>
          </div>

          <div className="mt-3 flex items-center justify-between gap-3">
            {!complete ? (
              <p className="text-[11px] text-tertiary">{t("models.local.required")}</p>
            ) : (
              <span />
            )}
            <button
              type="button"
              onClick={() => void submit()}
              disabled={!complete || busy}
              className="inline-flex h-7 shrink-0 items-center gap-1.5 rounded-[7px] bg-accent px-3 text-[11.5px] font-medium text-accent-contrast transition-colors hover:bg-accent-hover disabled:opacity-50"
            >
              {busy ? <Spinner size={11} /> : null}
              {busy ? t("models.local.registering") : t("models.local.register")}
            </button>
          </div>
        </section>
      </Modal>

      <ConfirmDialog
        open={unregisterTarget !== null}
        title={t("models.local.unregisterTitle", { alias: unregisterTarget?.alias ?? "" })}
        body={t("models.local.unregisterBody")}
        confirmLabel={t("models.local.unregister")}
        tone="danger"
        busy={busy}
        onConfirm={() => void unregister()}
        onClose={() => setUnregisterTarget(null)}
      />
    </>
  );
}

const inputClass =
  "h-7 w-full rounded-[6px] border border-line bg-elevated px-2 text-[11.5px] text-primary outline-none placeholder:text-tertiary focus:border-accent";

function Field({
  label,
  className,
  children,
}: {
  label: string;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <label className={`flex flex-col gap-1 ${className ?? ""}`}>
      <span className="text-[10.5px] font-medium text-tertiary">{label}</span>
      {children}
    </label>
  );
}
