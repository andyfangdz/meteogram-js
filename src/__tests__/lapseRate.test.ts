import { describe, it, expect } from "vitest";
import {
  computeMALR,
  computeELR,
  computeTheta,
  computeThetaE,
  computeInstability,
  getInstabilityColor,
  getInstabilityLabel,
  getBuoyancyColor,
  isCellSaturated,
  cPerKmToCPerKft,
  DALR_C_PER_KM,
  ISA_C_PER_KM,
} from "@/utils/lapseRate";

describe("utils/lapseRate", () => {
  describe("computeMALR", () => {
    it("falls between 3 and 9.8 °C/km for typical tropospheric inputs", () => {
      expect(computeMALR(25, 1000)).toBeGreaterThan(3);
      expect(computeMALR(25, 1000)).toBeLessThan(7);
      expect(computeMALR(-30, 400)).toBeGreaterThan(7);
      expect(computeMALR(-30, 400)).toBeLessThan(DALR_C_PER_KM);
    });

    it("decreases with warmer temperature at the same pressure", () => {
      expect(computeMALR(20, 1000)).toBeLessThan(computeMALR(0, 1000));
      expect(computeMALR(0, 1000)).toBeLessThan(computeMALR(-20, 1000));
    });

    it("clamps the saturation mixing ratio so p ≤ es is well-defined", () => {
      const result = computeMALR(40, 50);
      expect(Number.isFinite(result)).toBe(true);
      expect(result).toBeGreaterThan(0);
    });
  });

  describe("computeELR", () => {
    it("returns positive °C/km when the upper cell is colder", () => {
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

  describe("computeTheta", () => {
    it("equals T in K at 1000 hPa (no compression)", () => {
      expect(computeTheta(0, 1000)).toBeCloseTo(273.15, 5);
      expect(computeTheta(15, 1000)).toBeCloseTo(288.15, 5);
    });

    it("is greater than T (in K) at lower pressures (parcel warms when compressed)", () => {
      const T = 0; // °C
      expect(computeTheta(T, 500)).toBeGreaterThan(T + 273.15);
    });
  });

  describe("computeThetaE", () => {
    it("rises with surface temperature and dewpoint", () => {
      const cool = computeThetaE(15, 10, 1000);
      const warm = computeThetaE(25, 20, 1000);
      expect(warm).toBeGreaterThan(cool);
    });

    it("matches potential temperature in the dry limit", () => {
      // Very dry parcel (Td far below T) → tiny mixing ratio → θe ≈ θ.
      const T = 15;
      const p = 1000;
      const theta = computeTheta(T, p);
      const thetaE = computeThetaE(T, T - 50, p);
      expect(thetaE).toBeCloseTo(theta, 0);
    });

    it("stays finite at and beyond saturation (Td == T and Td > T)", () => {
      // Saturated: Td equals T.
      const sat = computeThetaE(20, 20, 1000);
      expect(Number.isFinite(sat)).toBe(true);
      expect(sat).toBeGreaterThan(0);
      // Pathological supersaturation (e.g., API noise) — must still be finite.
      const supersat = computeThetaE(20, 25, 1000);
      expect(Number.isFinite(supersat)).toBe(true);
      expect(supersat).toBeGreaterThan(0);
    });
  });

  describe("isCellSaturated", () => {
    it("flags cells with > 50% cloud cover", () => {
      expect(
        isCellSaturated({ cloudCoverage: 75, temperature: 10, dewPoint: 0 }),
      ).toBe(true);
    });

    it("flags cells where T - Td < 1°C", () => {
      expect(
        isCellSaturated({ cloudCoverage: 0, temperature: 10, dewPoint: 9.5 }),
      ).toBe(true);
    });

    it("does not flag dry, clear cells", () => {
      expect(
        isCellSaturated({ cloudCoverage: 10, temperature: 10, dewPoint: 5 }),
      ).toBe(false);
    });
  });

  describe("computeInstability", () => {
    it("is positive in a saturated convective layer (warm/moist below, cold/dry above)", () => {
      const lower = { mslFt: 0, temperature: 25, dewPoint: 24, hpa: 1000 };
      const upper = { mslFt: 3280.84, temperature: 12, dewPoint: 0, hpa: 700 };
      const score = computeInstability(lower, upper);
      expect(score).not.toBeNull();
      expect(score!).toBeGreaterThan(0);
    });

    it("is positive for an unsaturated layer with conditional instability (moist below, drier aloft)", () => {
      // ELR small but moisture stratification negative → -dθe/dz > 0.
      // Lower is unsaturated (4°C depression), so this is "conditional"
      // not realized — render layer is responsible for the visual distinction.
      const lower = { mslFt: 452, temperature: 9.4, dewPoint: 5.4, hpa: 1000 };
      const upper = { mslFt: 1500, temperature: 9.4, dewPoint: -10, hpa: 975 };
      const score = computeInstability(lower, upper);
      expect(score).not.toBeNull();
      expect(score!).toBeGreaterThan(0);
    });

    it("is negative for a strongly stable inversion", () => {
      const lower = { mslFt: 0, temperature: 5, dewPoint: 0, hpa: 1000 };
      const upper = { mslFt: 1640, temperature: 15, dewPoint: 5, hpa: 950 };
      const score = computeInstability(lower, upper);
      expect(score).not.toBeNull();
      expect(score!).toBeLessThan(0);
    });

    it("returns null when the cells are at the same altitude", () => {
      expect(
        computeInstability(
          { mslFt: 5000, temperature: 10, dewPoint: 5, hpa: 850 },
          { mslFt: 5000, temperature: 10, dewPoint: 5, hpa: 850 },
        ),
      ).toBeNull();
    });
  });

  describe("getInstabilityColor", () => {
    it("returns null inside the neutral deadband regardless of saturation", () => {
      expect(getInstabilityColor(0, true)).toBeNull();
      expect(getInstabilityColor(0.5, false)).toBeNull();
      expect(getInstabilityColor(-0.5, true)).toBeNull();
    });

    it("uses yellow→red ramp for saturated unstable layers", () => {
      const mild = getInstabilityColor(2, true)!;
      const strong = getInstabilityColor(10, true)!;
      const mildR = parseInt(mild.match(/rgba\((\d+),/)![1], 10);
      const strongR = parseInt(strong.match(/rgba\((\d+),/)![1], 10);
      expect(strongR).toBeGreaterThanOrEqual(mildR); // red has bigger R
    });

    it("stays yellow for unsaturated unstable layers (conditional only)", () => {
      // Conditional case keeps the yellow base color regardless of magnitude
      // — magnitude shows up in alpha, not hue.
      const mild = getInstabilityColor(2, false)!;
      const strong = getInstabilityColor(10, false)!;
      expect(mild).toMatch(/^rgba\(234, 179, 8/);
      expect(strong).toMatch(/^rgba\(234, 179, 8/);
    });

    it("uses green for stable scores regardless of saturation", () => {
      const sat = getInstabilityColor(-5, true)!;
      const unsat = getInstabilityColor(-5, false)!;
      expect(sat).toMatch(/^rgba\(34, 197, 94/);
      expect(unsat).toMatch(/^rgba\(34, 197, 94/);
    });
  });

  describe("getInstabilityLabel", () => {
    it("distinguishes realized from conditional instability via the saturated flag", () => {
      expect(getInstabilityLabel(2, true)).toBe("Unstable");
      expect(getInstabilityLabel(2, false)).toBe("Conditionally unstable");
      expect(getInstabilityLabel(8, true)).toBe("Strongly unstable");
      expect(getInstabilityLabel(8, false)).toBe("Strongly conditional");
    });

    it("labels stable scores the same regardless of saturation", () => {
      expect(getInstabilityLabel(-2, true)).toBe("Stable");
      expect(getInstabilityLabel(-2, false)).toBe("Stable");
      expect(getInstabilityLabel(-8, true)).toBe("Strongly stable");
      expect(getInstabilityLabel(0, true)).toBe("Neutral");
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
      expect(strong).toMatch(/0\.45\d*\)$/);
    });
  });

  describe("cPerKmToCPerKft", () => {
    it("converts 9.8 °C/km to ≈2.99 °C/kft", () => {
      expect(cPerKmToCPerKft(DALR_C_PER_KM)).toBeCloseTo(2.987, 2);
    });
  });
});
