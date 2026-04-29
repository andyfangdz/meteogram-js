import { describe, it, expect } from "vitest";
import {
  computeParcelProfile,
  computeColumnParcelProfile,
} from "@/utils/condensation";
import { CloudCell, CloudColumn } from "@/types/weather";

function cell(overrides: Partial<CloudCell>): CloudCell {
  return {
    hpa: 1000,
    mslFt: 0,
    geopotentialFt: 0,
    cloudCoverage: 0,
    mslFtBottom: -500,
    mslFtTop: 500,
    temperature: 15,
    dewPoint: 10,
    windSpeed: 0,
    windDirection: 0,
    lapseRateAboveCPerKm: null,
    malrCPerKm: 6,
    instabilityKPerKm: null,
    ...overrides,
  };
}

// Builds a column of cells from an explicit (hpa, mslFt, temp, dew) list,
// sorted ascending altitude (descending hpa) to match transformWeatherData.
function buildColumn(
  surfaceTempC: number,
  surfaceDewPointC: number,
  cells: CloudCell[],
): CloudColumn {
  return {
    date: new Date(),
    cloud: cells,
    groundTemp: surfaceTempC,
    groundDewPoint: surfaceDewPointC,
  };
}

describe("utils/condensation", () => {
  describe("computeParcelProfile (Espy LCL + parcel walk)", () => {
    it("places LCL roughly 125 m above the surface per °C of depression", () => {
      // 4 °C depression → ~500 m AGL ≈ 1640 ft AGL.
      const surfaceMslFt = 1000;
      const result = computeParcelProfile({
        surfaceTempC: 20,
        surfaceDewPointC: 16,
        surfaceMslFt,
        cells: [
          cell({ hpa: 1000, mslFt: 1000, temperature: 20, dewPoint: 16 }),
          cell({ hpa: 950, mslFt: 2640, temperature: 17, dewPoint: 12 }),
          cell({ hpa: 900, mslFt: 4500, temperature: 14, dewPoint: 8 }),
        ],
      });
      expect(result.lclMslFt).toBeCloseTo(1000 + 4 * 125 * 3.28084, 0);
    });

    it("returns nulls when surface is supersaturated (Td > T)", () => {
      const result = computeParcelProfile({
        surfaceTempC: 5,
        surfaceDewPointC: 7,
        surfaceMslFt: 0,
        cells: [cell({ hpa: 1000, mslFt: 0, temperature: 5, dewPoint: 7 })],
      });
      expect(result.lclMslFt).toBeNull();
      expect(result.lfcMslFt).toBeNull();
    });

    it("yields no LFC for an absolutely stable column", () => {
      // Environment cools at 4 °C/km — slower than MALR — so a saturated
      // parcel is always colder than env above LCL.
      const cells: CloudCell[] = [];
      for (let i = 0; i < 10; i++) {
        const z = i * 1000; // ft
        cells.push(
          cell({
            hpa: 1000 - i * 25,
            mslFt: z,
            temperature: 25 - (i * 1000 * 4) / 3280.84, // 4 °C/km
            dewPoint: 5 - (i * 1000 * 4) / 3280.84,
          }),
        );
      }
      const result = computeParcelProfile({
        surfaceTempC: 25,
        surfaceDewPointC: 5,
        surfaceMslFt: 0,
        cells,
      });
      expect(result.lfcMslFt).toBeNull();
    });

    it("populates a parcelTempC value for every cell above the surface", () => {
      const cells = [
        cell({ hpa: 1000, mslFt: 0, temperature: 20, dewPoint: 16 }),
        cell({ hpa: 900, mslFt: 3000, temperature: 12, dewPoint: 8 }),
        cell({ hpa: 800, mslFt: 6500, temperature: 4, dewPoint: 0 }),
      ];
      const result = computeParcelProfile({
        surfaceTempC: 20,
        surfaceDewPointC: 16,
        surfaceMslFt: 0,
        cells,
      });
      expect(result.parcelTempC).toHaveLength(3);
      result.parcelTempC.forEach((t) => {
        expect(t).not.toBeNull();
        expect(Number.isFinite(t as number)).toBe(true);
      });
    });
  });

  describe("computeColumnParcelProfile parcel modes", () => {
    function makeColumn(): CloudColumn {
      // Low levels are warm and moist, mid-troposphere is colder and drier.
      // Surface depression = 4 °C; parcel should rise.
      const cells: CloudCell[] = [
        cell({ hpa: 1000, mslFt: 0, temperature: 24, dewPoint: 20 }),
        cell({ hpa: 975, mslFt: 800, temperature: 22, dewPoint: 18 }),
        cell({ hpa: 950, mslFt: 1600, temperature: 20, dewPoint: 16 }),
        cell({ hpa: 925, mslFt: 2400, temperature: 18, dewPoint: 14 }),
        cell({ hpa: 900, mslFt: 3300, temperature: 16, dewPoint: 12 }),
        cell({ hpa: 850, mslFt: 5000, temperature: 11, dewPoint: 5 }),
        cell({ hpa: 800, mslFt: 6800, temperature: 6, dewPoint: -2 }),
        cell({ hpa: 700, mslFt: 10500, temperature: -4, dewPoint: -15 }),
      ];
      return buildColumn(24, 20, cells);
    }

    it("surface mode lifts a parcel from the ground", () => {
      const result = computeColumnParcelProfile(makeColumn(), 0, "surface");
      expect(result.lclMslFt).not.toBeNull();
      expect(result.parcelTempC[0]).not.toBeNull();
    });

    it("mixed-100 mode produces a different parcel than surface", () => {
      const col = makeColumn();
      const surface = computeColumnParcelProfile(col, 0, "surface");
      const ml = computeColumnParcelProfile(col, 0, "mixed-100");
      expect(ml.lclMslFt).not.toBeNull();
      // The mixed-layer parcel uses θ averaged over the lowest 100 hPa, so
      // its surface temperature differs from the raw surface measurement.
      expect(ml.lclMslFt).not.toBeCloseTo(surface.lclMslFt as number, 0);
    });

    it("most-unstable mode picks a parcel with at least as much CAPE as surface", () => {
      const col = makeColumn();
      const surface = computeColumnParcelProfile(col, 0, "surface");
      const mu = computeColumnParcelProfile(col, 0, "most-unstable");

      const capeProxy = (
        profile: { parcelTempC: (number | null)[] },
        cells: CloudCell[],
      ) => {
        let sum = 0;
        for (let i = 0; i < cells.length; i++) {
          const pt = profile.parcelTempC[i];
          if (pt == null) continue;
          const b = pt - cells[i].temperature;
          if (b > 0) sum += b;
        }
        return sum;
      };
      expect(capeProxy(mu, col.cloud)).toBeGreaterThanOrEqual(
        capeProxy(surface, col.cloud),
      );
    });
  });
});
