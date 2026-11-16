import { createContext, useCallback, useContext, useState, type ReactNode } from "react";
import { cx } from "../lib/cx";

type ToastTone = "neutral" | "danger";

type ToastItem = {
  id: number;
  message: string;
  tone: ToastTone;
};

type ToastApi = {
  push: (message: string, tone?: ToastTone) => void;
};

const ToastContext = createContext<ToastApi | null>(null);
const toastDuration = 4000;

export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);

  const dismiss = useCallback((id: number) => {
    setItems((current) => current.filter((item) => item.id !== id));
  }, []);

  const push = useCallback(
    (message: string, tone: ToastTone = "neutral") => {
      const id = Date.now() + Math.random();
      setItems((current) => [...current, { id, message, tone }]);
      window.setTimeout(() => dismiss(id), toastDuration);
    },
    [dismiss],
  );

  return (
    <ToastContext.Provider value={{ push }}>
      {children}
      <div
        role="status"
        aria-live="polite"
        className="pointer-events-none fixed inset-x-0 bottom-5 z-40 flex flex-col items-center gap-2"
      >
        {items.map((item) => (
          <button
            key={item.id}
            type="button"
            onClick={() => dismiss(item.id)}
            className={cx(
              "pop-in pointer-events-auto max-w-[420px] rounded-lg border border-line bg-elevated px-3 py-2 text-left text-[12px] shadow-[var(--shadow-popover)]",
              item.tone === "danger" ? "text-danger" : "text-primary",
            )}
          >
            {item.message}
          </button>
        ))}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast() {
  const api = useContext(ToastContext);

  if (!api) {
    throw new Error("useToast requires a ToastProvider");
  }

  return api;
}
