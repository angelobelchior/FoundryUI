import { useTranslation } from "react-i18next";
import { Modal } from "./Modal";

type ShortcutsModalProps = {
  open: boolean;
  onClose: () => void;
};

export function ShortcutsModal({ open, onClose }: ShortcutsModalProps) {
  const { t } = useTranslation();

  const rows = [
    { keys: ["⌘", "1"], label: t("shortcuts.goTo", { workspace: t("rail.chat") }) },
    { keys: ["⌘", "2"], label: t("shortcuts.goTo", { workspace: t("rail.models") }) },
    { keys: ["⌘", "3"], label: t("shortcuts.goTo", { workspace: t("rail.speech") }) },
    { keys: ["⌘", "4"], label: t("shortcuts.goTo", { workspace: t("rail.embeddings") }) },
    { keys: ["⌘", "5"], label: t("shortcuts.openSettings") },
    { keys: ["⌘", "⇧", "M"], label: t("shortcuts.dictation") },
    { keys: ["⌘", "F"], label: t("shortcuts.search") },
    { keys: ["⌘", "/"], label: t("shortcuts.showHelp") },
    { keys: ["Esc"], label: t("shortcuts.closeDialog") },
  ];

  return (
    <Modal open={open} title={t("shortcuts.title")} onClose={onClose}>
      <dl className="flex flex-col gap-2.5">
        {rows.map((row) => (
          <div key={row.label} className="flex items-center justify-between gap-6">
            <dt className="text-[12.5px] text-secondary">{row.label}</dt>
            <dd className="flex shrink-0 items-center gap-1">
              {row.keys.map((key) => (
                <kbd
                  key={key}
                  className="inline-flex h-5 min-w-5 items-center justify-center rounded border border-line bg-subtle px-1.5 font-mono text-[10.5px] text-secondary"
                >
                  {key}
                </kbd>
              ))}
            </dd>
          </div>
        ))}
      </dl>
    </Modal>
  );
}
