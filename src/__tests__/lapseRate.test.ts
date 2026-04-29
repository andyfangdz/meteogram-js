import { describe, it, expect } from "vitest";
import {
  computeMALR,
  computeELR,
  computeThetaE,
  computeInstability,
  getInstabilityColor,
  getInstabilityLabel,
  getBuoyancyColor,
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
      const theta = (T + 273.15) * Math.pow(1000 / p, 287.05 / 1004);
      const thetaE = computeThetaE(T, T - 50, p);
      expect(thetaE).toBeCloseTo(theta, 0);
    });
  });

  describe("computeInstability", () => {
    it("is positive when θe decreases with altitude (conditional/potential instability)", () => {
      // Warm moist below, cooler drier above → dθe/dz < 0 → score > 0.
      const lower = {
        mslFt: 0,
        temperature: 25,
        dewPoint: 22,
        hpa: 1000,
      };
      const upper = {
        mslFt: 3280.84,
        temperature: 12,
        dewPoint: 0,
        hpa: 700,
      };
      const score = computeInstability(lower, upper);
      expect(score).not.toBeNull();
      expect(score!).toBeGreaterThan(0);
    });

    it("is negative for a strongly stable layer (e.g., inversion)", () => {
      const lower = {
        mslFt: 0,
        temperature: 5,
        dewPoint: 0,
        hpa: 1000,
      };
      const upper = {
        mslFt: 1640,
        temperature: 15,
        dewPoint: 5,
        hpa: 950,
      };
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
    it("returns null inside the neutral deadband", () => {
      expect(getInstabilityColor(0)).toBeNull();
      expect(getInstabilityColor(0.5)).toBeNull();
      expect(getInstabilityColor(-0.5)).toBeNull();
    });

    it("uses warm hues (yellow → red) for unstable scores", () => {
      // +2 K/km should be near yellow.
      const yellow = getInstabilityColor(2)!;
      expect(yellow).toMatch(/^rgba\(/);
      // +10 K/km should be near red.
      const red = getInstabilityColor(10)!;
      const rRed = parseInt(red.match(/rgba\((\d+),/)![1], 10);
      const rYellow = parseInt(yellow.match(/rgba\((\d+),/)![1], 10);
      expect(rRed).toBeGreaterThanOrEqual(rYellow); // red has bigger R component
    });

    it("uses green for stable scores with alpha growing in magnitude", () => {
      const mild = getInstabilityColor(-3)!;
      const strong = getInstabilityColor(-15)!;
      expect(mild).toMatch(/^rgba\(34, 197, 94/);
      expect(strong).toMatch(/^rgba\(34, 197, 94/);
      const mildAlpha = parseFloat(mild.match(/, ([\d.]+)\)$/)![1]);
      const strongAlpha = parseFloat(strong.match(/, ([\d.]+)\)$/)![1]);
      expect(strongAlpha).toBeGreaterThan(mildAlpha);
    });
  });

  describe("getInstabilityLabel", () => {
    it("labels by magnitude and sign", () => {
      expect(getInstabilityLabel(0)).toBe("Neutral");
      expect(getInstabilityLabel(2)).toBe("Unstable");
      expect(getInstabilityLabel(8)).toBe("Strongly unstable");
      expect(getInstabilityLabel(-2)).toBe("Stable");
      expect(getInstabilityLabel(-8)).toBe("Strongly stable");
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
