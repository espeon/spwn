import { useState, useEffect, type Dispatch, type SetStateAction } from "react";

export function useBrowserState<T>(
  key: string,
  defaultValue: T,
): [T, Dispatch<SetStateAction<T>>] {
  const storageKey = `spwn-state-${key}`;

  const [state, setState] = useState<T>(() => {
    if (typeof window === "undefined") {
      return defaultValue;
    }

    try {
      const item = window.localStorage.getItem(storageKey);
      if (item === null) {
        return defaultValue;
      }
      return JSON.parse(item) as T;
    } catch (error) {
      console.warn(`Error reading localStorage key "${storageKey}":`, error);
      return defaultValue;
    }
  });

  useEffect(() => {
    try {
      if (state === undefined) {
        window.localStorage.removeItem(storageKey);
      } else {
        window.localStorage.setItem(storageKey, JSON.stringify(state));
      }
    } catch (error) {
      console.warn(`Error setting localStorage key "${storageKey}":`, error);
    }
  }, [state, storageKey]);

  return [state, setState];
}
