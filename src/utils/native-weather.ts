import { FEET_PER_METER } from "@/config/weather";
import type {
  CloudColumn,
  NativeWeatherModel,
  WeatherSource,
} from "@/types/weather";
import {
  completeCloudColumn,
  geopotentialToMsl,
  type BaseCloudCell,
} from "./weather";

export interface NativeForecast {
  model: NativeWeatherModel;
  dataset: string;
  init_time: string;
  snapshot_id: string;
  grid: { latitude: number; longitude: number; cell_id: number };
  levels_hpa: number[];
  fields: string[];
  rows: (number | number[])[][];
  units: Record<string, string>;
  native_wind_frame: "grid-relative" | "earth-relative";
  wind_rotation_degrees: number;
  available_leads: number[];
  expected_leads: number[];
  missing_leads: number[];
  forecast_complete: boolean;
  freshness: { available: boolean };
}

const regionalLevels = Array.from({ length: 30 }, (_, i) => 1000 - i * 25);
const aifsLevels = [1000, 925, 850, 700, 600, 500, 400, 300];
const surface = new Set(["temperature_2m", "dew_point_temperature_2m"]);
const finite = (v: unknown): v is number =>
  typeof v === "number" && Number.isFinite(v);
const record = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);
const same = (a: unknown, b: number[]) =>
  Array.isArray(a) && a.length === b.length && a.every((v, i) => v === b[i]);

export function parseNativeForecast(
  value: unknown,
  model: NativeWeatherModel,
): NativeForecast {
  if (
    !record(value) ||
    value.model !== model ||
    !record(value.grid) ||
    !finite(value.grid.latitude) ||
    !finite(value.grid.longitude) ||
    !Number.isInteger(value.grid.cell_id) ||
    !record(value.freshness) ||
    value.freshness.available !== true ||
    typeof value.init_time !== "string" ||
    !Number.isFinite(nativeInitTime(value.init_time)) ||
    typeof value.snapshot_id !== "string" ||
    !value.snapshot_id ||
    !finite(value.wind_rotation_degrees)
  ) {
    throw new Error("Invalid or stale native forecast identity");
  }
  const dataset =
    model === "aifs"
      ? "ecmwf-aifs-single-forecast-virtual"
      : model === "gfs"
        ? "noaa-gfs-forecast-virtual"
        : model === "rrfs"
          ? "noaa-rrfs-forecast-84-hour-virtual"
          : value.dataset === "noaa-hrrr-forecast-18-hour-virtual"
            ? "noaa-hrrr-forecast-18-hour-virtual"
            : "noaa-hrrr-forecast-48-hour-virtual";
  const expected =
    model === "gfs"
      ? [
          ...Array.from({ length: 121 }, (_, i) => i),
          ...Array.from({ length: 20 }, (_, i) => 123 + i * 3),
        ]
      : model === "aifs"
        ? Array.from({ length: 31 }, (_, i) => i * 6)
        : Array.from(
            {
              length:
                model === "rrfs" ? 85 : dataset.includes("18-hour") ? 19 : 49,
            },
            (_, i) => i,
          );
  const levels = model === "aifs" ? aifsLevels : regionalLevels;
  const required = [
    "lead_hour",
    "geopotential_height",
    "temperature",
    "wind_u",
    "wind_v",
    ...(model === "gfs"
      ? ["relative_humidity", "cloud_cover"]
      : model === "aifs"
        ? ["specific_humidity"]
        : ["relative_humidity", "dew_point_temperature"]),
    ...surface,
  ];
  const fields = value.fields;
  const leads = value.available_leads;
  const rows = value.rows;
  if (
    value.dataset !== dataset ||
    !same(value.levels_hpa, levels) ||
    !same(value.expected_leads, expected) ||
    !Array.isArray(fields) ||
    !fields.every((v) => typeof v === "string") ||
    fields[0] !== "lead_hour" ||
    new Set(fields).size !== fields.length ||
    required.some((f) => !fields.includes(f)) ||
    !Array.isArray(leads) ||
    leads.length === 0 ||
    !leads.every(
      (v, i) =>
        Number.isInteger(v) &&
        expected.includes(v) &&
        (i === 0 || v > leads[i - 1]),
    ) ||
    !same(
      value.missing_leads,
      expected.filter((h) => !leads.includes(h)),
    ) ||
    value.forecast_complete !== (leads.length === expected.length) ||
    !Array.isArray(rows) ||
    rows.length !== leads.length ||
    rows.some(
      (row, i) =>
        !Array.isArray(row) ||
        row.length !== fields.length ||
        row[0] !== leads[i] ||
        row.some((v, f) =>
          f === 0
            ? !finite(v)
            : surface.has(fields[f])
              ? !finite(v)
              : !Array.isArray(v) ||
                v.length !== levels.length ||
                !v.every(finite),
        ),
    )
  ) {
    throw new Error("Invalid native forecast schema or forecast hours");
  }
  const units = value.units;
  if (
    !record(units) ||
    units.geopotential_height !== "m" ||
    units.temperature !== "degree_Celsius" ||
    units.wind_u !== "m s-1" ||
    units.wind_v !== "m s-1" ||
    (model === "aifs"
      ? !["1", "kg kg-1", "kg/kg"].includes(String(units.specific_humidity))
      : units.relative_humidity !== "percent") ||
    (model === "gfs" && units.cloud_cover !== "percent") ||
    ((model === "hrrr" || model === "rrfs") &&
      units.dew_point_temperature !== "degree_Celsius") ||
    value.native_wind_frame !==
      (model === "hrrr" || model === "rrfs"
        ? "grid-relative"
        : "earth-relative") ||
    (value.native_wind_frame === "earth-relative" &&
      value.wind_rotation_degrees !== 0)
  ) {
    throw new Error("Unsupported native units or wind frame");
  }
  return value as unknown as NativeForecast;
}

