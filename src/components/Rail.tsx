import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "../lib/cx";
import type { PrimaryWorkspace, Workspace } from "../lib/workspaces";
import {
  ChatIcon,
  EmbeddingsIcon,
  KeyboardIcon,
  ModelsIcon,
  SettingsIcon,
  SpeechIcon,
  type IconComponent,
} from "./Icon";


type RailProps = {
  children?: ReactNode;
  active: Workspace;
  onSelect: (workspace: Workspace) => void;
  onShortcuts: () => void;
  highlightWorkspacesMap?: boolean;
  highlightItem?: Workspace | null;
};

function railItemClass(isActive: boolean) {
  return cx(
    "flex min-h-[var(--control-size)] w-full items-center gap-2.5 rounded-[var(--item-radius)] border px-2.5 py-2 text-left text-[13px] font-medium transition-colors",
    isActive
      ? "border-line-strong bg-active text-primary shadow-[var(--shadow-thumb)]"
      : "border-transparent text-secondary hover:border-line hover:bg-hover hover:text-primary active:bg-active",
  );
}

export function Rail({
  active,
  onSelect,
  onShortcuts,
  children,
  highlightWorkspacesMap = false,
  highlightItem = null,
}: RailProps) {
  const { t } = useTranslation();

  const primary: Array<{ id: PrimaryWorkspace; icon: IconComponent; label: string }> = [
    { id: "chat", icon: ChatIcon, label: t("rail.chat") },
    { id: "speech", icon: SpeechIcon, label: t("rail.speech") },
    { id: "embeddings", icon: EmbeddingsIcon, label: t("rail.embeddings") },
  ];

  return (
    <aside className="relative flex h-full min-h-0 min-w-0 flex-col border-r border-line bg-window">
      {highlightWorkspacesMap ? (
        <div
          id="onboarding-spotlight-anchor"
          aria-hidden
          className="onboarding-spotlight pointer-events-none absolute inset-2 z-10 rounded-[12px] border-2 border-accent shadow-[0_0_0_2px_rgba(255,255,255,0.75),0_0_24px_rgba(59,130,246,0.55)]"
        />
      ) : null}
      <div className="h-[var(--titlebar-inset)] shrink-0" data-tauri-drag-region />
      <nav aria-label={t("rail.label")} className="shrink-0 border-b border-line px-3 pb-4">
        <ul className="flex flex-col gap-1">
          {primary.map((item) => (
            <li key={item.id}>
              <button
                type="button"
                onClick={() => onSelect(item.id)}
                aria-current={active === item.id ? "page" : undefined}
                title={item.label}
                id={highlightItem === item.id ? "onboarding-spotlight-anchor" : undefined}
                className={cx(
                  railItemClass(active === item.id),
                  highlightItem === item.id
                    ? "onboarding-spotlight border-accent shadow-[0_0_0_2px_rgba(255,255,255,0.7),0_0_20px_rgba(59,130,246,0.5)]"
                    : null,
                )}
              >
                <item.icon size={19} />
                <span>{item.label}</span>
              </button>
            </li>
          ))}
        </ul>
      </nav>
      {children}
      <div className="mt-auto flex shrink-0 flex-col gap-1 border-t border-line p-3">
        <button
          type="button"
          onClick={() => onSelect("models")}
          aria-current={active === "models" ? "page" : undefined}
          title={t("rail.models")}
          id={highlightItem === "models" ? "onboarding-spotlight-anchor" : undefined}
          className={cx(
            railItemClass(active === "models"),
            highlightItem === "models"
              ? "onboarding-spotlight border-accent shadow-[0_0_0_2px_rgba(255,255,255,0.7),0_0_20px_rgba(59,130,246,0.5)]"
              : null,
          )}
        >
          <ModelsIcon size={19} /><span>{t("rail.models")}</span>
        </button>
        <button
          type="button"
          onClick={() => onSelect("settings")}
          aria-current={active === "settings" ? "page" : undefined}
          title={t("rail.settings")}
          id={highlightItem === "settings" ? "onboarding-spotlight-anchor" : undefined}
          className={cx(
            railItemClass(active === "settings"),
            highlightItem === "settings"
              ? "onboarding-spotlight border-accent shadow-[0_0_0_2px_rgba(255,255,255,0.7),0_0_20px_rgba(59,130,246,0.5)]"
              : null,
          )}
        >
          <SettingsIcon size={19} /><span>{t("rail.settings")}</span>
        </button>
        <button type="button" onClick={onShortcuts} title={t("rail.shortcuts")} className={railItemClass(false)}>
          <KeyboardIcon size={19} /><span>{t("rail.shortcuts")}</span>
        </button>
      </div>
    </aside>
  );
}
