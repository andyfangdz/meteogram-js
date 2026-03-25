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
  showDewPointDepressionLines?: string;
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
    showDewPointDepressionLines: getBoolParam(
      searchParams.showDewPointDepressionLines,
      DEFAULT_PREFERENCES.showDewPointDepressionLines,
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
  setParamIfDifferent("showDewPointDepressionLines", "showDewPointDepressionLines");

  return params;
}

export interface RouteParams {
  cruiseAltitudeFt: number;
  tasKnots: number;
  departureTime: Date;
  resolutionNM: number;
}

function getNextWholeHour(): Date {
  const now = new Date();
  now.setMinutes(0, 0, 0);
  now.setHours(now.getHours() + 1);
  return now;
}

export function parseRouteParams(searchParams: Record<string, string | undefined>): RouteParams {
  const alt = searchParams.alt ? parseInt(searchParams.alt, 10) : 6000;
  const tas = searchParams.tas ? parseInt(searchParams.tas, 10) : 120;
  const res = searchParams.res ? Math.max(5, parseInt(searchParams.res, 10)) : 25;
  const dep = searchParams.dep ? new Date(searchParams.dep) : getNextWholeHour();

  return {
    cruiseAltitudeFt: isNaN(alt) ? 6000 : alt,
    tasKnots: isNaN(tas) ? 120 : tas,
    resolutionNM: isNaN(res) ? 25 : res,
    departureTime: isNaN(dep.getTime()) ? getNextWholeHour() : dep,
  };
}

export function serializeRouteParams(params: RouteParams): URLSearchParams {
  const urlParams = new URLSearchParams();
  if (params.cruiseAltitudeFt !== 6000) urlParams.set("alt", params.cruiseAltitudeFt.toString());
  if (params.tasKnots !== 120) urlParams.set("tas", params.tasKnots.toString());
  if (params.resolutionNM !== 25) urlParams.set("res", params.resolutionNM.toString());
  // Always serialize departure time (no meaningful default to compare against)
  urlParams.set("dep", params.departureTime.toISOString());
  return urlParams;
}
