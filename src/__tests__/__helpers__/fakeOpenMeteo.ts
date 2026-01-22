import { FEET_PER_METER } from "@/config/weather";
import { RADIUS, POLAR_RADIUS } from "wgs84";

export type ForecastKey = "hourly" | "minutely15";

type FakeVar = { values: (index: number) => number };

type FakeForecast = {
  time: () => number;
  timeEnd: () => number;
  interval: () => number;
  variables: (index: number) => FakeVar;
};

type FakeResponse = {
  utcOffsetSeconds: () => number;
  hourly?: () => FakeForecast;
  minutely15?: () => FakeForecast;
};

/**
 * Calculate the local Earth radius at a given latitude using WGS84 ellipsoid.
 * Uses constants from the wgs84 package.
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

export function geopotentialToMslMeters(
  geopotentialMeters: number,
  latitudeDegrees: number,
): number {
  const localRadius = getWGS84LocalRadius(latitudeDegrees);
  return (
    (localRadius * geopotentialMeters) / (localRadius - geopotentialMeters)
  );
}

export function createFakeOpenMeteoResponse(
  forecastKey: ForecastKey,
  hpaLevels: number[],
  steps: number = 1,
): FakeResponse {
  const start = 0;
  const interval = 1;
  const endExclusive = start + steps;

  // Build variables in the exact order used by getAllVariables
  const variables: number[][] = [];

  // cloud cover per level
  hpaLevels.forEach((_hpa, hpaIndex) => {
    const series = Array.from({ length: steps }, (_v, t) => 40 + hpaIndex * 10 + t);
    variables.push(series);
  });

  // geopotential_height per level (monotonic with level index)
  hpaLevels.forEach((_hpa, hpaIndex) => {
    const series = Array.from({ length: steps }, (_v, t) => 100 + hpaIndex * 100 + t);
    variables.push(series);
  });

  // temperature per level
  hpaLevels.forEach((_hpa, hpaIndex) => {
    const series = Array.from({ length: steps }, (_v, t) => 10 - hpaIndex * 2 - t);
    variables.push(series);
  });

  // wind speed per level (km/h)
  hpaLevels.forEach((_hpa, hpaIndex) => {
    const series = Array.from({ length: steps }, (_v, t) => 20 + hpaIndex * 5 + t);
    variables.push(series);
  });

  // wind direction per level (deg)
  hpaLevels.forEach((_hpa, hpaIndex) => {
    const series = Array.from({ length: steps }, (_v, t) => 45 + hpaIndex * 45);
    variables.push(series);
  });

  // dew point per level (temperature minus spread, where spread is 3°C at ground level and increases by 1°C per altitude level)
  hpaLevels.forEach((_hpa, hpaIndex) => {
    const series = Array.from({ length: steps }, (_v, t) => 10 - hpaIndex * 2 - t - (3 + hpaIndex));
    variables.push(series);
  });

  // ground temperature (2m)
  const groundTemp = Array.from({ length: steps }, (_v, t) => 15 - t);
  variables.push(groundTemp);

  const forecast: FakeForecast = {
    time: () => start,
    timeEnd: () => endExclusive,
    interval: () => interval,
    variables: (index: number) => ({
      values: (timeIndex: number) => variables[index][timeIndex],
    }),
  };

  const response: FakeResponse = {
    utcOffsetSeconds: () => 0,
    [forecastKey]: () => forecast,
  };

  return response;
}
