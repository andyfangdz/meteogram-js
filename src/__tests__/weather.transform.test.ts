import { describe, it, expect } from "vitest";
import { transformWeatherData } from "@/utils/weather";
import { MODEL_CONFIGS } from "@/config/weather";
import type { WeatherModel } from "@/types/weather";
import { createFakeOpenMeteoResponse, geopotentialToMslMeters } from "./__helpers__/fakeOpenMeteo";

function getModel(): WeatherModel { return "gfs_seamless"; }

describe("utils/weather.transformWeatherData", () => {
  it("creates CloudColumns with sorted cells and top/bottom bounds", () => {
    const model = getModel();
    const forecastKey = MODEL_CONFIGS[model].forecastDataKey;
    const fakeResponse = createFakeOpenMeteoResponse(
      forecastKey,
      MODEL_CONFIGS[model].hpaLevels,
      1,
    );

    const result = transformWeatherData(fakeResponse as any, model);
    expect(result).toHaveLength(1);
    const col = result[0];

    // Sorted descending by hpa: first element should be the highest pressure level
    expect(col.cloud[0].hpa).toBe(MODEL_CONFIGS[model].hpaLevels[0]);
    expect(col.cloud[1].hpa).toBe(MODEL_CONFIGS[model].hpaLevels[1]);
    expect(col.cloud[col.cloud.length - 1].hpa).toBe(
      MODEL_CONFIGS[model].hpaLevels[MODEL_CONFIGS[model].hpaLevels.length - 1],
    );

    // Validate groundTemp present
    expect(typeof col.groundTemp).toBe("number");

    // Validate msl bounds for first cell
    const first = col.cloud[0];
    const second = col.cloud[1];
    const last = col.cloud[col.cloud.length - 1];

    const expectedMslFirst = geopotentialToMslMeters(100) * 3.28084; // helper builds geopotential = 100 + index*100
    const expectedMslSecond = geopotentialToMslMeters(200) * 3.28084;
    expect(first.mslFt).toBeCloseTo(expectedMslFirst, 1);
    expect(second.mslFt).toBeCloseTo(expectedMslSecond, 1);

    // Bounds: first has no prev => bottom ~ msl-500, top ~ avg(first,second)
    expect(first.mslFtBottom).toBeCloseTo(first.mslFt - 500, 5);
    expect(first.mslFtTop).toBeCloseTo((first.mslFt + second.mslFt) / 2, 5);

    // Last has no next => top ~ msl+500, bottom ~ avg(prev,last)
    const prevOfLast = col.cloud[col.cloud.length - 2];
    expect(last.mslFtBottom).toBeCloseTo(
      (prevOfLast.mslFt + last.mslFt) / 2,
      5,
    );
    expect(last.mslFtTop).toBeCloseTo(last.mslFt + 500, 5);
  });

  it("filters out invalid cells with non-finite values", () => {
    const model = getModel();
    const forecastKey = MODEL_CONFIGS[model].forecastDataKey;
    const fakeResponse = createFakeOpenMeteoResponse(
      forecastKey,
      MODEL_CONFIGS[model].hpaLevels,
      1,
    ) as any;

    // Corrupt a variable to produce NaN in one cell's windSpeed
    const forecast = fakeResponse[forecastKey]();
    const len = MODEL_CONFIGS[model].hpaLevels.length;
    const windSpeedBaseIndex = 3 * len; // matches transform indexing
    // Make wind speed for second HPA level NaN by overriding variables() handler
    const origVariables = forecast.variables;
    (forecast as any).variables = (index: number) => {
      if (index === windSpeedBaseIndex + 1) {
        return { values: () => NaN };
      }
      return origVariables(index);
    };

    const result = transformWeatherData(fakeResponse, model);
    expect(result).toHaveLength(1);
    const col = result[0];

    // One invalid cell removed from full set
    expect(col.cloud).toHaveLength(len - 1);
  });
});
