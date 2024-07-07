import { fetchWeatherApi } from "openmeteo";
import range from "lodash/range";

const url = "https://api.open-meteo.com/v1/forecast";

const HPA_LEVELS = range(1000, 500, -25);

const CLOUD_COVER_HPA_VARS = HPA_LEVELS.map((hpa) => `cloud_cover_${hpa}hPa`);
const GEOPOTENTIAL_HEIGHT_HPA_VARS = HPA_LEVELS.map(
  (hpa) => `geopotential_height_${hpa}hPa`
);

const VARIABLES = [...CLOUD_COVER_HPA_VARS, ...GEOPOTENTIAL_HEIGHT_HPA_VARS];

const params = {
  latitude: 40.729275,
  longitude: -73.41342,
  minutely_15: VARIABLES.join(","),
  models: "gfs_hrrr",
  forecast_minutely_15: 4 * 36,
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

export default async function fetchWeatherData(): Promise<CloudData[]> {
  console.log(CLOUD_COVER_HPA_VARS, GEOPOTENTIAL_HEIGHT_HPA_VARS);
  const responses = await fetchWeatherApi(url, params);

  // Process first location. Add a for-loop for multiple locations or weather models
  const response = responses[0];

  // Attributes for timezone and location
  const utcOffsetSeconds = response.utcOffsetSeconds();

  const forecastData = response.minutely15()!;

  const cloudData = range(
    Number(forecastData.time()),
    Number(forecastData.timeEnd()),
    forecastData.interval()
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
    cloud: dateAndCloud.cloud.slice(0, -1)
      .map((cloud, index) => ({
        ...cloud,
        mslFtBottom:
          index === 0
            ? 0
            : (dateAndCloud.cloud[index - 1].mslFt + cloud.mslFt) / 2,
        mslFtTop: (cloud.mslFt + dateAndCloud.cloud[index + 1].mslFt) / 2,
      }))
      ,
  }));

  return cloudDataWithMslRange;
}
