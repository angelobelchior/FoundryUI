import { useTranslation } from "react-i18next";
import { FolderIcon } from "../../components/Icon";
import { ListEmpty } from "../../components/EmptyState";
import { Spinner } from "../../components/Spinner";
import type { ChatProject } from "../../lib/projects";
import { cx } from "../../lib/cx";
import { projectControlFocus } from "./projectFocus";

type ProjectNavigationProps = {
  projectsView: boolean;
  onChange: (value: boolean) => void;
};

export function ProjectNavigation({ projectsView, onChange }: ProjectNavigationProps) {
  const { t } = useTranslation();
  return (
    <div className="flex items-center gap-1" role="group" aria-label={t("projects.organization")}>
      {[false, true].map((value) => (
        <button
          key={String(value)}
          type="button"
          aria-pressed={projectsView === value}
          onClick={() => onChange(value)}
          className={cx(
            "min-w-0 flex-1 rounded-[var(--item-radius)] px-2 py-1.5 text-[12px] font-medium transition-colors",
            projectsView === value ? "bg-active text-primary" : "text-primary hover:bg-hover",
            projectControlFocus,
          )}
        >
          {t(value ? "projects.title" : "chat.listTitle")}
        </button>
      ))}
    </div>
  );
}

type ProjectsPanelProps = {
  projects: ChatProject[];
  selectedId: number | null;
  loading: boolean;
  error: string | null;
  onSelect: (project: ChatProject) => void;
  onRetry: () => void;
};

export function ProjectsPanel({ projects, selectedId, loading, error, onSelect, onRetry }: ProjectsPanelProps) {
  const { t } = useTranslation();
  if (loading) {
    return <div className="flex justify-center py-6"><Spinner size={14} className="text-tertiary" /></div>;
  }
  if (error) {
    return (
      <div className="space-y-3 px-2 py-4">
        <p role="alert" className="text-[12px] leading-relaxed text-danger">{error}</p>
        <button
          type="button"
          onClick={onRetry}
          className={cx("rounded-[7px] border border-line px-3 py-1.5 text-[12px] text-primary hover:bg-hover", projectControlFocus)}
        >
          {t("projects.retry")}
        </button>
      </div>
    );
  }
  if (!projects.length) {
    return (
      <div className="[&_p]:text-primary">
        <ListEmpty icon={FolderIcon} title={t("projects.emptyTitle")} body={t("projects.emptyBody")} />
      </div>
    );
  }
  return (
    <ul className="flex flex-col gap-1 pb-2">
      {projects.map((project) => (
        <li key={project.id}>
          <button
            type="button"
            onClick={() => onSelect(project)}
            aria-current={selectedId === project.id ? "true" : undefined}
            className={cx(
              "flex w-full min-w-0 flex-col gap-1 rounded-[var(--item-radius)] border px-[var(--item-pad-x)] py-[var(--item-pad-y)] text-left transition-colors",
              selectedId === project.id
                ? "border-line-strong bg-elevated shadow-[var(--shadow-thumb)]"
                : "border-transparent hover:border-line hover:bg-hover",
              projectControlFocus,
            )}
          >
            <span className="flex min-w-0 items-center gap-1.5">
              <FolderIcon size={13} className="shrink-0 text-secondary" />
              <span className="truncate text-[12.5px] font-medium text-primary" title={project.name}>
                {project.name}
              </span>
            </span>
            <span className="w-full truncate text-[10.5px] text-primary" title={project.folderPath}>
              {project.folderPath}
            </span>
            <span className="text-[10.5px] text-primary">
              {t(`projects.availability.${project.availability}`)}
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
}
