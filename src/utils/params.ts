import { DEFAULT_PREFERENCES } from "@/config/preferences";
import { VisualizationPreferences } from "@/types/weather";

type SearchParams = {
  useLocalTime?: string;
  highlightCeiling?: string;
  clampCoverage?: string;
  showPressureLines?: string;
  showWindBarbs?: string;
  showIsothermLines?: string;
  showIsotachLines?: string;
};

/**
 * Parses visualization preferences from search parameters, applying defaults.
 * Handles boolean flags represented as strings ("true" or "false").
 */
export function parseVisualizationPreferences(
  searchParams: SearchParams,
): VisualizationPreferences {
  const getBoolParam = (
    param: string | undefined,
    defaultValue: boolean,
  ): boolean => {
    if (param === "true") return true;
    if (param === "false") return false;
    return defaultValue;
  };

  return {
    useLocalTime: getBoolParam(
      searchParams.useLocalTime,
      DEFAULT_PREFERENCES.useLocalTime,
    ),
    highlightCeilingCoverage: getBoolParam(
      searchParams.highlightCeiling, // Note: "false" means disable, so we invert the logic slightly if needed, but getBoolParam handles it
      DEFAULT_PREFERENCES.highlightCeilingCoverage,
    ),
    clampCloudCoverageAt50Pct: getBoolParam(
      searchParams.clampCoverage, // Note: "false" means disable
      DEFAULT_PREFERENCES.clampCloudCoverageAt50Pct,
    ),
    showPressureLines: getBoolParam(
      searchParams.showPressureLines,
      DEFAULT_PREFERENCES.showPressureLines,
    ),
    showWindBarbs: getBoolParam(
      searchParams.showWindBarbs,
      DEFAULT_PREFERENCES.showWindBarbs,
    ),
    showIsothermLines: getBoolParam(
      searchParams.showIsothermLines,
      DEFAULT_PREFERENCES.showIsothermLines,
    ),
    showIsotachLines: getBoolParam(
      searchParams.showIsotachLines,
      DEFAULT_PREFERENCES.showIsotachLines,
    ),
  };
}

/**
 * Serializes visualization preferences into URLSearchParams,
 * only including values that differ from the defaults.
 */
export function serializeVisualizationPreferences(
  preferences: VisualizationPreferences,
): URLSearchParams {
  const params = new URLSearchParams();

  const setParamIfDifferent = (
    key: keyof VisualizationPreferences,
    urlKey: string,
  ) => {
    // Explicitly assert key is a valid key for DEFAULT_PREFERENCES
    const prefKey = key as keyof typeof DEFAULT_PREFERENCES;
    if (preferences[key] !== DEFAULT_PREFERENCES[prefKey]) {
      params.set(urlKey, preferences[key].toString());
    }
  };

  setParamIfDifferent("useLocalTime", "useLocalTime");
  setParamIfDifferent("highlightCeilingCoverage", "highlightCeiling");
  setParamIfDifferent("clampCloudCoverageAt50Pct", "clampCoverage");
  setParamIfDifferent("showPressureLines", "showPressureLines");
  setParamIfDifferent("showWindBarbs", "showWindBarbs");
  setParamIfDifferent("showIsothermLines", "showIsothermLines");
  setParamIfDifferent("showIsotachLines", "showIsotachLines");

  return params;
}
