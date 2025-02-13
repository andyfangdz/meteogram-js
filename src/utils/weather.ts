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

      // Only return valid data
      if (
        cloudCoverage == null ||
        geopotentialMeters == null ||
        temperature == null ||
        windSpeed == null ||
        windDirection == null ||
        !Number.isFinite(cloudCoverage) ||
        !Number.isFinite(geopotentialMeters) ||
        !Number.isFinite(temperature) ||
        !Number.isFinite(windSpeed) ||
        !Number.isFinite(windDirection)
      ) {
        return null;
      }

      return {
        hpa,
        geopotentialFt: geopotentialMeters * FEET_PER_METER,
        mslFt: geopotentialToMsl(geopotentialMeters) * FEET_PER_METER,
        cloudCoverage,
        temperature,
        windSpeed,
        windDirection,
      };
    }).filter(Boolean), // Remove null entries
    groundTemp: forecastData
      .variables(groundTempIndex)! // temperature_2m is after all HPA variables
      .values(index)!,
  }));

  return cloudData.map((dateAndCloud) => {
    // Filter out null entries and sort by pressure (height)
    const validClouds = dateAndCloud.cloud
      .filter((cloud) => cloud != null)
      .sort((a, b) => b!.hpa - a!.hpa);

    return {
      date: dateAndCloud.date,
      cloud: validClouds.map((cloud, index) => {
        const prevCloud = index > 0 ? validClouds[index - 1] : null;
        const nextCloud =
          index < validClouds.length - 1 ? validClouds[index + 1] : null;

        // Calculate mslFtBottom and mslFtTop using available adjacent levels
        const mslFtBottom = prevCloud
          ? (prevCloud.mslFt + cloud!.mslFt) / 2
          : cloud!.mslFt - 500; // If no lower level, go 500ft below

        const mslFtTop = nextCloud
          ? (cloud!.mslFt + nextCloud.mslFt) / 2
          : cloud!.mslFt + 500; // If no upper level, go 500ft above

        return {
          ...cloud!,
          mslFtBottom,
          mslFtTop,
        };
      }),
      groundTemp: dateAndCloud.groundTemp,
    };
  });
}
