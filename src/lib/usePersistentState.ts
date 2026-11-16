import { useCallback, useState } from "react";
import { readStored, writeStored } from "./storage";

export function usePersistentState<T extends string>(key: string, fallback: T, allowed: readonly T[]) {
  const [value, setValue] = useState<T>(() => {
    const stored = readStored(key);

    return stored && allowed.includes(stored as T) ? (stored as T) : fallback;
  });

  const update = useCallback(
    (next: T) => {
      setValue(next);
      writeStored(key, next);
    },
    [key],
  );

  return [value, update] as const;
}
