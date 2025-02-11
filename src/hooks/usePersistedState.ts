import { useState, useEffect } from "react";

export function usePersistedState<T>(
  key: string,
  defaultValue: T,
): [T, (value: T) => void] {
  // Initialize state with defaultValue
  const [state, setState] = useState<T>(() => {
    // Check if we're in the browser
    if (typeof window === "undefined") {
      return defaultValue;
    }

    try {
      const item = window.localStorage.getItem(key);
      return item ? JSON.parse(item) : defaultValue;
    } catch (error) {
      console.warn(`Error reading localStorage key "${key}":`, error);
      return defaultValue;
    }
  });

  // Update localStorage when state changes
  useEffect(() => {
    try {
      window.localStorage.setItem(key, JSON.stringify(state));
    } catch (error) {
      console.warn(`Error setting localStorage key "${key}":`, error);
    }
  }, [key, state]);

  return [state, setState];
}
