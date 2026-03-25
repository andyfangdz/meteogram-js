import { describe, it, expect } from "vitest";
import { haversineDistanceNM, forwardBearing, interpolateGreatCircle, parseWaypointString, generateRouteSamplePoints, tailwindComponent, computeTimings, closestColumnByTime } from "@/utils/route";
import type { CloudColumn, RouteWaypoint } from "@/types/weather";

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

describe("parseWaypointString", () => {
  it("parses dash-separated airport codes", () => {
    const result = parseWaypointString("KCDW-KFRG");
    expect(result).toEqual([
      { name: "KCDW", identifier: "KCDW" },
      { name: "KFRG", identifier: "KFRG" },
    ]);
  });

  it("parses custom coordinate waypoints", () => {
    const result = parseWaypointString("KCDW-MySpot@41.5,-73.0-KFRG");
    expect(result).toHaveLength(3);
    expect(result[1]).toEqual({ name: "MySpot", identifier: "MySpot@41.5,-73.0" });
  });

  it("throws for fewer than 2 waypoints", () => {
    expect(() => parseWaypointString("KCDW")).toThrow("at least 2 waypoints");
  });
});

describe("generateRouteSamplePoints", () => {
  it("generates points for a multi-waypoint route", () => {
    const waypoints = [
      { name: "A", latitude: 40.87, longitude: -74.28 },
      { name: "B", latitude: 41.5, longitude: -73.0 },
      { name: "C", latitude: 40.73, longitude: -73.42 },
    ];
    const points = generateRouteSamplePoints(waypoints, 25);
    const userPoints = points.filter((p) => p.isUserDefined);
    expect(userPoints).toHaveLength(3);
    expect(userPoints[0].name).toBe("A");
    expect(userPoints[1].name).toBe("B");
    expect(userPoints[2].name).toBe("C");
    for (let i = 1; i < points.length; i++) {
      expect(points[i].distanceNM).toBeGreaterThan(points[i - 1].distanceNM);
    }
  });

  it("caps total points at maxPoints", () => {
    const waypoints = [
      { name: "A", latitude: 40.0, longitude: -74.0 },
      { name: "B", latitude: 50.0, longitude: -60.0 },
    ];
    const result = generateRouteSamplePoints(waypoints, 5, 20);
    expect(result.length).toBeLessThanOrEqual(20);
  });
});

describe("tailwindComponent", () => {
  it("returns negative (headwind) when wind is from ahead", () => {
    const result = tailwindComponent(20, 0, 0);
    expect(result).toBeCloseTo(-20 * 0.539957, 1);
  });

  it("returns positive (tailwind) when wind is from behind", () => {
    const result = tailwindComponent(20, 180, 0);
    expect(result).toBeCloseTo(20 * 0.539957, 1);
  });

  it("returns ~0 for pure crosswind", () => {
    const result = tailwindComponent(20, 90, 0);
    expect(result).toBeCloseTo(0, 1);
  });
});

describe("computeTimings", () => {
  it("computes time-over based on TAS with calm wind", () => {
    const departureTime = new Date("2026-03-24T14:00:00Z");
    const waypoints: RouteWaypoint[] = [
      { name: "A", latitude: 40, longitude: -74, distanceNM: 0, isUserDefined: true },
      { name: "B", latitude: 40.5, longitude: -73.5, distanceNM: 30, isUserDefined: false },
      { name: "C", latitude: 41, longitude: -73, distanceNM: 60, isUserDefined: true },
    ];

    const makeWeatherData = (_index: number): CloudColumn[] => [{
      date: new Date("2026-03-24T14:00:00Z"),
      groundTemp: 15,
      cloud: [{
        hpa: 850, mslFt: 5000, geopotentialFt: 5000,
        cloudCoverage: 50, mslFtBottom: 4500, mslFtTop: 5500,
        temperature: 5, dewPoint: 2,
        windSpeed: 0, windDirection: 0,
      }],
    }];

    const result = computeTimings(waypoints, makeWeatherData, 6000, 120, departureTime);
    expect(result[0].estimatedTimeOver).toEqual(departureTime);
    // 30 NM at 120 kts = 15 min
    const expected15min = new Date(departureTime.getTime() + 15 * 60 * 1000);
    expect(result[1].estimatedTimeOver.getTime()).toBeCloseTo(expected15min.getTime(), -3);
    // 60 NM at 120 kts = 30 min
    const expected30min = new Date(departureTime.getTime() + 30 * 60 * 1000);
    expect(result[2].estimatedTimeOver.getTime()).toBeCloseTo(expected30min.getTime(), -3);
  });
});
