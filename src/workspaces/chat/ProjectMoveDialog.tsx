import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Modal } from "../../components/Modal";
import type { ChatProject } from "../../lib/projects";
import { projectDialogFocus } from "./projectFocus";

type ProjectMoveDialogProps = {
  projects: ChatProject[];
  projectId: number | null;
  busy: boolean;
  error: string | null;
  onClose: () => void;
  onMove: (destination: number | null) => Promise<void>;
};

export function ProjectMoveDialog({
  projects, projectId, busy, error, onClose, onMove,
}: ProjectMoveDialogProps) {
  const { t } = useTranslation();
  const options = [
    { id: "none", name: t("chat.listTitle") },
    ...projects.map((project) => ({
      id: String(project.id),
      name: `${project.name} · ${project.folderPath}`,
    })),
  ].filter((option) => option.id !== (projectId === null ? "none" : String(projectId)));
  const [destination, setDestination] = useState(options[0]?.id ?? "none");
  const destinationInput = useRef<HTMLSelectElement>(null);
  useEffect(() => { destinationInput.current?.focus(); }, []);

  return (
    <Modal open title={t("projects.move")} onClose={() => { if (!busy) onClose(); }} className={projectDialogFocus}>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (!busy && options.length) {
            void onMove(destination === "none" ? null : Number(destination));
          }
        }}
      >
        <p className="mb-4 text-[13px] leading-relaxed text-primary">{t("projects.moveHint")}</p>
        {error ? (
          <p role="alert" className="mb-3 text-[13px] leading-relaxed text-danger">{error}</p>
        ) : null}
        <label htmlFor="project-destination" className="mb-1 block text-[12px] font-medium text-primary">
          {t("projects.destination")}
        </label>
        <select
          id="project-destination"
          ref={destinationInput}
          value={destination}
          onChange={(event) => setDestination(event.target.value)}
          disabled={busy || !options.length}
          className="h-8 w-full min-w-0 rounded-[7px] border border-line bg-canvas px-2 text-[13px] text-primary"
        >
          {options.map((option) => (
            <option key={option.id} value={option.id}>{option.name}</option>
          ))}
        </select>
        <div className="mt-4 flex flex-wrap justify-end gap-2">
          <button
            type="button"
            disabled={busy}
            onClick={onClose}
            className="inline-flex h-8 items-center justify-center rounded-md border border-line px-3 text-[11.5px] font-medium text-primary transition-colors hover:bg-hover disabled:opacity-60"
          >
            {t("common.cancel")}
          </button>
          <button
            type="submit"
            disabled={busy || !options.length}
            className="inline-flex h-8 items-center justify-center rounded-md bg-accent px-3 text-[11.5px] font-medium text-accent-contrast transition-colors hover:bg-accent-hover disabled:opacity-50"
          >
            {busy ? t("common.loading") : t("projects.move")}
          </button>
        </div>
      </form>
    </Modal>
  );
}