/** Source initialization timestamps without a suffix are explicitly UTC. */
export function nativeInitTime(init: string): number {
  return Date.parse(/(?:Z|[+-]\d{2}:\d{2})$/i.test(init) ? init : init + "Z");
}

/** Sundqvist humidity cloud estimate with pressure-dependent critical RH.
 * See Open-Meteo Meteorology.swift and the ECMWF cloud parameterization reference.
 */
export function humidityCloudCover(rh: number, hpa: number): number {
  const critical = 0.7 + 0.2 * Math.exp(1 - Math.pow(1013.25 / hpa, 4));
  return (
    Math.max(0, 1 - Math.sqrt(Math.max(0, 1 - rh / 100) / (1 - critical))) * 100
  );
}

/** Magnus water dew point. Zero vapour has no finite dew point. */
export function dewPointFromHumidity(
  temperature: number,
  rh: number,
): number | null {
  if (rh <= 0) return null;
  const gamma =
    Math.log(Math.min(rh, 100) / 100) +
    (17.625 * temperature) / (243.04 + temperature);
  const value = (243.04 * gamma) / (17.625 - gamma);
  return Number.isFinite(value) ? value : null;
}

function vapourPressure(q: number, hpa: number): number {
  return (q * hpa * 100) / (0.622 + 0.378 * q);
}

/** Murphy–Koop saturation pressures, with ECMWF's quadratic mixed phase. */
export function specificHumidityToRh(
  q: number,
  temperature: number,
  hpa: number,
): number {
  if (q < 0 || q >= 1) return NaN;
  const t = temperature + 273.15;
  const ice = Math.exp(
    9.550426 - 5723.265 / t + 3.53068 * Math.log(t) - 0.00728332 * t,
  );
  const water = Math.exp(
    54.842763 -
      6763.22 / t -
      4.21 * Math.log(t) +
      0.000367 * t +
      Math.tanh(0.0415 * (t - 218.8)) *
        (53.878 - 1331.22 / t - 9.44523 * Math.log(t) + 0.014025 * t),
  );
  const liquid = Math.max(0, Math.min(1, (t - 250.16) / 23)) ** 2;
  return Math.max(
    0,
    Math.min(
      100,
      (100 * vapourPressure(q, hpa)) / (liquid * water + (1 - liquid) * ice),
    ),
  );
}

