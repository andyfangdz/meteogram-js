import { FEET_PER_METER } from "@/config/weather";

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

export function geopotentialToMslMeters(geopotentialMeters: number): number {
  const EARTH_RADIUS_METERS = 6371000;
  return (
    (EARTH_RADIUS_METERS * geopotentialMeters) /
    (EARTH_RADIUS_METERS - geopotentialMeters)
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
