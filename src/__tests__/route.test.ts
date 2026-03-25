import { describe, it, expect } from "vitest";
import { haversineDistanceNM, forwardBearing, interpolateGreatCircle } from "@/utils/route";

describe("haversineDistanceNM", () => {
  it("returns 0 for same point", () => {
    expect(haversineDistanceNM(40, -74, 40, -74)).toBeCloseTo(0, 1);
  });

  it("calculates KCDW to KFRG (~50 NM)", () => {
    const dist = haversineDistanceNM(40.8727, -74.2838, 40.7344, -73.4164);
    expect(dist).toBeGreaterThan(40);
    expect(dist).toBeLessThan(60);
  });
});

describe("forwardBearing", () => {
  it("returns ~0 for due north", () => {
    const bearing = forwardBearing(40, -74, 41, -74);
    expect(bearing).toBeCloseTo(0, 0);
  });

  it("returns ~90 for due east", () => {
    const bearing = forwardBearing(40, -74, 40, -73);
    expect(bearing).toBeGreaterThan(85);
    expect(bearing).toBeLessThan(95);
  });
});

describe("interpolateGreatCircle", () => {
  it("returns start and end for short distance", () => {
    const points = interpolateGreatCircle(
      { lat: 40, lon: -74, name: "A" },
      { lat: 40.01, lon: -74, name: "B" },
      100,
    );
    expect(points).toHaveLength(2);
    expect(points[0].isUserDefined).toBe(true);
    expect(points[1].isUserDefined).toBe(true);
  });

  it("generates intermediate points at resolution intervals", () => {
    const points = interpolateGreatCircle(
      { lat: 40.8727, lon: -74.2838, name: "KCDW" },
      { lat: 40.7344, lon: -73.4164, name: "KFRG" },
      10,
    );
    expect(points.length).toBeGreaterThanOrEqual(4);
    expect(points.length).toBeLessThanOrEqual(8);
    expect(points[0].name).toBe("KCDW");
    expect(points[0].isUserDefined).toBe(true);
    expect(points[points.length - 1].name).toBe("KFRG");
    expect(points[points.length - 1].isUserDefined).toBe(true);
    expect(points[1].isUserDefined).toBe(false);
    for (let i = 1; i < points.length; i++) {
      expect(points[i].distanceNM).toBeGreaterThan(points[i - 1].distanceNM);
    }
  });
});
