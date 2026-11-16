import { useEffect, useId, useRef, useState } from "react";
import { CheckIcon, ChevronDownIcon } from "./Icon";
import { cx } from "../lib/cx";

export type FilterOption = {
  value: string;
  label: string;
  count?: number;
};

type FilterMenuProps = {
  label: string;
  value: string;
  options: readonly FilterOption[];
  onChange: (value: string) => void;
};

export function FilterMenu({ label, value, options, onChange }: FilterMenuProps) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const optionRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const pendingFocusIndexRef = useRef<number | null>(null);
  const menuId = useId();
  const selected = options.find((option) => option.value === value) ?? options[0];
  const active = options[0] ? value !== options[0].value : false;
  const showValue = active && selected !== undefined;

  optionRefs.current.length = options.length;

  const selectedIndex = Math.max(
    0,
    options.findIndex((option) => option.value === value),
  );

  function openMenu(index: number) {
    pendingFocusIndexRef.current = index;
    setOpen(true);
  }

  function moveFocusToOption(index: number) {
    if (options.length === 0) {
      return;
    }

    const size = options.length;
    const next = ((index % size) + size) % size;

    optionRefs.current[next]?.focus();
  }

  useEffect(() => {
    if (!open) {
      return;
    }

    function onPointerDown(event: PointerEvent) {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) {
        setOpen(false);
      }
    }

    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        setOpen(false);
        triggerRef.current?.focus();
      }
    }

    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);

    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);

  useEffect(() => {
    if (!open) {
      return;
    }

    const index = pendingFocusIndexRef.current;

    if (index === null) {
      return;
    }

    pendingFocusIndexRef.current = null;
    moveFocusToOption(index);
  }, [open]);

  return (
    <div ref={rootRef} className={cx("relative min-w-[152px] flex-1", open && "z-40")}>
      <button
        ref={triggerRef}
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        onClick={() => setOpen((current) => !current)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown") {
            event.preventDefault();
            openMenu(selectedIndex);
            return;
          }

          if (event.key === "ArrowUp") {
            event.preventDefault();
            openMenu(selectedIndex > 0 ? selectedIndex : options.length - 1);
          }
        }}
        className={cx(
          "flex h-[var(--control-size-sm)] w-full min-w-0 items-center justify-between gap-1.5 rounded-[var(--item-radius)] border px-2 text-[11.5px] transition-colors",
          active
            ? "border-line-strong bg-subtle text-primary"
            : "border-line text-secondary hover:border-line-strong hover:text-primary",
        )}
      >
        <span className="min-w-0 flex items-center gap-1.5 overflow-hidden">
          <span className="shrink-0 text-tertiary">{label}</span>
          {showValue ? <span className="truncate font-medium">{selected.label}</span> : null}
        </span>
        <ChevronDownIcon
          size={12}
          className={cx("shrink-0 text-tertiary transition-transform duration-150", open && "rotate-180")}
        />
      </button>
      {open ? (
        <div
          id={menuId}
          role="menu"
          aria-label={label}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault();
              setOpen(false);
              triggerRef.current?.focus();
              return;
            }

            if (event.key === "Tab") {
              setOpen(false);
              return;
            }

            if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
              return;
            }

            event.preventDefault();

            const activeElement = document.activeElement as HTMLButtonElement | null;
            const index = optionRefs.current.findIndex((node) => node === activeElement);
            const current = index >= 0 ? index : selectedIndex;

            if (event.key === "Home") {
              moveFocusToOption(0);
              return;
            }

            if (event.key === "End") {
              moveFocusToOption(options.length - 1);
              return;
            }

            moveFocusToOption(current + (event.key === "ArrowDown" ? 1 : -1));
          }}
          className="pop-in absolute left-0 top-[calc(100%+4px)] z-[90] max-h-[280px] w-full min-w-0 overflow-y-auto rounded-lg border border-line bg-elevated p-1 shadow-[var(--shadow-popover)]"
        >
          {options.map((option, index) => {
            const isSelected = option.value === value;

            return (
              <button
                key={option.value}
                type="button"
                role="menuitemradio"
                aria-checked={isSelected}
                ref={(node) => {
                  optionRefs.current[index] = node;
                }}
                onClick={() => {
                  onChange(option.value);
                  setOpen(false);
                  triggerRef.current?.focus();
                }}
                className={cx(
                  "flex h-[var(--control-size-sm)] w-full items-center gap-2 rounded-[var(--item-radius)] px-2 text-left text-[12px] transition-colors",
                  isSelected
                    ? "bg-subtle text-primary"
                    : "text-secondary hover:bg-hover hover:text-primary",
                )}
              >
                <span className="flex w-4 shrink-0 justify-center">
                  {isSelected ? <CheckIcon size={13} /> : null}
                </span>
                <span className="flex-1 truncate">{option.label}</span>
                {option.count === undefined ? null : (
                  <span className="text-[10.5px] tabular-nums text-tertiary">{option.count}</span>
                )}
              </button>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}
