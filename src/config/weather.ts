import range from "lodash/range";
import { Locations, WeatherModel } from "../types/weather";

export const API_URL = "https://api.open-meteo.com/v1/forecast";

export const GEM_HPA_LEVELS = [
  1015, 1000, 985, 970, 950, 925, 900, 875, 850, 800, 750, 700, 650, 600, 550,
  500, 450, 400, 350, 300, 275, 250,
];

export const DEFAULT_HPA_LEVELS = range(1000, 250, -25);

export const MODEL_NAMES: WeatherModel[] = [
  "best_match",
  "gfs_seamless",
  "gfs_hrrr",
  "ecmwf_ifs",
  "ecmwf_aifs025_single",
  "gem_seamless",
  "gem_hrdps_continental",
];

export type ModelConfigs = {
  [key in WeatherModel]: ModelConfig;
};

export class ModelConfig {
  constructor({
    varsKey,
    stepKey,
    stepSize,
    forecastDataKey,
    windBarbStep,
    windBarbPressureLevelStep,
    maxIsothermStepDistance,
    hpaLevels,
  }: {
    varsKey: string;
    stepKey: string;
    stepSize: number;
    forecastDataKey: "minutely15" | "hourly";
    windBarbStep: number;
    windBarbPressureLevelStep: number;
    maxIsothermStepDistance: number;
    hpaLevels: number[];
  }) {
    this.varsKey = varsKey;
    this.stepKey = stepKey;
    this.stepSize = stepSize;
    this.forecastDataKey = forecastDataKey;
    this.windBarbStep = windBarbStep;
    this.windBarbPressureLevelStep = windBarbPressureLevelStep;
    this.maxIsothermStepDistance = maxIsothermStepDistance;
    this.hpaLevels = hpaLevels;
  }

  varsKey: string;
  stepKey: string;
  stepSize: number;
  forecastDataKey: "minutely15" | "hourly";
  windBarbStep: number;
  windBarbPressureLevelStep: number;
  maxIsothermStepDistance: number;
  hpaLevels: number[];

  getCloudCoverVars() {
    return this.hpaLevels.map((hpa) => `cloud_cover_${hpa}hPa`);
  }

  getGeopotentialHeightVars() {
    return this.hpaLevels.map((hpa) => `geopotential_height_${hpa}hPa`);
  }

  getTemperatureVars() {
    return this.hpaLevels.map((hpa) => `temperature_${hpa}hPa`);
  }

  getWindSpeedVars() {
    return this.hpaLevels.map((hpa) => `wind_speed_${hpa}hPa`);
  }

  getWindDirectionVars() {
    return this.hpaLevels.map((hpa) => `wind_direction_${hpa}hPa`);
  }

  getDewPointVars() {
    return this.hpaLevels.map((hpa) => `dew_point_${hpa}hPa`);
  }

  // Returns all variables for the API request.
  // IMPORTANT: The order here determines the index order in the API response.
  // See utils/weather.ts for the index calculations that depend on this order.
  // When adding new variables, update both this method and the indices in utils/weather.ts.
  getAllVariables() {
    return [
      ...this.getCloudCoverVars(), // indices [0, N)
      ...this.getGeopotentialHeightVars(), // indices [N, 2N)
      ...this.getTemperatureVars(), // indices [2N, 3N)
      ...this.getWindSpeedVars(), // indices [3N, 4N)
      ...this.getWindDirectionVars(), // indices [4N, 5N)
      ...this.getDewPointVars(), // indices [5N, 6N)
      "temperature_2m", // index 6N (ground level temperature)
    ];
  }
}

