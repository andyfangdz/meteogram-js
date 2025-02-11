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

  const cloudData = range(
    Number(forecastData.time()),
    Number(forecastData.timeEnd()),
    forecastData.interval(),
  ).map((time, index) => ({
    date: new Date((time + utcOffsetSeconds) * 1000),
    cloud: HPA_LEVELS.map((hpa, hpaIndex) => {
      const geopotentialMeters = forecastData
        .variables(hpaIndex + HPA_LEVELS.length)!
        .values(index)!;
      const temperature = forecastData
        .variables(hpaIndex + 2 * HPA_LEVELS.length)!
        .values(index)!;
      return {
        hpa,
        geopotentialFt: geopotentialMeters * FEET_PER_METER,
        mslFt: geopotentialToMsl(geopotentialMeters) * FEET_PER_METER,
        cloudCoverage: forecastData.variables(hpaIndex)!.values(index)!,
        temperature,
      };
    }),
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
  }));
}
