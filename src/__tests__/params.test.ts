import { describe, it, expect } from "vitest";
import { parseVisualizationPreferences, serializeVisualizationPreferences } from "@/utils/params";
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