export const MODEL_CONFIGS: ModelConfigs = {
  best_match: new ModelConfig({
    varsKey: "minutely_15",
    stepKey: "forecast_minutely_15",
    stepSize: 4 * 80, // 4 times per hour * 40 hours
    forecastDataKey: "minutely15",
    windBarbStep: 4, // Show wind barbs every hour (4 * 15min steps)
    windBarbPressureLevelStep: 4, // Show wind barbs every 4 pressure levels
    maxIsothermStepDistance: 8, // Allow isotherms to skip up to 2 hours worth of steps (8 * 15min)
    hpaLevels: DEFAULT_HPA_LEVELS,
  }),
  gfs_hrrr: new ModelConfig({
    varsKey: "minutely_15",
    stepKey: "forecast_minutely_15",
    stepSize: 4 * 40, // 4 times per hour * 40 hours
    forecastDataKey: "minutely15",
    windBarbStep: 4, // Show wind barbs every hour (4 * 15min steps)
    windBarbPressureLevelStep: 4, // Show wind barbs every 4 pressure levels
    maxIsothermStepDistance: 8, // Allow isotherms to skip up to 2 hours worth of steps (8 * 15min)
    hpaLevels: DEFAULT_HPA_LEVELS,
  }),
  gfs_seamless: new ModelConfig({
    varsKey: "hourly",
    stepKey: "forecast_hourly",
    stepSize: 24 * 7, // 24 hours * 7 days
    forecastDataKey: "hourly",
    windBarbStep: 3, // Show wind barbs every 3 hours
    windBarbPressureLevelStep: 4, // Show wind barbs every 4 pressure levels
    maxIsothermStepDistance: 6, // Allow isotherms to skip up to 6 hours
    hpaLevels: DEFAULT_HPA_LEVELS,
  }),
  ecmwf_ifs: new ModelConfig({
    varsKey: "hourly",
    stepKey: "forecast_hourly",
    stepSize: 24 * 7, // 24 hours * 7 days
    forecastDataKey: "hourly",
    windBarbStep: 3, // Show wind barbs every 3 hours
    windBarbPressureLevelStep: 1, // Show wind barbs for every pressure level
    maxIsothermStepDistance: 6, // Allow isotherms to skip up to 6 hours
    hpaLevels: DEFAULT_HPA_LEVELS,
  }),
  ecmwf_aifs025_single: new ModelConfig({
    varsKey: "hourly",
    stepKey: "forecast_hourly",
    stepSize: 24 * 7, // 24 hours * 7 days
    forecastDataKey: "hourly",
    windBarbStep: 3, // Show wind barbs every 3 hours
    windBarbPressureLevelStep: 1, // Show wind barbs for every pressure level
    maxIsothermStepDistance: 6, // Allow isotherms to skip up to 6 hours
    hpaLevels: DEFAULT_HPA_LEVELS,
  }),
  gem_seamless: new ModelConfig({
    varsKey: "hourly",
    stepKey: "forecast_hourly",
    stepSize: 24 * 7, // 24 hours * 7 days
    forecastDataKey: "hourly",
    windBarbStep: 3, // Show wind barbs every 3 hours
    windBarbPressureLevelStep: 4, // Show wind barbs for every pressure level
    maxIsothermStepDistance: 6, // Allow isotherms to skip up to 6 hours
    hpaLevels: GEM_HPA_LEVELS,
  }),
  gem_hrdps_continental: new ModelConfig({
    varsKey: "hourly",
    stepKey: "forecast_days",
    stepSize: 2,
    forecastDataKey: "hourly",
    windBarbStep: 3, // Show wind barbs every 3 hours
    windBarbPressureLevelStep: 4, // Show wind barbs for every pressure level
    maxIsothermStepDistance: 6, // Allow isotherms to skip up to 6 hours
    hpaLevels: GEM_HPA_LEVELS,
  }),
};

export const LOCATIONS: Locations = {
  KFRG: {
    longitude: -73.41639,
    latitude: 40.73443,
  },
  KNYC: {
    longitude: -73.96667,
    latitude: 40.78333,
  },
  KCDW: {
    longitude: -74.283767,
    latitude: 40.872665,
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
export const MAX_VARIABLES_PER_REQUEST = 80;

