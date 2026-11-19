import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Modal } from "../../components/Modal";
import type { ProjectFolderSelection } from "../../lib/projects";
import { projectDialogFocus } from "./projectFocus";

type ProjectCreateDialogProps = {
  selection: ProjectFolderSelection;
  busy: boolean;
  error: string | null;
  expired: boolean;
  onCancel: () => void;
  onConfirm: (name: string) => Promise<void>;
};

export function ProjectCreateDialog({ selection, busy, error, expired, onCancel, onConfirm }: ProjectCreateDialogProps) {
  const { t } = useTranslation();
  const [name, setName] = useState(selection.defaultName);
  const nameInput = useRef<HTMLInputElement>(null);
  const confirmButton = useRef<HTMLButtonElement>(null);
  const existing = selection.existingProject;

  useEffect(() => {
    if (existing) confirmButton.current?.focus();
    else nameInput.current?.focus();
  }, [existing]);

  return (
    <Modal
      open
      title={t(existing ? "projects.existingTitle" : "projects.new")}
      onClose={() => { if (!busy) onCancel(); }}
      className={projectDialogFocus}
    >
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (!busy && !expired && (existing || name.trim())) void onConfirm(existing?.name ?? name.trim());
        }}
      >
        <p className="mb-3 text-[13px] leading-relaxed text-primary">
          {existing ? t("projects.existingHint", { name: existing.name }) : t("projects.createHint")}
        </p>
        <span
          aria-label={t("projects.folder")}
          title={selection.folderPath}
          className="mb-4 block break-words text-[12px] leading-relaxed text-primary"
        >
          {selection.folderPath}
        </span>
        {error ? <p role="alert" className="mb-3 text-[13px] leading-relaxed text-danger">{error}</p> : null}
        {!existing ? (
          <>
            <label htmlFor="project-create-name" className="mb-1 block text-[12px] font-medium text-primary">
              {t("projects.name")}
            </label>
            <input
              id="project-create-name"
              ref={nameInput}
              value={name}
              onChange={(event) => setName(event.target.value)}
              readOnly={busy}
              className="h-8 w-full rounded-[7px] border border-line bg-canvas px-2 text-[13px] text-primary"
            />
          </>
        ) : null}
        <div className="mt-4 flex flex-wrap justify-end gap-2">
          <button
            type="button"
            disabled={busy}
            onClick={onCancel}
            className="inline-flex h-8 items-center justify-center rounded-md border border-line px-3 text-[11.5px] font-medium text-primary transition-colors hover:bg-hover disabled:opacity-60"
          >
            {t("common.cancel")}
          </button>
          <button
            type="submit"
            ref={confirmButton}
            disabled={busy || expired || (!existing && !name.trim())}
            className="inline-flex h-8 items-center justify-center rounded-md bg-accent px-3 text-[11.5px] font-medium text-accent-contrast transition-colors hover:bg-accent-hover disabled:opacity-50"
          >
            {busy ? t("common.loading") : t(existing ? "projects.openExisting" : "projects.createConfirm")}
          </button>
        </div>
      </form>
    </Modal>
  );
}
