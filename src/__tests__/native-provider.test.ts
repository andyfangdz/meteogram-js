import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { nativeFixture, withNativeClouds } from "./__helpers__/nativeForecast";
import { createFakeOpenMeteoResponse } from "./__helpers__/fakeOpenMeteo";
import { MODEL_CONFIGS } from "@/config/weather";
import { getWeatherData } from "@/app/actions/weather";

const { openMeteo } = vi.hoisted(() => ({ openMeteo: vi.fn() }));
vi.mock("openmeteo", () => ({ fetchWeatherApi: openMeteo }));

const token = "public-test-token-that-must-never-reach-the-browser";
const requests = vi.fn();
let failure: "none" | "invalid" | "timeout" | "unavailable" = "none";

describe("server-side native weather provider", () => {
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date("2026-10-18T19:15:00Z"));
    vi.stubEnv("METEOGRAM_API_URL", "https://weather.example.test");
    vi.stubEnv("METEOGRAM_API_TOKEN", token);
    failure = "none";
    requests.mockReset();
    openMeteo.mockReset();
    openMeteo.mockImplementation(async (_url, params) => {
      const cfg =
        params.models === "best_match" || params.models === "gfs_hrrr"
          ? MODEL_CONFIGS.best_match
          : MODEL_CONFIGS.gfs_seamless;
      return [
        createFakeOpenMeteoResponse(cfg.forecastDataKey, cfg.hpaLevels, 2),
      ];
    });
    vi.stubGlobal(
      "fetch",
      requests.mockImplementation(async (input) => {
        const url = new URL(String(input));
        if (url.hostname === "api.open-meteo.com") {
          return { ok: true, json: async () => ({ elevation: [100] }) };
        }
        if (failure === "timeout")
          throw new DOMException("Timed out", "TimeoutError");
        const body = nativeFixture(
          url.searchParams.get("model") as "hrrr" | "gfs" | "rrfs",
        );
        if (failure === "invalid") (body.rows[0][1] as number[])[0] = Infinity;
        return {
          ok: failure !== "unavailable",
          status: failure === "unavailable" ? 503 : 200,
          json: async () => body,
        };
      }),
    );
    vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(console, "error").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.unstubAllEnvs();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("uses native profiles with server-only authentication and independent terrain", async () => {
    const result = await getWeatherData(
      "gfs_seamless",
      "New Point@40.872665,-74.283767",
    );
    expect(result.source).toMatchObject({
      provider: "dynamical",
      model: "gfs",
    });
    expect(result.data).toHaveLength(3);
    expect(result.elevationFt).toBeCloseTo(328.084);
    expect(openMeteo).not.toHaveBeenCalled();
    const call = requests.mock.calls.find(
      ([input]) => new URL(String(input)).hostname === "weather.example.test",
    )!;
    expect(new URL(String(call[0])).searchParams.get("lat")).toBe("40.872665");
    expect(call[1]).toMatchObject({
      headers: { Authorization: "Bearer " + token },
      cache: "no-store",
      redirect: "error",
    });
    expect(JSON.stringify(result)).not.toContain(token);
    expect(JSON.stringify(result)).not.toContain("weather.example.test");
  });

  it.each(["invalid", "timeout", "unavailable"] as const)(
    "falls back on %s without losing elevation",
    async (kind) => {
      failure = kind;
      const result = await getWeatherData("gfs_seamless", "KCDW");
      expect(result.source).toMatchObject({
        provider: "open-meteo",
        fallback: true,
        model: "gfs_seamless",
      });
      expect(openMeteo).toHaveBeenCalled();
      expect(result.elevationFt).toBeCloseTo(328.084);
      expect(
        requests.mock.calls.filter(
          ([input]) => new URL(String(input)).hostname === "api.open-meteo.com",
        ),
      ).toHaveLength(1);
    },
  );

  it("keeps locations outside CONUS on the global provider", async () => {
    const result = await getWeatherData("gfs_seamless", "London@51.5,-0.1");
    expect(result.source.provider).toBe("open-meteo");
    expect(
      requests.mock.calls.some(
        ([input]) => new URL(String(input)).hostname === "weather.example.test",
      ),
    ).toBe(false);
  });

  it("exposes RRFS and labels its actual global fallback model", async () => {
    expect((await getWeatherData("rrfs", "KCDW")).source.model).toBe("rrfs");
    failure = "unavailable";
    expect((await getWeatherData("rrfs", "KCDW")).source).toMatchObject({
      provider: "open-meteo",
      model: "gfs_seamless",
      fallback: true,
    });
    expect(openMeteo.mock.calls[0][1].models).toBe("gfs_seamless");
  });

  it("overlays the newest HRRR run on the complete 48-hour endpoint", async () => {
    requests.mockImplementation(async (input) => {
      const url = new URL(String(input));
      if (url.hostname === "api.open-meteo.com")
        return { ok: true, json: async () => ({ elevation: [100] }) };
      const isLong = url.searchParams.get("run") === "long";
      const f = nativeFixture(
        "hrrr",
        isLong ? "2026-10-18T18:00:00" : "2026-10-18T19:00:00",
      );
      if (isLong) {
        f.dataset = "noaa-hrrr-forecast-48-hour-virtual";
        f.expected_leads = Array.from({ length: 49 }, (_, i) => i);
      }
      f.available_leads = [...f.expected_leads];
      f.missing_leads = [];
      f.forecast_complete = true;
      const template = f.rows[0];
      f.rows = f.available_leads.map((lead) => [lead, ...template.slice(1)]);
      return { ok: true, json: async () => withNativeClouds(f) };
    });
    const result = await getWeatherData("gfs_hrrr", "KCDW");
    const apiCalls = requests.mock.calls.filter(
      ([input]) => new URL(String(input)).hostname === "weather.example.test",
    );
    expect(
      apiCalls
        .map(([input]) => new URL(String(input)).searchParams.get("run"))
        .sort(),
    ).toEqual(["latest", "latest_complete", "long"]);
    expect(result.source.runs?.map((r) => r.forecastLengthHours)).toEqual([
      18, 48,
    ]);
    expect(result.data[0].initializedAt).toBe("2026-10-18T19:00:00.000Z");
    expect(result.data.at(-1)!.date.toISOString()).toBe(
      "2026-10-20T18:00:00.000Z",
    );
    expect(result.data.at(-1)!.initializedAt).toBe("2026-10-18T18:00:00.000Z");
    expect(JSON.stringify(result)).not.toContain(token);
  });

  it("rejects malformed coordinates before any network request", async () => {
    await expect(
      getWeatherData("gfs_seamless", "Broken@NaN,-74"),
    ).rejects.toThrow("Failed to process");
    expect(requests).not.toHaveBeenCalled();
    expect(openMeteo).not.toHaveBeenCalled();
  });
});
