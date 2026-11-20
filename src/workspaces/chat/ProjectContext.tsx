import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { PencilIcon, TrashIcon } from "../../components/Icon";
import { IconButton } from "../../components/IconButton";
import { Modal } from "../../components/Modal";
import type { ChatProject } from "../../lib/projects";
import { cx } from "../../lib/cx";
import { projectControlFocus, projectDialogFocus } from "./projectFocus";

type ProjectContextProps = {
  project: ChatProject;
  disabled: boolean;
  busy: boolean;
  error: string | null;
  onRename: (name: string) => Promise<boolean>;
  onRemove: () => void;
  onRelocate: () => void;
};

export function ProjectContext({
  project, disabled, busy, error, onRename, onRemove, onRelocate,
}: ProjectContextProps) {
  const { t } = useTranslation();
  const [renameOpen, setRenameOpen] = useState(false);
  const [name, setName] = useState(project.name);
  const nameInput = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (renameOpen) {
      nameInput.current?.focus();
    }
  }, [renameOpen]);

  async function submitRename() {
    if (!name.trim() || busy || disabled) {
      return;
    }
    if (await onRename(name.trim())) {
      setRenameOpen(false);
    }
  }

  return (
    <>
      <div className="flex min-w-0 shrink-0 items-center gap-3 border-b border-line px-5 py-2 text-[12px] text-primary">
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-center gap-3">
            <span className="truncate font-medium" title={project.name}>{project.name}</span>
          </div>
          <span aria-label={t("projects.folder")} title={project.folderPath} className="mt-0.5 block truncate">
            {project.folderPath}
          </span>
          {project.availability !== "available" ? (
            <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1">
              <span role="status">{t(`projects.availability.${project.availability}`)}</span>
              <button
                type="button"
                disabled={disabled || busy}
                onClick={onRelocate}
                className={cx("font-medium text-primary hover:underline disabled:opacity-50", projectControlFocus)}
              >
                {t("projects.locate")}
              </button>
              <span className="w-full text-primary">{t("projects.unavailableHint")}</span>
            </div>
          ) : null}
        </div>
        <IconButton
          label={t("projects.rename")}
          disabled={disabled || busy}
          className={projectControlFocus}
          onClick={() => { setName(project.name); setRenameOpen(true); }}
        >
          <PencilIcon size={14} />
        </IconButton>
        <IconButton label={t("projects.remove")} disabled={disabled || busy} onClick={onRemove} className={projectControlFocus}>
          <TrashIcon size={14} />
        </IconButton>
      </div>
      <Modal open={renameOpen} title={t("projects.rename")} onClose={() => { if (!busy) setRenameOpen(false); }} className={projectDialogFocus}>
        <form onSubmit={(event) => { event.preventDefault(); void submitRename(); }}>
          <p className="mb-4 text-[13px] leading-relaxed text-primary">{t("projects.renameHint")}</p>
          {error ? (
            <p role="alert" className="mb-3 text-[13px] leading-relaxed text-danger">{error}</p>
          ) : null}
          <label htmlFor="project-name" className="mb-1 block text-[12px] font-medium text-primary">
            {t("projects.name")}
          </label>
          <input
            id="project-name"
            ref={nameInput}
            value={name}
            onChange={(event) => setName(event.target.value)}
            readOnly={busy}
            className="h-8 w-full rounded-[7px] border border-line bg-canvas px-2 text-[13px] text-primary"
          />
          <div className="mt-4 flex flex-wrap justify-end gap-2">
            <button
              type="button"
              disabled={busy}
              onClick={() => setRenameOpen(false)}
              className="inline-flex h-8 items-center justify-center rounded-md border border-line px-3 text-[11.5px] font-medium text-primary transition-colors hover:bg-hover disabled:opacity-60"
            >
              {t("common.cancel")}
            </button>
            <button
              type="submit"
              disabled={busy || disabled || !name.trim()}
              className="inline-flex h-8 items-center justify-center rounded-md bg-accent px-3 text-[11.5px] font-medium text-accent-contrast transition-colors hover:bg-accent-hover disabled:opacity-50"
            >
              {busy ? t("chat.renaming") : t("chat.renameSave")}
            </button>
          </div>
        </form>
      </Modal>
    </>
  );
}
