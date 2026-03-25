import { describe, it, expect } from "vitest";
import { parseVisualizationPreferences, serializeVisualizationPreferences, parseRouteParams, serializeRouteParams } from "@/utils/params";
import { DEFAULT_PREFERENCES } from "@/config/preferences";

describe("utils/params", () => {
  it("parses defaults when no params provided", () => {
    const prefs = parseVisualizationPreferences({});
    expect(prefs).toEqual(DEFAULT_PREFERENCES);
  });

  it("parses boolean flags correctly", () => {
    const prefs = parseVisualizationPreferences({
      useLocalTime: "true",
      highlightCeiling: "false",
      clampCoverage: "false",
      showPressureLines: "true",
      showWindBarbs: "false",
      showIsothermLines: "true",
      showIsotachLines: "true",
    });

    expect(prefs.useLocalTime).toBe(true);
    expect(prefs.highlightCeilingCoverage).toBe(false);
    expect(prefs.clampCloudCoverageAt50Pct).toBe(false);
    expect(prefs.showPressureLines).toBe(true);
    expect(prefs.showWindBarbs).toBe(false);
    expect(prefs.showIsothermLines).toBe(true);
    expect(prefs.showIsotachLines).toBe(true);
  });

  it("serializes only differences from defaults", () => {
    const prefs = { ...DEFAULT_PREFERENCES, showWindBarbs: false, useLocalTime: true };
    const params = serializeVisualizationPreferences(prefs);

    const obj = Object.fromEntries(params.entries());
    expect(obj).toEqual({ showWindBarbs: "false", useLocalTime: "true" });
  });
});

describe("parseRouteParams", () => {
  it("parses all route params with defaults", () => {
    const result = parseRouteParams({});
    expect(result.cruiseAltitudeFt).toBe(6000);
    expect(result.tasKnots).toBe(120);
    expect(result.resolutionNM).toBe(25);
    expect(result.departureTime).toBeInstanceOf(Date);
  });

  it("parses provided route params", () => {
    const result = parseRouteParams({
      alt: "8000",
      tas: "150",
      res: "10",
      dep: "2026-03-24T14:00:00.000Z",
    });
    expect(result.cruiseAltitudeFt).toBe(8000);
    expect(result.tasKnots).toBe(150);
    expect(result.resolutionNM).toBe(10);
    expect(result.departureTime).toEqual(new Date("2026-03-24T14:00:00.000Z"));
  });

  it("clamps resolution to minimum of 5", () => {
    const result = parseRouteParams({ res: "2" });
    expect(result.resolutionNM).toBe(5);
  });
});

describe("serializeRouteParams", () => {
  it("serializes non-default route params", () => {
    const params = serializeRouteParams({
      cruiseAltitudeFt: 8000,
      tasKnots: 120,
      resolutionNM: 25,
      departureTime: new Date("2026-03-24T14:00:00.000Z"),
    });
    expect(params.get("alt")).toBe("8000");
    expect(params.has("tas")).toBe(false);
    expect(params.has("res")).toBe(false);
    expect(params.get("dep")).toBe("2026-03-24T14:00:00.000Z");
  });
});