/** Rotate grid-relative components into true east/north before computing from direction. */
export function nativeWind(u: number, v: number, rotationDegrees: number) {
  const theta = (rotationDegrees * Math.PI) / 180;
  const east = u * Math.cos(theta) + v * Math.sin(theta);
  const north = v * Math.cos(theta) - u * Math.sin(theta);
  return {
    windSpeed: Math.hypot(east, north) * 3.6,
    windDirection:
      east === 0 && north === 0
        ? 0
        : ((Math.atan2(east, north) * 180) / Math.PI + 540) % 360,
  };
}

export function transformNativeForecast(
  forecast: NativeForecast,
  now = Date.now(),
): {
  data: CloudColumn[];
  source: WeatherSource;
} {
  const init = nativeInitTime(forecast.init_time);
  const ageHours = (now - init) / 3_600_000;
  if (
    ageHours < -1 ||
    ageHours >
      (forecast.model === "hrrr" || forecast.model === "rrfs" ? 24 : 48)
  ) {
    throw new Error("Native forecast initialization is stale");
  }
  const positions = new Map(forecast.fields.map((f, i) => [f, i]));
  const cadenceMinutes = forecast.model === "aifs" ? 360 : 60;
  const cutoff =
    Math.floor(now / (cadenceMinutes * 60_000)) * cadenceMinutes * 60_000;
  const data = forecast.rows.flatMap((row) => {
    const lead = row[0] as number;
    const date = new Date(init + lead * 3_600_000);
    if (date.getTime() < cutoff) return [];
    const at = (name: string, level: number) =>
      (row[positions.get(name)!] as number[])[level];
    const cloud = forecast.levels_hpa.flatMap((hpa, level): BaseCloudCell[] => {
      const temperature = at("temperature", level);
      const q =
        forecast.model === "aifs" ? at("specific_humidity", level) : null;
      const rh =
        q === null
          ? at("relative_humidity", level)
          : specificHumidityToRh(q, temperature, hpa);
      const dewPoint = positions.has("dew_point_temperature")
        ? at("dew_point_temperature", level)
        : q === null
          ? dewPointFromHumidity(temperature, rh)
          : vapourPressure(q, hpa) > 0
            ? (243.04 * Math.log(vapourPressure(q, hpa) / 610.94)) /
              (17.625 - Math.log(vapourPressure(q, hpa) / 610.94))
            : null;
      const geopotential = at("geopotential_height", level);
      const wind = nativeWind(
        at("wind_u", level),
        at("wind_v", level),
        forecast.wind_rotation_degrees,
      );
      const cell = {
        hpa,
        temperature,
        dewPoint: dewPoint ?? NaN,
        geopotentialFt: geopotential * FEET_PER_METER,
        mslFt:
          geopotentialToMsl(geopotential, forecast.grid.latitude) *
          FEET_PER_METER,
        cloudCoverage: positions.has("cloud_cover")
          ? at("cloud_cover", level)
          : humidityCloudCover(rh, hpa),
        ...wind,
      };
      return Object.values(cell).every(Number.isFinite) ? [cell] : [];
    });
    if (cloud.length === 0)
      throw new Error("Native forecast contains no usable pressure levels");
    return [
      {
        ...completeCloudColumn({
          date,
          cloud,
          groundTemp: row[positions.get("temperature_2m")!] as number,
          groundDewPoint: row[
            positions.get("dew_point_temperature_2m")!
          ] as number,
        }),
        durationMinutes:
          forecast.model === "aifs"
            ? 360
            : forecast.model === "gfs" && lead >= 120
              ? 180
              : 60,
      },
    ];
  });
  if (data.length < 2 || !data.some((d) => d.date.getTime() >= now)) {
    throw new Error("Native forecast has insufficient current/future hours");
  }
  return {
    data,
    source: {
      provider: "dynamical",
      model: forecast.model,
      initializedAt: new Date(init).toISOString(),
      complete: forecast.forecast_complete,
      availableHours: forecast.available_leads.length,
      expectedHours: forecast.expected_leads.length,
      cloudMethod: positions.has("cloud_cover") ? "model" : "humidity-estimate",
      cadenceMinutes,
    },
  };
}
