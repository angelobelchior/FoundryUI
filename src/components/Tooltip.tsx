import { useCallback, useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

type TooltipSide = "right" | "bottom";

type TooltipProps = {
  label: string;
  title?: string;
  side?: TooltipSide;
  children: ReactNode;
  showDelayMs?: number;
};

const SHOW_DELAY_MS = 420;
const VIEWPORT_MARGIN = 8;
const SIDE_GAP = 8;
const BOTTOM_GAP = 6;

type Position = {
  top: number;
  left: number;
};

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), Math.max(min, max));
}

export function Tooltip({
  label,
  title,
  side = "right",
  children,
  showDelayMs = SHOW_DELAY_MS,
}: TooltipProps) {
  const anchorRef = useRef<HTMLSpanElement>(null);
  const bubbleRef = useRef<HTMLDivElement>(null);
  const timerRef = useRef<number | null>(null);
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState<Position | null>(null);

  const clearTimer = useCallback(() => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  const hide = useCallback(() => {
    clearTimer();
    setOpen(false);
    setPosition(null);
  }, [clearTimer]);

  const show = useCallback(() => {
    if (!label) {
      return;
    }

    clearTimer();
    timerRef.current = window.setTimeout(() => {
      timerRef.current = null;
      setOpen(true);
    }, showDelayMs);
  }, [clearTimer, label, showDelayMs]);

  const place = useCallback(() => {
    const anchor = anchorRef.current;
    const bubble = bubbleRef.current;

    if (!anchor || !bubble) {
      return;
    }

    const rect = anchor.getBoundingClientRect();
    const size = bubble.getBoundingClientRect();

    let top: number;
    let left: number;

    if (side === "right") {
      top = rect.top + rect.height / 2 - size.height / 2;
      left = rect.right + SIDE_GAP;

      if (left + size.width > window.innerWidth - VIEWPORT_MARGIN) {
        left = rect.left - size.width - SIDE_GAP;
      }
    } else {
      top = rect.bottom + BOTTOM_GAP;
      left = rect.left + rect.width / 2 - size.width / 2;

      if (top + size.height > window.innerHeight - VIEWPORT_MARGIN) {
        top = rect.top - size.height - BOTTOM_GAP;
      }
    }

    setPosition({
      top: clamp(top, VIEWPORT_MARGIN, window.innerHeight - size.height - VIEWPORT_MARGIN),
      left: clamp(left, VIEWPORT_MARGIN, window.innerWidth - size.width - VIEWPORT_MARGIN),
    });
  }, [side]);

  useEffect(() => () => clearTimer(), [clearTimer]);

  useLayoutEffect(() => {
    if (open) {
      place();
    }
  }, [open, place]);

  useEffect(() => {
    if (!open) {
      return;
    }

    const onMove = () => place();

    window.addEventListener("scroll", onMove, true);
    window.addEventListener("resize", onMove);

    return () => {
      window.removeEventListener("scroll", onMove, true);
      window.removeEventListener("resize", onMove);
    };
  }, [open, place]);

  return (
    <>
      <span
        ref={anchorRef}
        className="inline-flex"
        onMouseEnter={show}
        onMouseLeave={hide}
        onFocus={show}
        onBlur={hide}
      >
        {children}
      </span>
      {open
        ? createPortal(
            <div
              ref={bubbleRef}
              role="tooltip"
              className={title ? "tip-bubble tip-bubble-rich" : "tip-bubble"}
              style={
                position
                  ? { top: position.top, left: position.left }
                  : { top: 0, left: 0, visibility: "hidden" }
              }
            >
              {title ? (
                <div className="flex flex-col gap-1">
                  <span className="text-[10px] font-semibold uppercase tracking-[0.05em] text-tertiary">
                    {title}
                  </span>
                  <span className="text-[11px] leading-relaxed text-secondary">{label}</span>
                </div>
              ) : (
                label
              )}
            </div>,
            document.body,
          )
        : null}
    </>
  );
}
