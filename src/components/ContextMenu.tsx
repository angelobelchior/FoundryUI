import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { IconComponent } from "./Icon";
import { cx } from "../lib/cx";

export type ContextMenuItem = {
  label: string;
  icon?: IconComponent;
  tone?: "default" | "danger";
  disabled?: boolean;
  onSelect: () => void;
};

type ContextMenuProps = {
  x: number;
  y: number;
  label: string;
  items: ContextMenuItem[];
  onClose: () => void;
};

export function ContextMenu({ x, y, label, items, onClose }: ContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  const itemRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const [position, setPosition] = useState({ left: x, top: y });

  itemRefs.current.length = items.length;

  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  useLayoutEffect(() => {
    const node = menuRef.current;

    if (!node) {
      return;
    }

    const margin = 8;
    const rect = node.getBoundingClientRect();

    setPosition({
      left: Math.max(margin, Math.min(x, window.innerWidth - rect.width - margin)),
      top: Math.max(margin, Math.min(y, window.innerHeight - rect.height - margin)),
    });
  }, [x, y]);

  useEffect(() => {
    const previouslyFocused = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const enabled = itemRefs.current.filter((node) => node && !node.disabled);
    const first = enabled[0];

    if (first) {
      first.focus();
    } else {
      menuRef.current?.focus();
    }

    function onPointerDown(event: PointerEvent) {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        onCloseRef.current();
      }
    }

    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        onCloseRef.current();
        return;
      }

      if (event.key === "Tab") {
        onCloseRef.current();
        return;
      }

      if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
        return;
      }

      const enabledItems = itemRefs.current.filter((node) => node && !node.disabled);

      if (enabledItems.length === 0) {
        return;
      }

      event.preventDefault();

      if (event.key === "Home") {
        enabledItems[0]?.focus();
        return;
      }

      if (event.key === "End") {
        enabledItems[enabledItems.length - 1]?.focus();
        return;
      }

      const active = document.activeElement as HTMLButtonElement | null;
      const index = enabledItems.findIndex((node) => node === active);
      const current = index >= 0 ? index : 0;
      const offset = event.key === "ArrowDown" ? 1 : -1;
      const next = (current + offset + enabledItems.length) % enabledItems.length;

      enabledItems[next]?.focus();
    }

    function dismiss() {
      onCloseRef.current();
    }

    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onKeyDown, true);
    document.addEventListener("scroll", dismiss, true);
    window.addEventListener("blur", dismiss);
    window.addEventListener("resize", dismiss);

    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("keydown", onKeyDown, true);
      document.removeEventListener("scroll", dismiss, true);
      window.removeEventListener("blur", dismiss);
      window.removeEventListener("resize", dismiss);

      if (previouslyFocused && document.contains(previouslyFocused)) {
        previouslyFocused.focus();
      }
    };
  }, []);

  return createPortal(
    <div
      ref={menuRef}
      role="menu"
      aria-label={label}
      tabIndex={-1}
      style={position}
      className="pop-in fixed z-[80] min-w-[190px] rounded-lg border border-line bg-elevated p-1 shadow-[var(--shadow-popover)] outline-none"
    >
      {items.map((item, index) => {
        const Icon = item.icon;

        return (
          <button
            key={item.label}
            type="button"
            role="menuitem"
            aria-disabled={item.disabled ? "true" : undefined}
            disabled={item.disabled}
            ref={(node) => {
              itemRefs.current[index] = node;
            }}
            onClick={() => {
              item.onSelect();
              onClose();
            }}
            className={cx(
              "flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-[12px] transition-colors disabled:cursor-not-allowed disabled:opacity-40",
              item.tone === "danger"
                ? "text-danger hover:bg-danger/10"
                : "text-secondary hover:bg-hover hover:text-primary",
            )}
          >
            <span className="flex w-4 shrink-0 justify-center">
              {Icon ? <Icon size={14} /> : null}
            </span>
            <span className="flex-1 truncate">{item.label}</span>
          </button>
        );
      })}
    </div>,
    document.body,
  );
}
