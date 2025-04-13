"use client";

import React, {
  createContext,
  useContext,
  useState,
  useEffect,
  useCallback,
  ReactNode,
  SetStateAction,
} from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { usePersistedState } from "@/hooks/usePersistedState";
import { VisualizationPreferences } from "@/types/weather";
import { DEFAULT_PREFERENCES } from "@/config/preferences";
import {
  parseVisualizationPreferences,
  serializeVisualizationPreferences,
} from "@/utils/params";

interface PreferencesContextType {
  preferences: VisualizationPreferences;
  setPreference: <K extends keyof VisualizationPreferences>(
    key: K,
    value: VisualizationPreferences[K],
  ) => void;
  setPreferences: (prefs: Partial<VisualizationPreferences>) => void;
}

const PreferencesContext = createContext<PreferencesContextType | undefined>(
  undefined,
);

interface PreferencesProviderProps {
  children: ReactNode;
}

// Helper to get search params as an object
const getSearchParamsObject = (
  searchParams: URLSearchParams,
): Record<string, string> => {
  const obj: Record<string, string> = {};
  searchParams.forEach((value, key) => {
    obj[key] = value;
  });
  return obj;
};

export const PreferencesProvider: React.FC<PreferencesProviderProps> = ({
  children,
}) => {
  const router = useRouter();
  const searchParams = useSearchParams();

  // Initial state from URL, falling back to defaults
  const initialPreferencesFromUrl = parseVisualizationPreferences(
    getSearchParamsObject(searchParams),
  );

  // Use persisted state, initializing with URL/default values
  const [preferences, setStoredPreferences] =
    usePersistedState<VisualizationPreferences>(
      "meteogram-preferences", // Single key for the whole object
      initialPreferencesFromUrl,
    );

  // State to track if initial sync with URL is done
  const [isSyncedWithUrl, setIsSyncedWithUrl] = useState(false);

  // Sync URL params -> persisted state (on initial load/direct navigation)
  useEffect(() => {
    const prefsFromUrl = parseVisualizationPreferences(
      getSearchParamsObject(searchParams),
    );
    // Only update if persisted state differs AND we haven't synced yet
    // This prevents overwriting user's stored prefs if they navigate without params
    if (
      !isSyncedWithUrl &&
      JSON.stringify(prefsFromUrl) !== JSON.stringify(preferences)
    ) {
      console.log("Syncing URL to State:", prefsFromUrl);
      setStoredPreferences(prefsFromUrl);
    }
    setIsSyncedWithUrl(true); // Mark sync as done
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [searchParams]); // Only run when URL params change

  // Sync persisted state -> URL params (when state changes)
  useEffect(() => {
    // Only update URL if the state is synced and actually changed
    if (isSyncedWithUrl) {
      const currentPath = window.location.pathname; // Simple way to get current path
      const newParams = serializeVisualizationPreferences(preferences);
      const queryString = newParams.toString();
      const newUrl = `${currentPath}${queryString ? "?" + queryString : ""}`;

      // Check if URL needs updating to avoid unnecessary history entries
      if (window.location.search !== (queryString ? "?" + queryString : "")) {
        console.log("Syncing State to URL:", newUrl);
        router.replace(newUrl, { scroll: false });
      }
    }
  }, [preferences, router, isSyncedWithUrl]);

  const updatePreferences = useCallback(
    (newPrefs: Partial<VisualizationPreferences>) => {
      const updater: SetStateAction<VisualizationPreferences> = (prev) => ({
        ...prev,
        ...newPrefs,
      });
      setStoredPreferences(updater);
    },
    [setStoredPreferences],
  );

  const setPreference = useCallback(
    <K extends keyof VisualizationPreferences>(
      key: K,
      value: VisualizationPreferences[K],
    ) => {
      const updater: SetStateAction<VisualizationPreferences> = (prev) => ({
        ...prev,
        [key]: value,
      });
      setStoredPreferences(updater);
    },
    [setStoredPreferences],
  );

  return (
    <PreferencesContext.Provider
      value={{ preferences, setPreference, setPreferences: updatePreferences }}
    >
      {children}
    </PreferencesContext.Provider>
  );
};

export const usePreferences = (): PreferencesContextType => {
  const context = useContext(PreferencesContext);
  if (context === undefined) {
    throw new Error("usePreferences must be used within a PreferencesProvider");
  }
  return context;
};
