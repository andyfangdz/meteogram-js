import { describe, expect, it } from "vitest";
import { nativeFixture } from "./__helpers__/nativeForecast";
import {
  parseNativeForecast,
  transformNativeForecast,
  nativeWind,
  nativeInitTime,
  humidityCloudCover,
  dewPointFromHumidity,
  specificHumidityToRh,
} from "@/utils/native-weather";

describe("native meteorological adapter", () => {
  it("uses UTC timestamps, preserves missing hours and finishes chart geometry", () => {
    const f = parseNativeForecast(nativeFixture(), "hrrr");
    const { data, source } = transformNativeForecast(
      f,
      Date.parse("2026-10-18T19:15:00Z"),
    );
    expect(data.map((d) => d.date.toISOString())).toEqual([
      "2026-10-18T19:00:00.000Z",
      "2026-10-18T21:00:00.000Z",
      "2026-10-18T23:00:00.000Z",
    ]);
    expect(data.map((d) => d.durationMinutes)).toEqual([60, 60, 60]);
    expect(source).toMatchObject({
      provider: "dynamical",
      complete: false,
      availableHours: 3,
      expectedHours: 19,
      cloudMethod: "humidity-estimate",
    });
    expect(data[0].cloud.map((c) => c.hpa)).toEqual(f.levels_hpa);
    expect(data[0].cloud[1].mslFt).toBeGreaterThan(
      data[0].cloud[1].geopotentialFt,
    );
    expect(data[0].cloud[1].mslFtTop).toBeGreaterThan(
      data[0].cloud[1].mslFtBottom,
    );
    expect(data[0].cloud[1].lapseRateAboveCPerKm).toBeGreaterThan(0);
    expect(data[0].cloud[0].dewPoint).toBe(10);
    expect(data[0].cloud[0].windDirection).toBeCloseTo(284.452);
  });

  it("converts vector units and rotates grid winds with the correct sign", () => {
    expect(nativeWind(10, 0, 0)).toEqual({ windSpeed: 36, windDirection: 270 });
    expect(nativeWind(0, 10, 0).windDirection).toBe(180);
    expect(nativeWind(0, 10, 14.452).windDirection).toBeCloseTo(194.452);
    expect(nativeWind(0, 0, 14)).toEqual({ windSpeed: 0, windDirection: 0 });
    expect(nativeWind(-10, 0, -14.452).windDirection).toBeCloseTo(75.548);
  });

  it("derives humidity clouds and dew point against known reference values", () => {
    expect(dewPointFromHumidity(20, 50)).toBeCloseTo(9.261, 2);
    expect(dewPointFromHumidity(20, 100)).toBeCloseTo(20);
    expect(dewPointFromHumidity(20, 0)).toBeNull();
    expect(humidityCloudCover(50, 500)).toBe(0);
    expect(humidityCloudCover(100, 500)).toBe(100);
    expect(humidityCloudCover(90, 500)).toBeCloseTo(42.265, 2);
    expect(specificHumidityToRh(0.007302, 20, 1000)).toBeCloseTo(50, 0);
    expect(specificHumidityToRh(-0.1, -20, 500)).toBeNaN();
  });

  it("keeps actual AIFS levels and six-hour cadence without fabricating layers", () => {
    const f = parseNativeForecast(
      nativeFixture("aifs", "2026-10-18T12:00:00"),
      "aifs",
    );
    const { data, source } = transformNativeForecast(
      f,
      Date.parse("2026-10-18T14:00:00Z"),
    );
    expect(data[0].cloud).toHaveLength(8);
    expect(data[0].durationMinutes).toBe(360);
    expect(source.cloudMethod).toBe("humidity-estimate");
    expect(data[0].cloud[0].dewPoint).toBeCloseTo(9.25, 1);
  });

  it("uses GFS native cloud fraction and derives dew point", () => {
    const f = parseNativeForecast(
      nativeFixture("gfs", "2026-10-18T12:00:00Z"),
      "gfs",
    );
    const { data, source } = transformNativeForecast(
      f,
      Date.parse("2026-10-18T12:10:00Z"),
    );
    expect(data[0].cloud[0].cloudCoverage).toBe(35);
    expect(data[0].cloud[0].dewPoint).toBeCloseTo(18.309, 2);
    expect(source.cloudMethod).toBe("model");
    expect(data[0].cloud[0].windDirection).toBe(270);
  });

  it.each([
    "bad-units",
    "wrong-wind-frame",
    "nonfinite",
    "shape",
    "duplicate",
    "wrong-product",
    "stale",
  ] as const)(
    "rejects %s instead of painting missing values as clear skies",
    (bad) => {
      const f = nativeFixture();
      if (bad === "bad-units") f.units.temperature = "kelvin";
      if (bad === "wrong-wind-frame") f.native_wind_frame = "earth-relative";
      if (bad === "nonfinite") (f.rows[0][1] as number[])[0] = NaN;
      if (bad === "shape") (f.rows[0][1] as number[]).pop();
      if (bad === "duplicate") f.available_leads[1] = 0;
      if (bad === "wrong-product") f.dataset = "another-product";
      if (bad === "stale") f.freshness.available = false;
      expect(() => parseNativeForecast(f, "hrrr")).toThrow();
    },
  );

  it("rejects past-only and aged forecasts so the server can fall back", () => {
    const f = nativeFixture();
    expect(() =>
      transformNativeForecast(f, Date.parse("2026-10-19T01:00:00Z")),
    ).toThrow("insufficient");
    expect(() =>
      transformNativeForecast(f, Date.parse("2026-10-20T01:00:00Z")),
    ).toThrow("stale");
    expect(nativeInitTime("2026-10-18T19:00:00")).toBe(
      Date.parse("2026-10-18T19:00:00Z"),
    );
  });
});
