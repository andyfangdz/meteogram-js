import range from "lodash/range";
import { CloudColumn, WeatherModel } from "../types/weather";
import { FEET_PER_METER, HPA_LEVELS, MODEL_CONFIGS } from "../config/weather";

const EARTH_RADIUS_METERS = 6371000;

function geopotentialToMsl(geopotentialMeters: number): number {
  return (
    (EARTH_RADIUS_METERS * geopotentialMeters) /
    (EARTH_RADIUS_METERS - geopotentialMeters)
  );
}

export function transformWeatherData(
  response: any,
  model: WeatherModel,
): CloudColumn[] {
  const utcOffsetSeconds = response.utcOffsetSeconds();
  const modelConfig = MODEL_CONFIGS[model];
  const forecastData = response[modelConfig.forecastDataKey]()!;

  // Calculate base indices for each variable type
  const cloudCoverBaseIndex = 0;
  const geopotentialBaseIndex = HPA_LEVELS.length;
  const temperatureBaseIndex = 2 * HPA_LEVELS.length;
  const windSpeedBaseIndex = 3 * HPA_LEVELS.length;
  const windDirectionBaseIndex = 4 * HPA_LEVELS.length;
  const groundTempIndex = 5 * HPA_LEVELS.length;

  const cloudData = range(
    Number(forecastData.time()),
    Number(forecastData.timeEnd()),
    forecastData.interval(),
  ).map((time, index) => ({
    date: new Date((time + utcOffsetSeconds) * 1000),
    cloud: HPA_LEVELS.map((hpa, hpaIndex) => {
      const cloudCoverage = forecastData
        .variables(cloudCoverBaseIndex + hpaIndex)!
        .values(index)!;
      const geopotentialMeters = forecastData
        .variables(geopotentialBaseIndex + hpaIndex)!
        .values(index)!;
      const temperature = forecastData
        .variables(temperatureBaseIndex + hpaIndex)!
        .values(index)!;
      const windSpeed = forecastData
        .variables(windSpeedBaseIndex + hpaIndex)!
        .values(index)!;
      const windDirection = forecastData
        .variables(windDirectionBaseIndex + hpaIndex)!
        .values(index)!;

      return {
        hpa,
        geopotentialFt: geopotentialMeters * FEET_PER_METER,
        mslFt: geopotentialToMsl(geopotentialMeters) * FEET_PER_METER,
        cloudCoverage,
        temperature,
        windSpeed,
        windDirection,
      };
    }),
    groundTemp: forecastData
      .variables(groundTempIndex)! // temperature_2m is after all HPA variables
      .values(index)!,
  }));

  return cloudData.map((dateAndCloud) => ({
    date: dateAndCloud.date,
    cloud: dateAndCloud.cloud.slice(0, -1).map((cloud, index) => ({
      ...cloud,
      mslFtBottom:
        index === 0
          ? 0
          : (dateAndCloud.cloud[index - 1].mslFt + cloud.mslFt) / 2,
      mslFtTop: (cloud.mslFt + dateAndCloud.cloud[index + 1].mslFt) / 2,
    })),
    groundTemp: dateAndCloud.groundTemp,
  }));
}
