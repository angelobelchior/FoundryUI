import { useEffect, useRef, useState } from "react";
import { CheckIcon, ChevronDownIcon } from "../../components/Icon";
import { cx } from "../../lib/cx";

export type ChatModelOption = {
  value: string;
  label: string;
  hint?: string;
};

type ModelMenuProps = {
  label: string;
  value: string;
  options: readonly ChatModelOption[];
  disabled?: boolean;
  onChange: (value: string) => void;
};

export function ModelMenu({ label, value, options, disabled = false, onChange }: ModelMenuProps) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const selected = options.find((option) => option.value === value);

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

  return (
    <div ref={rootRef} className="relative">
      <button
        ref={triggerRef}
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={label}
        disabled={disabled}
        onClick={() => setOpen((current) => !current)}
        className={cx(
          "inline-flex h-7 max-w-[220px] items-center gap-1.5 rounded-[7px] border border-line px-2 text-[11.5px] transition-colors",
          "text-secondary hover:border-line-strong hover:text-primary",
          "disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:border-line disabled:hover:text-secondary",
        )}
      >
        <span className="truncate font-medium">{selected?.label ?? value}</span>
        <ChevronDownIcon
          size={12}
          className={cx("shrink-0 text-tertiary transition-transform duration-150", open && "rotate-180")}
        />
      </button>
      {open ? (
        <div
          role="menu"
          aria-label={label}
          className="pop-in absolute right-0 top-[calc(100%+4px)] z-30 max-h-[280px] min-w-[200px] overflow-y-auto rounded-lg border border-line bg-elevated p-1 shadow-[var(--shadow-popover)]"
        >
          {options.map((option) => {
            const isSelected = option.value === value;

            return (
              <button
                key={option.value}
                type="button"
                role="menuitemradio"
                aria-checked={isSelected}
                onClick={() => {
                  onChange(option.value);
                  setOpen(false);
                  triggerRef.current?.focus();
                }}
                className={cx(
                  "flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-[12px] transition-colors",
                  isSelected
                    ? "bg-subtle text-primary"
                    : "text-secondary hover:bg-hover hover:text-primary",
                )}
              >
                <span className="flex w-4 shrink-0 justify-center">
                  {isSelected ? <CheckIcon size={13} /> : null}
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block truncate">{option.label}</span>
                  {option.hint ? (
                    <span className="block truncate text-[10px] text-tertiary">{option.hint}</span>
                  ) : null}
                </span>
              </button>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}
