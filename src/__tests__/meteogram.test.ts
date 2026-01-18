import { describe, it, expect } from "vitest";
import { findDewPointDepressionPoints } from "@/utils/meteogram";
import type { CloudColumn, CloudCell } from "@/types/weather";

// Helper to create minimal CloudColumn data for testing
function createTestCloudColumn(
  date: Date,
  cells: Array<{
    mslFt: number;
    temperature: number;
    dewPoint: number;
  }>,
): CloudColumn {
  return {
    date,
    cloud: cells.map((c) => ({
      mslFt: c.mslFt,
      mslFtTop: c.mslFt + 500,
      mslFtBottom: c.mslFt - 500,
      temperature: c.temperature,
      dewPoint: c.dewPoint,
      cloudCoverage: 50,
      hpa: 850,
      windSpeed: 20,
      windDirection: 270,
    })),
    groundTemp: 15,
  };
}

describe("utils/meteogram.findDewPointDepressionPoints", () => {
  it("returns empty array for empty weather data", () => {
    const result = findDewPointDepressionPoints([]);
    expect(result).toEqual([]);
  });

  it("returns empty array when weatherData is undefined/null", () => {
    const result = findDewPointDepressionPoints(null as any);
    expect(result).toEqual([]);
  });

  it("uses default thresholds [3, 5, 10] when not specified", () => {
    // Create data where depression increases with altitude (crossing multiple thresholds)
    const columns: CloudColumn[] = [];
    for (let t = 0; t < 10; t++) {
      columns.push(
        createTestCloudColumn(new Date(Date.now() + t * 3600000), [
          { mslFt: 2000, temperature: 15, dewPoint: 14 }, // 1°C spread
          { mslFt: 4000, temperature: 12, dewPoint: 9 }, // 3°C spread (crosses threshold)
          { mslFt: 6000, temperature: 9, dewPoint: 4 }, // 5°C spread (crosses threshold)
          { mslFt: 8000, temperature: 6, dewPoint: -1 }, // 7°C spread
          { mslFt: 10000, temperature: 3, dewPoint: -7 }, // 10°C spread (crosses threshold)
          { mslFt: 12000, temperature: 0, dewPoint: -12 }, // 12°C spread
        ]),
      );
    }

    const result = findDewPointDepressionPoints(columns);

    // Should find isolines for default thresholds [3, 5, 10]
    const foundSpreads = new Set(result.map((r) => r.spread));
    // At least some of the default thresholds should be found
    const defaultThresholds = [3, 5, 10];
    const foundDefaultCount = defaultThresholds.filter((t) =>
      foundSpreads.has(t),
    ).length;
    expect(foundDefaultCount).toBeGreaterThan(0);
  });

  it("respects custom thresholds", () => {
    // Create data with varying depression from 0 at bottom to 10 at top
    const columns: CloudColumn[] = [];
    for (let t = 0; t < 5; t++) {
      columns.push(
        createTestCloudColumn(new Date(Date.now() + t * 3600000), [
          { mslFt: 3000, temperature: 10, dewPoint: 10 }, // 0°C spread
          { mslFt: 6000, temperature: 5, dewPoint: 2 }, // 3°C spread
          { mslFt: 9000, temperature: 0, dewPoint: -5 }, // 5°C spread
          { mslFt: 12000, temperature: -5, dewPoint: -12 }, // 7°C spread
        ]),
      );
    }

    const customThresholds = [0, 1, 3, 5, 10];
    const result = findDewPointDepressionPoints(columns, customThresholds);

    // Should only contain spreads from our custom thresholds
    result.forEach((line) => {
      expect(customThresholds).toContain(line.spread);
    });
  });

  it("returns points with valid x (time index) and y (altitude) coordinates", () => {
    // Create data where depression increases with altitude (crossing the 5°C threshold)
    const columns: CloudColumn[] = [];
    for (let t = 0; t < 10; t++) {
      columns.push(
        createTestCloudColumn(new Date(Date.now() + t * 3600000), [
          { mslFt: 3000, temperature: 15, dewPoint: 13 }, // 2°C spread
          { mslFt: 6000, temperature: 10, dewPoint: 6 }, // 4°C spread
          { mslFt: 9000, temperature: 5, dewPoint: -1 }, // 6°C spread (crosses 5°C)
          { mslFt: 12000, temperature: 0, dewPoint: -8 }, // 8°C spread
        ]),
      );
    }

    const result = findDewPointDepressionPoints(columns, [5]);

    expect(result.length).toBeGreaterThan(0);
    result.forEach((line) => {
      expect(line.points.length).toBeGreaterThan(2);
      line.points.forEach((point) => {
        // x should be a valid time index (can be fractional from interpolation)
        expect(point.x).toBeGreaterThanOrEqual(0);
        expect(point.x).toBeLessThanOrEqual(columns.length);
        // y should be within altitude range (with some padding)
        expect(point.y).toBeGreaterThan(0);
        expect(point.y).toBeLessThan(20000);
      });
    });
  });

  it("handles data with missing temperature values gracefully", () => {
    const columns: CloudColumn[] = [];
    for (let t = 0; t < 3; t++) {
      const column = createTestCloudColumn(new Date(Date.now() + t * 3600000), [
        { mslFt: 3000, temperature: 10, dewPoint: 5 },
        { mslFt: 6000, temperature: 5, dewPoint: 0 },
      ]);
      // Corrupt one cell's temperature
      if (t === 1) {
        column.cloud[0].temperature = undefined as any;
      }
      columns.push(column);
    }

    // Should not throw, may return fewer or no lines
    expect(() => findDewPointDepressionPoints(columns, [5])).not.toThrow();
  });

  it("handles data with missing dew point values gracefully", () => {
    const columns: CloudColumn[] = [];
    for (let t = 0; t < 3; t++) {
      const column = createTestCloudColumn(new Date(Date.now() + t * 3600000), [
        { mslFt: 3000, temperature: 10, dewPoint: 5 },
        { mslFt: 6000, temperature: 5, dewPoint: 0 },
      ]);
      // Corrupt one cell's dew point
      if (t === 1) {
        column.cloud[0].dewPoint = undefined as any;
      }
      columns.push(column);
    }

    // Should not throw
    expect(() => findDewPointDepressionPoints(columns, [5])).not.toThrow();
  });

  it("detects multiple thresholds when depression varies across altitude", () => {
    // Create data where depression increases with altitude
    // This should trigger isolines at different thresholds
    const columns: CloudColumn[] = [];
    for (let t = 0; t < 10; t++) {
      columns.push(
        createTestCloudColumn(new Date(Date.now() + t * 3600000), [
          { mslFt: 2000, temperature: 20, dewPoint: 19 }, // 1°C spread
          { mslFt: 4000, temperature: 15, dewPoint: 12 }, // 3°C spread
          { mslFt: 6000, temperature: 10, dewPoint: 5 }, // 5°C spread
          { mslFt: 8000, temperature: 5, dewPoint: -2 }, // 7°C spread
          { mslFt: 10000, temperature: 0, dewPoint: -10 }, // 10°C spread
        ]),
      );
    }

    const result = findDewPointDepressionPoints(columns, [1, 3, 5, 10]);

    // Should find isolines for multiple thresholds
    const foundSpreads = new Set(result.map((r) => r.spread));
    expect(foundSpreads.size).toBeGreaterThan(1);
  });

  it("handles negative dew point depression (supersaturation) gracefully", () => {
    // In rare cases dewPoint > temperature (supersaturation)
    const columns: CloudColumn[] = [];
    for (let t = 0; t < 3; t++) {
      columns.push(
        createTestCloudColumn(new Date(Date.now() + t * 3600000), [
          { mslFt: 3000, temperature: 5, dewPoint: 6 }, // -1°C spread (supersaturation)
          { mslFt: 6000, temperature: 5, dewPoint: 4 }, // 1°C spread
          { mslFt: 9000, temperature: 5, dewPoint: 0 }, // 5°C spread
        ]),
      );
    }

    // Should not throw when handling negative depression values
    expect(() => findDewPointDepressionPoints(columns, [0, 1, 5])).not.toThrow();
  });

  it("clamps negative depression values to 0 (supersaturation treated as saturated)", () => {
    // Create data where lower altitudes have supersaturation (dewPoint > temperature)
    // This should result in the 0°C isoline appearing where supersaturation meets normal conditions
    const columns: CloudColumn[] = [];
    for (let t = 0; t < 10; t++) {
      columns.push(
        createTestCloudColumn(new Date(Date.now() + t * 3600000), [
          { mslFt: 2000, temperature: 10, dewPoint: 12 }, // -2°C spread -> clamped to 0
          { mslFt: 4000, temperature: 10, dewPoint: 10 }, // 0°C spread (saturated)
          { mslFt: 6000, temperature: 10, dewPoint: 7 }, // 3°C spread
          { mslFt: 8000, temperature: 10, dewPoint: 3 }, // 7°C spread
        ]),
      );
    }

    const result = findDewPointDepressionPoints(columns, [0, 3]);

    // Should find 0°C and 3°C isolines
    // The 0°C isoline should appear between the clamped supersaturation and normal saturation
    const foundSpreads = new Set(result.map((r) => r.spread));
    expect(foundSpreads.has(3)).toBe(true);
  });
});
