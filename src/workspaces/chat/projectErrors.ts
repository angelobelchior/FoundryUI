import type { TFunction } from "i18next";
import type { ProjectCommandErrorCode } from "../../lib/projects";

const codes: ProjectCommandErrorCode[] = [
  "ProjectInvalidInput", "ProjectNotFound", "ProjectConflict", "ProjectStaleContext",
  "ProjectBusy", "ProjectStorage", "ProjectFolderUnavailable", "ProjectInvalidSelection",
  "ProjectDialogUnavailable", "ProjectNotReady",
];

export function projectErrorLabel(error: unknown, t: TFunction): string {
  const code = typeof error === "object" && error !== null && "code" in error ? error.code : null;
  const known = codes.find((candidate) => candidate === code) ?? "ProjectStorage";
  return t(`projects.errors.${known}`);
}
