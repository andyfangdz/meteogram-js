"use client";

import React, {
  createContext,
  useContext,
  useState,
  useEffect,
  useCallback,
  ReactNode,
  useRef,
} from "react";
import { useRouter } from "next/navigation";
import Cookies from "js-cookie";
import { VisualizationPreferences } from "@/types/weather";
import { DEFAULT_PREFERENCES } from "@/config/preferences";
import { serializeVisualizationPreferences } from "@/utils/params";

const PREFERENCES_COOKIE_NAME = "meteogram-preferences";

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
  initialPreferences?: VisualizationPreferences;
  cookieReadSuccess?: boolean;
}

/**
 * Simple function to retrieve preferences from client cookie
 */
function getCookiePreferences(): VisualizationPreferences | null {
  try {
    const cookieValue = Cookies.get(PREFERENCES_COOKIE_NAME);
    if (!cookieValue) return null;

    const parsed = JSON.parse(cookieValue);
    if (typeof parsed !== "object" || parsed === null) return null;

    return { ...DEFAULT_PREFERENCES, ...parsed };
  } catch (error) {
    console.error("Failed to parse client cookie:", error);
    return null;
  }
}

export const PreferencesProvider: React.FC<PreferencesProviderProps> = ({
  children,
  initialPreferences = DEFAULT_PREFERENCES,
  cookieReadSuccess = true, // Default to true if not provided
}) => {
  const router = useRouter();

  // Debug log the initial preferences and cookieReadSuccess
  const mountedRef = useRef(false);
  useEffect(() => {
    if (!mountedRef.current) {
      console.log("Client: Initial state from server:", {
        preferences: initialPreferences,
        cookieReadSuccess: cookieReadSuccess,
      });
      mountedRef.current = true;
    }
  }, [initialPreferences, cookieReadSuccess]);

  // Use state with the initialPreferences (from server)
  const [preferences, setPreferencesState] = useState<VisualizationPreferences>(
    () => {
      // Check if server successfully read cookies
      if (cookieReadSuccess === false) {
        // Server couldn't read cookies, try on client side
        console.log(
          "Client: Server reported cookie read failure, trying client-side",
        );
        try {
          const cookie = Cookies.get(PREFERENCES_COOKIE_NAME);
          if (cookie) {
            console.log("Client: Found cookie:", cookie);
            try {
              const cookiePrefs = JSON.parse(cookie);
              console.log(
                "Client: Successfully parsed cookie into:",
                cookiePrefs,
              );
              return { ...DEFAULT_PREFERENCES, ...cookiePrefs };
            } catch (e) {
              console.error("Client: Failed to parse cookie JSON:", e);
            }
          } else {
            console.log("Client: No cookie found");
          }
        } catch (e) {
          console.error("Client: Error reading cookie:", e);
        }
      } else {
        console.log(
          "Client: Using server-provided preferences (cookieReadSuccess=true)",
        );
      }

      // Otherwise use server preferences (which could be from URL params or server cookies)
      return initialPreferences;
    },
  );

  // Save to cookie whenever state changes (and sync to URL)
  useEffect(() => {
    if (!mountedRef.current) return; // Skip initial mount render

    // Save to cookie
    try {
      console.log("Client: Saving preferences to cookie:", preferences);
      Cookies.set(PREFERENCES_COOKIE_NAME, JSON.stringify(preferences), {
        expires: 365,
        path: "/",
        sameSite: "lax",
      });
    } catch (error) {
      console.error("Failed to save preferences cookie:", error);
    }

    // Sync to URL
    const currentPath = window.location.pathname;
    const newParams = serializeVisualizationPreferences(preferences);
    const queryString = newParams.toString();
    const newUrl = `${currentPath}${queryString ? "?" + queryString : ""}`;

    // Only update if needed to avoid unnecessary history entries
    if (window.location.search !== (queryString ? "?" + queryString : "")) {
      console.log("Client: Updating URL with preferences");
      router.replace(newUrl, { scroll: false });
    }
  }, [preferences, router]);

  // Update preferences (partial)
  const updatePreferences = useCallback(
    (newPartialPrefs: Partial<VisualizationPreferences>) => {
      console.log("Client: Updating partial preferences:", newPartialPrefs);
      setPreferencesState((prev) => ({
        ...prev,
        ...newPartialPrefs,
      }));
    },
    [],
  );

  // Update a single preference
  const setPreference = useCallback(
    <K extends keyof VisualizationPreferences>(
      key: K,
      value: VisualizationPreferences[K],
    ) => {
      console.log(`Client: Setting preference ${String(key)} =`, value);
      setPreferencesState((prev) => ({
        ...prev,
        [key]: value,
      }));
    },
    [],
  );

  // Use a memoized context value to prevent unnecessary re-renders
  const contextValue = React.useMemo(
    () => ({
      preferences,
      setPreference,
      setPreferences: updatePreferences,
    }),
    [preferences, setPreference, updatePreferences],
  );

  return (
    <PreferencesContext.Provider value={contextValue}>
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
