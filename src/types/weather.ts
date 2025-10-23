export interface CloudCell {
  hpa: number;
  mslFt: number;
  geopotentialFt: number;
  cloudCoverage: number;
  mslFtBottom: number;
  mslFtTop: number;
  temperature: number; // Temperature in Celsius
  windSpeed: number; // Wind speed in km/h
  windDirection: number; // Wind direction in degrees (0-360)
}

export interface CloudColumn {
  date: Date;
  cloud: CloudCell[];
  groundTemp: number; // Temperature in Celsius at 2m above ground
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

export interface VisualizationPreferences {
  useLocalTime: boolean;
  highlightCeilingCoverage: boolean;
  clampCloudCoverageAt50Pct: boolean;
  showPressureLines: boolean;
  showWindBarbs: boolean;
  showIsothermLines: boolean;
  showIsotachLines: boolean;
}
