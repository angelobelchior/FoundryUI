import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { ArchiveIcon, StarIcon, TrashIcon } from "../../components/Icon";
import { ContextMenu, type ContextMenuItem } from "../../components/ContextMenu";
import { cx } from "../../lib/cx";
import type { Conversation } from "../../lib/chat";

type ConversationListProps = {
  conversations: Conversation[];
  selectedId: number | null;
  streamingConversationId: number | null;
  disabled: boolean;
  archivedView: boolean;
  onSelect: (id: number) => void;
  onToggleFavorite: (conversation: Conversation) => void;
  onToggleArchive: (conversation: Conversation) => void;
  onRequestDelete: (conversation: Conversation) => void;
};

type MenuState = {
  conversation: Conversation;
  x: number;
  y: number;
};

export function ConversationList({
  conversations,
  selectedId,
  streamingConversationId,
  disabled,
  archivedView,
  onSelect,
  onToggleFavorite,
  onToggleArchive,
  onRequestDelete,
}: ConversationListProps) {
  const { t, i18n } = useTranslation();
  const [menu, setMenu] = useState<MenuState | null>(null);
  const dateFormat = new Intl.DateTimeFormat(i18n.language, {
    month: "short",
    day: "numeric",
  });
  const closeMenu = useCallback(() => setMenu(null), []);

  function openMenu(conversation: Conversation, x: number, y: number) {
    if (disabled) {
      return;
    }

    setMenu({ conversation, x, y });
  }

  const menuItems: ContextMenuItem[] = menu
    ? [
        {
          label: menu.conversation.favorite ? t("chat.unfavorite") : t("chat.favorite"),
          icon: StarIcon,
          onSelect: () => onToggleFavorite(menu.conversation),
        },
        {
          label: archivedView ? t("chat.unarchive") : t("chat.archive"),
          icon: ArchiveIcon,
          onSelect: () => onToggleArchive(menu.conversation),
        },
        {
          label: t("chat.deleteForever"),
          icon: TrashIcon,
          tone: "danger",
          onSelect: () => onRequestDelete(menu.conversation),
        },
      ]
    : [];

  return (
    <>
      <ul className="flex flex-col gap-1 pb-2">
        {conversations.map((conversation) => {
          const selected = conversation.id === selectedId;
          const streaming = conversation.id === streamingConversationId;

          return (
            <li
              key={conversation.id}
              onContextMenu={(event) => {
                if (disabled) {
                  return;
                }

                event.preventDefault();
                openMenu(conversation, event.clientX, event.clientY);
              }}
            >
              <button
                type="button"
                onClick={() => onSelect(conversation.id)}
                onKeyDown={(event) => {
                  if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
                    event.preventDefault();

                    const rect = event.currentTarget.getBoundingClientRect();

                    openMenu(conversation, rect.right - 12, rect.top + 26);
                  }
                }}
                aria-current={selected ? "true" : undefined}
                className={cx(
                  "flex w-full flex-col gap-1 rounded-[var(--item-radius)] border px-[var(--item-pad-x)] py-[var(--item-pad-y)] text-left transition-colors",
                  selected
                    ? "border-line-strong bg-elevated shadow-[var(--shadow-thumb)]"
                    : "border-transparent hover:border-line hover:bg-hover",
                )}
              >
                <span className="flex w-full min-w-0 items-center gap-1.5">
                  {conversation.favorite ? (
                    <StarIcon size={11} className="shrink-0 fill-current text-accent" />
                  ) : null}
                  <span className="min-w-0 flex-1 truncate text-[12.5px] font-medium text-primary">
                    {conversation.title || t("chat.untitled")}
                  </span>
                  {streaming ? (
                    <span
                      aria-label={t("chat.status.streaming")}
                      className="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-accent"
                    />
                  ) : null}
                </span>
                <span className="mt-0.5 flex w-full min-w-0 items-center justify-between gap-2">
                  <span className="truncate text-[10.5px] text-secondary">
                    {conversation.modelAlias}
                  </span>
                  <span className="shrink-0 text-[10.5px] tabular-nums text-tertiary">
                    {dateFormat.format(conversation.updatedAt)}
                  </span>
                </span>
              </button>
            </li>
          );
        })}
      </ul>
      {menu ? (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          label={t("chat.conversationActions")}
          items={menuItems}
          onClose={closeMenu}
        />
      ) : null}
    </>
  );
}
