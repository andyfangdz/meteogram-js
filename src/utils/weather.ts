import range from "lodash/range";
import { RADIUS, POLAR_RADIUS } from "wgs84";
import { CloudColumn, WeatherModel } from "../types/weather";
import {
  FEET_PER_METER,
  MODEL_CONFIGS,
  MAX_VARIABLES_PER_REQUEST,
} from "../config/weather";

/**
 * Calculate the local Earth radius at a given latitude using WGS84 ellipsoid.
 * Uses the geocentric radius formula from the wgs84 package constants.
 * @param latitudeDegrees - Latitude in degrees
 * @returns Local Earth radius in meters
 */
function getWGS84LocalRadius(latitudeDegrees: number): number {
  const latRad = (latitudeDegrees * Math.PI) / 180;
  const cosLat = Math.cos(latRad);
  const sinLat = Math.sin(latRad);

  // Using WGS84 constants from the wgs84 package
  const a = RADIUS; // Semi-major axis (equatorial radius)
  const b = POLAR_RADIUS; // Semi-minor axis (polar radius)

  const numerator = Math.sqrt(
    Math.pow(a * a * cosLat, 2) + Math.pow(b * b * sinLat, 2),
  );
  const denominator = Math.sqrt(
    Math.pow(a * cosLat, 2) + Math.pow(b * sinLat, 2),
  );

  return numerator / denominator;
}

/**
 * Convert geopotential height to mean sea level (MSL) geometric height.
 * Uses WGS84 ellipsoid model for accurate conversion based on latitude.
 * @param geopotentialMeters - Geopotential height in meters
 * @param latitudeDegrees - Latitude in degrees
 * @returns MSL geometric height in meters
 */
function geopotentialToMsl(
  geopotentialMeters: number,
  latitudeDegrees: number,
): number {
  const localRadius = getWGS84LocalRadius(latitudeDegrees);
  return (
    (localRadius * geopotentialMeters) / (localRadius - geopotentialMeters)
  );
}

export function transformWeatherData(
  responses: any[],
  model: WeatherModel,
  latitude: number,
): CloudColumn[] {
  // Use first response for time grid metadata (all chunks share same time grid)
  const mainResponse = responses[0];
  const utcOffsetSeconds = mainResponse.utcOffsetSeconds();
  const modelConfig = MODEL_CONFIGS[model];
  const forecastDataMain = mainResponse[modelConfig.forecastDataKey]()!;

  // Helper to access values across chunked responses
  const getValue = (globalIndex: number, timeIndex: number): number | null => {
    const chunkIndex = Math.floor(globalIndex / MAX_VARIABLES_PER_REQUEST);
    const varIndex = globalIndex % MAX_VARIABLES_PER_REQUEST;

    if (chunkIndex >= responses.length) return null;

    const response = responses[chunkIndex];
    const forecastData = response[modelConfig.forecastDataKey]()!;
    const variable = forecastData.variables(varIndex);
    return variable ? variable.values(timeIndex) : null;
  };

  const len = modelConfig.hpaLevels.length;
  const cloudCoverBaseIndex = 0;
  const geopotentialBaseIndex = len;
  const temperatureBaseIndex = 2 * len;
  const windSpeedBaseIndex = 3 * len;
  const windDirectionBaseIndex = 4 * len;
  const dewPointBaseIndex = 5 * len;
  const groundTempIndex = 6 * len;

  const cloudData = range(
    Number(forecastDataMain.time()),
    Number(forecastDataMain.timeEnd()),
    forecastDataMain.interval(),
  ).map((time, index) => ({
    date: new Date((time + utcOffsetSeconds) * 1000),
    cloud: modelConfig.hpaLevels
      .map((hpa, hpaIndex) => {
        const cloudCoverage = getValue(cloudCoverBaseIndex + hpaIndex, index);
        const geopotentialMeters = getValue(
          geopotentialBaseIndex + hpaIndex,
          index,
        );
        const temperature = getValue(temperatureBaseIndex + hpaIndex, index);
        const windSpeed = getValue(windSpeedBaseIndex + hpaIndex, index);
        const windDirection = getValue(windDirectionBaseIndex + hpaIndex, index);
        const dewPoint = getValue(dewPointBaseIndex + hpaIndex, index);

        // Only return valid data
        if (
          cloudCoverage == null ||
          geopotentialMeters == null ||
          temperature == null ||
          windSpeed == null ||
          windDirection == null ||
          dewPoint == null ||
          !Number.isFinite(cloudCoverage) ||
          !Number.isFinite(geopotentialMeters) ||
          !Number.isFinite(temperature) ||
          !Number.isFinite(windSpeed) ||
          !Number.isFinite(windDirection) ||
          !Number.isFinite(dewPoint)
        ) {
          return null;
        }

        return {
          hpa,
          geopotentialFt: geopotentialMeters * FEET_PER_METER,
          mslFt: geopotentialToMsl(geopotentialMeters, latitude) * FEET_PER_METER,
          cloudCoverage,
          temperature,
          dewPoint,
          windSpeed,
          windDirection,
        };
      })
      .filter(Boolean), // Remove null entries
    groundTemp: getValue(groundTempIndex, index)!,
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
