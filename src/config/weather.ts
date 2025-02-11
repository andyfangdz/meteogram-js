import range from "lodash/range";
import { Locations, WeatherModel, ModelConfigs } from "../types/weather";

export const API_URL = "https://api.open-meteo.com/v1/forecast";

export const HPA_LEVELS = range(1000, 250, -25);

export const MODEL_NAMES: WeatherModel[] = ["gfs_seamless", "gfs_hrrr"];

export const MODEL_CONFIGS: ModelConfigs = {
  gfs_hrrr: {
    varsKey: "minutely_15",
    stepKey: "forecast_minutely_15",
    stepSize: 4 * 40, // 4 times per hour * 40 hours
    forecastDataKey: "minutely15",
  },
  gfs_seamless: {
    varsKey: "hourly",
    stepKey: "forecast_hourly",
    stepSize: 24 * 7, // 24 hours * 7 days
    forecastDataKey: "hourly",
  },
};

export const CLOUD_COVER_HPA_VARS = HPA_LEVELS.map(
  (hpa) => `cloud_cover_${hpa}hPa`,
);
export const GEOPOTENTIAL_HEIGHT_HPA_VARS = HPA_LEVELS.map(
  (hpa) => `geopotential_height_${hpa}hPa`,
);
export const TEMPERATURE_HPA_VARS = HPA_LEVELS.map(
  (hpa) => `temperature_${hpa}hPa`,
);

export const VARIABLES = [
  ...CLOUD_COVER_HPA_VARS,
  ...GEOPOTENTIAL_HEIGHT_HPA_VARS,
  ...TEMPERATURE_HPA_VARS,
];

export const LOCATIONS: Locations = {
  KFRG: {
    longitude: -73.41639,
    latitude: 40.73443,
  },
  KNYC: {
    longitude: -73.9666699,
    latitude: 40.78333,
  },
  KHWV: {
    longitude: -72.8688899,
    latitude: 40.82167,
  },
  KISP: {
    longitude: -73.10167,
    latitude: 40.79389,
  },
  KBDR: {
    longitude: -73.12618,
    latitude: 41.16348,
  },
  "South Practice Area": {
    longitude: -73.13705,
    latitude: 40.62212,
  },
  "North Practice Area": {
    longitude: -73.28619,
    latitude: 40.95599,
  },
};

export const DEFAULT_PARAMS = {
  cell_selection: "nearest",
};

export const FEET_PER_METER = 3.28084;
