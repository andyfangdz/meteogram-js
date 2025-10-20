import { describe, it, expect, vi, beforeEach } from "vitest";
import { MODEL_CONFIGS, API_URL, FEET_PER_METER } from "@/config/weather";
import type { WeatherModel } from "@/types/weather";
import { createFakeOpenMeteoResponse } from "./__helpers__/fakeOpenMeteo";

const model: WeatherModel = "gfs_seamless";
const forecastKey = MODEL_CONFIGS[model].forecastDataKey;

const mockFetchWeatherApi = vi.fn(async () => {
  const fakeResponse = createFakeOpenMeteoResponse(
    forecastKey,
    MODEL_CONFIGS[model].hpaLevels,
    1,
  );
  return [fakeResponse as any];
});

vi.mock("openmeteo", () => ({
  fetchWeatherApi: mockFetchWeatherApi,
}));

describe("server actions", () => {
  beforeEach(() => {
    mockFetchWeatherApi.mockClear();
    // Mock global fetch for elevation API
    // @ts-expect-error Node fetch typing
    globalThis.fetch = vi.fn(async () => ({
      ok: true,
      json: async () => ({ elevation: [100] }),
    }));
  });

  it("fetchWeatherDataAction passes correct params to OpenMeteo", async () => {
    const { fetchWeatherDataAction } = await import("@/app/actions/weather");

    const responses = await fetchWeatherDataAction(model, "KFRG");
    expect(Array.isArray(responses)).toBe(true);
    expect(mockFetchWeatherApi).toHaveBeenCalledTimes(1);

    const [, params] = mockFetchWeatherApi.mock.calls[0];
    const modelConfig = MODEL_CONFIGS[model];

    expect(params.latitude).toBeDefined();
    expect(params.longitude).toBeDefined();
    expect(params.models).toBe(model);

    // Vars key should contain all variables joined
    const expectedVars = modelConfig.getAllVariables().join(",");
    expect(params[modelConfig.varsKey]).toBe(expectedVars);

    // Step key/size included
    if (modelConfig.stepKey) {
      expect(params[modelConfig.stepKey]).toBe(modelConfig.stepSize);
    }
  });

  it("getWeatherData integrates forecast + elevation and transforms data", async () => {
    const { getWeatherData } = await import("@/app/actions/weather");

    const { data, elevationFt, timestamp } = await getWeatherData(model, "KFRG");

    expect(typeof timestamp).toBe("string");
    expect(elevationFt).toBeCloseTo(100 * FEET_PER_METER, 3);
    expect(data.length).toBeGreaterThan(0);
    expect(data[0].cloud.length).toBeGreaterThan(0);
  });

  it("supports custom coordinate locations via Name@lat,long", async () => {
    const { getWeatherData } = await import("@/app/actions/weather");

    const result = await getWeatherData(model, "Custom Place@40.0,-73.9");
    expect(result.data.length).toBeGreaterThan(0);
  });
});
