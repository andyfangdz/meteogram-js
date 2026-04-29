import { describe, it, expect } from "vitest";
import {
  computeMALR,
  computeELR,
  getStabilityCategory,
  getEffectiveStabilityCategory,
  getStabilityColor,
  getBuoyancyColor,
  isCellSaturated,
  cPerKmToCPerKft,
  DALR_C_PER_KM,
  ISA_C_PER_KM,
} from "@/utils/lapseRate";

describe("utils/lapseRate", () => {
  describe("computeMALR", () => {
    it("falls between 3 and 9.8 °C/km for typical tropospheric inputs", () => {
      // Warm/moist surface — small MALR (lots of latent heat release).
      expect(computeMALR(25, 1000)).toBeGreaterThan(3);
      expect(computeMALR(25, 1000)).toBeLessThan(7);
      // Cold/dry mid-troposphere — MALR approaches DALR.
      expect(computeMALR(-30, 400)).toBeGreaterThan(7);
      expect(computeMALR(-30, 400)).toBeLessThan(DALR_C_PER_KM);
    });

    it("decreases with warmer temperature at the same pressure", () => {
      // More water vapor → more latent heat → smaller MALR.
      expect(computeMALR(20, 1000)).toBeLessThan(computeMALR(0, 1000));
      expect(computeMALR(0, 1000)).toBeLessThan(computeMALR(-20, 1000));
    });

    it("clamps the saturation mixing ratio so p ≤ es is well-defined", () => {
      // Pathological input: vapor pressure exceeds total pressure.
      const result = computeMALR(40, 50);
      expect(Number.isFinite(result)).toBe(true);
      expect(result).toBeGreaterThan(0);
    });
  });

  describe("computeELR", () => {
    it("returns positive °C/km when the upper cell is colder", () => {
      // 1 km separation, 6.5 °C cooler upstairs → ISA-like 6.5 °C/km.
      const elr = computeELR(
        { mslFt: 0, temperature: 15 },
        { mslFt: 3280.84, temperature: 8.5 },
      );
      expect(elr).toBeCloseTo(ISA_C_PER_KM, 1);
    });

    it("returns null when the cells are at the same altitude or inverted", () => {
      expect(
        computeELR(
          { mslFt: 5000, temperature: 10 },
          { mslFt: 5000, temperature: 5 },
        ),
      ).toBeNull();
      expect(
        computeELR(
          { mslFt: 5000, temperature: 10 },
          { mslFt: 4000, temperature: 5 },
        ),
      ).toBeNull();
    });
  });

  describe("getStabilityCategory", () => {
    it("classifies by ELR vs MALR/DALR boundaries", () => {
      const malr = 6;
      expect(getStabilityCategory(11, malr)).toBe("absolutely-unstable");
      expect(getStabilityCategory(8, malr)).toBe("conditionally-unstable");
      expect(getStabilityCategory(4, malr)).toBe("absolutely-stable");
      expect(getStabilityCategory(DALR_C_PER_KM, malr)).toBe(
        "absolutely-unstable",
      );
      expect(getStabilityCategory(malr, malr)).toBe("absolutely-stable");
    });
  });

  describe("getStabilityColor", () => {
    it("uses the per-category default alpha when none is provided", () => {
      expect(getStabilityColor("absolutely-stable")).toMatch(
        /^rgba\(34, 197, 94, 0\.28\)$/,
      );
    });

    it("accepts an explicit alpha override", () => {
      expect(getStabilityColor("absolutely-unstable", 0.6)).toMatch(
        /rgba\(239, 68, 68, 0\.6\)/,
      );
    });
  });

  describe("getBuoyancyColor", () => {
    it("returns red for positive buoyancy and blue for negative", () => {
      expect(getBuoyancyColor(2)).toMatch(/^rgba\(239, 68, 68/);
      expect(getBuoyancyColor(-2)).toMatch(/^rgba\(59, 130, 246/);
    });

    it("returns null for buoyancy below the visibility threshold", () => {
      expect(getBuoyancyColor(0)).toBeNull();
      expect(getBuoyancyColor(0.1)).toBeNull();
    });

    it("clamps opacity for large magnitudes", () => {
      const strong = getBuoyancyColor(20)!;
      // Max alpha is 0.45 by design.
      expect(strong).toMatch(/0\.45\d*\)$/);
    });
  });

  describe("cPerKmToCPerKft", () => {
    it("converts 9.8 °C/km to ≈2.99 °C/kft", () => {
      expect(cPerKmToCPerKft(DALR_C_PER_KM)).toBeCloseTo(2.987, 2);
    });
  });

  describe("isCellSaturated", () => {
    it("returns true when cloud cover > 50%", () => {
      expect(
        isCellSaturated({ cloudCoverage: 75, temperature: 10, dewPoint: 0 }),
      ).toBe(true);
    });

    it("returns true when dewpoint depression < 1°C", () => {
      expect(
        isCellSaturated({ cloudCoverage: 0, temperature: 10, dewPoint: 9.5 }),
      ).toBe(true);
    });

    it("returns false when dry and clear", () => {
      expect(
        isCellSaturated({ cloudCoverage: 10, temperature: 10, dewPoint: 5 }),
      ).toBe(false);
    });
  });

  describe("getEffectiveStabilityCategory", () => {
    it("upgrades conditionally-unstable to absolutely-unstable when saturated", () => {
      // ELR=8 sits between MALR=6 and DALR=9.8 → conditionally unstable.
      expect(getEffectiveStabilityCategory(8, 6, false)).toBe(
        "conditionally-unstable",
      );
      expect(getEffectiveStabilityCategory(8, 6, true)).toBe(
        "absolutely-unstable",
      );
    });

    it("leaves stable and absolutely-unstable categories unchanged", () => {
      expect(getEffectiveStabilityCategory(4, 6, true)).toBe(
        "absolutely-stable",
      );
      expect(getEffectiveStabilityCategory(4, 6, false)).toBe(
        "absolutely-stable",
      );
      expect(getEffectiveStabilityCategory(11, 6, true)).toBe(
        "absolutely-unstable",
      );
    });
  });
});
