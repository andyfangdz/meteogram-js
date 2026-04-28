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

  it("round-trips showStabilityTint", () => {
    const parsed = parseVisualizationPreferences({ showStabilityTint: "true" });
    expect(parsed.showStabilityTint).toBe(true);

    const serialized = serializeVisualizationPreferences({
      ...DEFAULT_PREFERENCES,
      showStabilityTint: true,
    });
    expect(Object.fromEntries(serialized.entries())).toEqual({
      showStabilityTint: "true",
    });
  });

  it("round-trips showCondensationLevels and showParcelBuoyancy", () => {
    const parsed = parseVisualizationPreferences({
      showCondensationLevels: "true",
      showParcelBuoyancy: "true",
    });
    expect(parsed.showCondensationLevels).toBe(true);
    expect(parsed.showParcelBuoyancy).toBe(true);

    const serialized = serializeVisualizationPreferences({
      ...DEFAULT_PREFERENCES,
      showCondensationLevels: true,
      showParcelBuoyancy: true,
    });
    expect(Object.fromEntries(serialized.entries())).toEqual({
      showCondensationLevels: "true",
      showParcelBuoyancy: "true",
    });
  });

  it("round-trips parcelMode and rejects unknown values", () => {
    expect(parseVisualizationPreferences({ parcelMode: "mixed-100" }).parcelMode).toBe(
      "mixed-100",
    );
    expect(
      parseVisualizationPreferences({ parcelMode: "most-unstable" }).parcelMode,
    ).toBe("most-unstable");
    // Unknown value falls back to the default rather than passing through.
    expect(parseVisualizationPreferences({ parcelMode: "bogus" }).parcelMode).toBe(
      DEFAULT_PREFERENCES.parcelMode,
    );

    // Default is omitted from the serialized URL.
    expect(
      Object.fromEntries(
        serializeVisualizationPreferences({
          ...DEFAULT_PREFERENCES,
          parcelMode: "surface",
        }).entries(),
      ),
    ).toEqual({});
    expect(
      Object.fromEntries(
        serializeVisualizationPreferences({
          ...DEFAULT_PREFERENCES,
          parcelMode: "most-unstable",
        }).entries(),
      ),
    ).toEqual({ parcelMode: "most-unstable" });
  });
});
