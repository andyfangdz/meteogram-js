import { fetchWeatherApi } from "openmeteo";
import range from "lodash/range";

const url = "https://api.open-meteo.com/v1/forecast";

const HPA_LEVELS = range(1000, 250, -25);

export const MODEL_NAMES = ["gfs_seamless", "gfs_hrrr"];

const CLOUD_COVER_HPA_VARS = HPA_LEVELS.map((hpa) => `cloud_cover_${hpa}hPa`);
const GEOPOTENTIAL_HEIGHT_HPA_VARS = HPA_LEVELS.map(
  (hpa) => `geopotential_height_${hpa}hPa`,
);

const VARIABLES = [...CLOUD_COVER_HPA_VARS, ...GEOPOTENTIAL_HEIGHT_HPA_VARS];

export const LOCATIONS: {
  [key: string]: { longitude: number; latitude: number };
} = {
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
};

const params = {
  cell_selection: "nearest",
};

const FEET_PER_METER = 3.28084;

export interface CloudData {
  date: Date;
  cloud: {
    hpa: number;
    mslFt: number;
    cloudCoverage: number;
    mslFtBottom: number;
    mslFtTop: number;
  }[];
}

export default async function fetchWeatherData(
  model: string,
  location: string,
): Promise<CloudData[]> {
  console.log(CLOUD_COVER_HPA_VARS, GEOPOTENTIAL_HEIGHT_HPA_VARS);
  const modelVarsKey = model === "gfs_hrrr" ? "minutely_15" : "hourly";
  const modelStepKey =
    model === "gfs_hrrr" ? "forecast_minutely_15" : "forecast_hourly";
  const modelStepSize = model === "gfs_hrrr" ? 4 * 36 : 24 * 5;
  const responses = await fetchWeatherApi(url, {
    ...params,
    ...LOCATIONS[location],
    models: model,
    [modelVarsKey]: VARIABLES.join(","),
    [modelStepKey]: modelStepSize,
  });

  // Process first location. Add a for-loop for multiple locations or weather models
  const response = responses[0];

  // Attributes for timezone and location
  const utcOffsetSeconds = response.utcOffsetSeconds();

  const forecastData =
    model === "gfs_hrrr" ? response.minutely15()! : response.hourly()!;
  console.log(forecastData);
  const cloudData = range(
    Number(forecastData.time()),
    Number(forecastData.timeEnd()),
    forecastData.interval(),
  ).map((time, index) => ({
    date: new Date((time + utcOffsetSeconds) * 1000),
    cloud: HPA_LEVELS.map((hpa, hpaIndex) => ({
      hpa,
      mslFt:
        forecastData.variables(hpaIndex + HPA_LEVELS.length)!.values(index)! *
        FEET_PER_METER,
      cloudCoverage: forecastData.variables(hpaIndex)!.values(index)!,
    })),
  }));

  const cloudDataWithMslRange = cloudData.map((dateAndCloud) => ({
    date: dateAndCloud.date,
    cloud: dateAndCloud.cloud.slice(0, -1).map((cloud, index) => ({
      ...cloud,
      mslFtBottom:
        index === 0
          ? 0
          : (dateAndCloud.cloud[index - 1].mslFt + cloud.mslFt) / 2,
      mslFtTop: (cloud.mslFt + dateAndCloud.cloud[index + 1].mslFt) / 2,
    })),
  }));

  return cloudDataWithMslRange;
}
