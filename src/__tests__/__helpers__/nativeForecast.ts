import type { NativeForecast } from "@/utils/native-weather";
import type { NativeWeatherModel } from "@/types/weather";

export function nativeFixture(
  model: NativeWeatherModel = "hrrr",
  init = "2026-10-18T19:00:00.000000000",
): NativeForecast {
  const levels =
    model === "aifs"
      ? [1000, 925, 850, 700, 600, 500, 400, 300]
      : Array.from({ length: 30 }, (_, i) => 1000 - i * 25);
  const pressureFields = [
    "geopotential_height",
    "temperature",
    "wind_u",
    "wind_v",
    ...(model === "aifs"
      ? ["specific_humidity"]
      : model === "gfs"
        ? ["relative_humidity", "cloud_cover"]
        : ["relative_humidity", "dew_point_temperature"]),
  ];
  const fields = [
    "lead_hour",
    ...pressureFields,
    "temperature_2m",
    "dew_point_temperature_2m",
  ];
  const have = model === "aifs" ? [0, 6, 12] : [0, 2, 4];
  const expected =
    model === "aifs"
      ? Array.from({ length: 31 }, (_, i) => i * 6)
      : model === "gfs"
        ? [
            ...Array.from({ length: 121 }, (_, i) => i),
            ...Array.from({ length: 20 }, (_, i) => 123 + 3 * i),
          ]
        : Array.from({ length: model === "rrfs" ? 85 : 19 }, (_, i) => i);
  const values = (field: string, i: number) => {
    if (field === "geopotential_height") return 100 + i * 250;
    if (field === "temperature") return 20 - i * 2;
    if (field === "dew_point_temperature") return 10 - i * 2;
    if (field === "wind_u") return 10;
    if (field === "wind_v") return 0;
    if (field === "relative_humidity") return 90;
    if (field === "specific_humidity") return 0.0073;
    return 35;
  };
  return {
    model,
    dataset:
      model === "hrrr"
        ? "noaa-hrrr-forecast-18-hour-virtual"
        : model === "aifs"
          ? "ecmwf-aifs-single-forecast-virtual"
          : model === "gfs"
            ? "noaa-gfs-forecast-virtual"
            : "noaa-rrfs-forecast-84-hour-virtual",
    init_time: init,
    snapshot_id: "fixture-immutable-snapshot",
    grid: { latitude: 40.872665, longitude: -74.283767, cell_id: 123 },
    levels_hpa: levels,
    fields,
    rows: have.map((lead) => [
      lead,
      ...pressureFields.map((f) => levels.map((_, i) => values(f, i))),
      20,
      10,
    ]),
    units: {
      geopotential_height: "m",
      temperature: "degree_Celsius",
      wind_u: "m s-1",
      wind_v: "m s-1",
      relative_humidity: "percent",
      cloud_cover: "percent",
      specific_humidity: "1",
      dew_point_temperature: "degree_Celsius",
    },
    native_wind_frame:
      model === "hrrr" || model === "rrfs" ? "grid-relative" : "earth-relative",
    wind_rotation_degrees: model === "hrrr" || model === "rrfs" ? 14.452 : 0,
    available_leads: have,
    expected_leads: expected,
    missing_leads: expected.filter((h) => !have.includes(h)),
    forecast_complete: false,
    freshness: { available: true },
  };
}

export function withNativeClouds(
  forecast = nativeFixture(),
  leads = forecast.available_leads,
) {
  forecast.native_clouds = {
    geometry: {
      method: "hrrr_hybrid_moist_log_pressure_v1",
      coverage: "exact_source_float32",
      height: "log_pressure_interpolation",
    },
    model_levels: Array.from({ length: 50 }, (_, i) => i + 1),
    fields: [
      "lead_hour",
      "fraction_of_cloud_cover",
      "geopotential_height",
      "pressure",
    ],
    units: {
      fraction_of_cloud_cover: "1",
      geopotential_height: "m",
      pressure: "Pa",
    },
    available_leads: [...leads],
    lead_sources: leads.map((lead) => ({
      lead,
      snapshot_id: "cloud-snapshot",
      pressure_snapshot_id: forecast.snapshot_id,
    })),
    // Source model level 1 is highest. Preserve its identity when sorting by height.
    rows: leads.map((lead) => [
      lead,
      Array.from({ length: 50 }, (_, i) => (i === 49 ? 0 : 0.8)),
      Array.from({ length: 50 }, (_, i) => 100 + (49 - i) * 200),
      Array.from({ length: 50 }, (_, i) => 100000 - (49 - i) * 1500),
    ]),
  };
  return forecast;
}
