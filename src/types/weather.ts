export interface CloudCell {
  hpa: number;
  mslFt: number;
  geopotentialFt: number;
  cloudCoverage: number;
  mslFtBottom: number;
  mslFtTop: number;
  temperature: number; // Temperature in Celsius
  dewPoint: number; // Dew point in Celsius
  windSpeed: number; // Wind speed in km/h
  windDirection: number; // Wind direction in degrees (0-360)
  // Environmental lapse rate from this cell to the next-higher-altitude cell, °C/km.
  // null on the topmost cell where there is no level above.
  lapseRateAboveCPerKm: number | null;
  // Saturated (moist) adiabatic lapse rate at this cell's (T, p), °C/km.
  malrCPerKm: number;
  // Continuous instability score, K/km. Positive = unstable, negative =
  // stable. The topmost cell extrapolates from the layer below (so the
  // tint covers it) — only null when the column has fewer than two cells.
  instabilityKPerKm: number | null;
}

export interface CloudColumn {
  date: Date;
  cloud: CloudCell[];
  groundTemp: number; // Temperature in Celsius at 2m above ground
  groundDewPoint: number; // Dew point in Celsius at 2m above ground
}

export interface Location {
  longitude: number;
  latitude: number;
}

export interface LocationWithDescription extends Location {
  description: string;
}

export interface Locations {
  [key: string]: Location;
}

export interface LocationsWithDescription {
  [key: string]: LocationWithDescription;
}

export type WeatherModel =
  | "best_match"
  | "gfs_seamless"
  | "gfs_hrrr"
  | "ecmwf_ifs"
  | "ecmwf_aifs025_single"
  | "gem_seamless"
  | "gem_hrdps_continental";

export interface WeatherApiParams {
  cell_selection: string;
  longitude: number;
  latitude: number;
  models: string;
  [key: string]: any;
}

export const PARCEL_MODES = ["surface", "mixed-100", "most-unstable"] as const;
export type ParcelMode = (typeof PARCEL_MODES)[number];

export interface VisualizationPreferences {
  useLocalTime: boolean;
  highlightCeilingCoverage: boolean;
  clampCloudCoverageAt50Pct: boolean;
  showPressureLines: boolean;
  showWindBarbs: boolean;
  showIsothermLines: boolean;
  showIsotachLines: boolean;
  showDewPointDepressionLines: boolean;
  showStabilityTint: boolean;
  showCondensationLevels: boolean;
  showParcelBuoyancy: boolean;
  parcelMode: ParcelMode;
}
