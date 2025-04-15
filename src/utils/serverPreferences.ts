import { cookies } from "next/headers";
import { VisualizationPreferences } from "@/types/weather";
import { DEFAULT_PREFERENCES } from "@/config/preferences";
import { parseVisualizationPreferences } from "@/utils/params";

const PREFERENCES_COOKIE_NAME = "meteogram-preferences";

interface ServerPreferencesResult {
  preferences: VisualizationPreferences;
  cookieReadSuccess: boolean;
}

/**
 * Reads preferences from cookies on the server-side
 * Returns both the preferences and a flag indicating whether cookies were successfully read
 */
export async function getServerPreferences(): Promise<ServerPreferencesResult> {
  let cookieReadSuccess = false;
  try {
    // Get the cookie store - cookies() returns a Promise in newer Next.js
    console.log("Server: Attempting to read cookies");
    const cookieStore = await cookies();

    // Successfully accessed cookie API
    cookieReadSuccess = true;

    // Get the cookie value
    const cookieValue = cookieStore.get(PREFERENCES_COOKIE_NAME)?.value;

    if (!cookieValue) {
      console.log("Server: No preferences cookie found, using defaults");
      return {
        preferences: DEFAULT_PREFERENCES,
        cookieReadSuccess: true,
      };
    }

    try {
      const parsedCookie = JSON.parse(cookieValue);
      console.log("Server: Using preferences from cookie:", parsedCookie);
      // Ensure all required preference fields are present
      return {
        preferences: { ...DEFAULT_PREFERENCES, ...parsedCookie },
        cookieReadSuccess: true,
      };
    } catch (error) {
      console.error("Server: Failed to parse preferences cookie:", error);
      return {
        preferences: DEFAULT_PREFERENCES,
        cookieReadSuccess: false,
      };
    }
  } catch (error) {
    console.error("Server: Error accessing cookies:", error);
    return {
      preferences: DEFAULT_PREFERENCES,
      cookieReadSuccess: false,
    };
  }
}

/**
 * Combines preferences from searchParams with serverPreferences
 * SearchParams take precedence if they exist
 */
export async function getInitialPreferences(
  searchParams: Record<string, string | string[] | undefined>,
): Promise<ServerPreferencesResult> {
  // Define the known preference URL parameter names
  const preferenceParamNames = [
    "useLocalTime",
    "highlightCeiling",
    "clampCoverage",
    "showPressureLines",
    "showWindBarbs",
    "showIsothermLines",
  ];

  // Check if any preference-related parameters exist in the URL
  const hasPreferenceParams = Object.keys(searchParams).some((key) =>
    preferenceParamNames.includes(key),
  );

  if (hasPreferenceParams) {
    // Convert searchParams to format expected by parseVisualizationPreferences
    const parsedParams: Record<string, string> = {};

    Object.entries(searchParams).forEach(([key, value]) => {
      if (typeof value === "string") {
        parsedParams[key] = value;
      } else if (Array.isArray(value) && value.length > 0) {
        parsedParams[key] = value[0];
      }
    });

    const preferencesFromUrl = parseVisualizationPreferences(parsedParams);
    console.log("Server: Using preferences from URL", preferencesFromUrl);
    return {
      preferences: { ...DEFAULT_PREFERENCES, ...preferencesFromUrl },
      cookieReadSuccess: true, // URL params override cookies anyway
    };
  }

  // Fall back to cookies
  console.log("Server: No URL preferences, falling back to cookie");
  return await getServerPreferences();
}
