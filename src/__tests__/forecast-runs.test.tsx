import React from "react";
import { render } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import {
  mergeForecastRuns,
  initializationBoundaries,
} from "@/utils/forecast-runs";
import { nativeFixture, withNativeClouds } from "./__helpers__/nativeForecast";
import {
  transformNativeForecast,
  parseNativeForecast,
} from "@/utils/native-weather";
import type {
  NativeWeatherModel,
  CloudColumn,
  WeatherSource,
} from "@/types/weather";
import RunBoundaries from "@/app/components/run-boundaries";

function run(
  model: NativeWeatherModel,
  init: string,
  leads: number[],
  fullHrrr = false,
  temperature = 20,
) {
  const f = nativeFixture(model, init);
  if (fullHrrr) {
    f.dataset = "noaa-hrrr-forecast-48-hour-virtual";
    f.expected_leads = Array.from({ length: 49 }, (_, i) => i);
  }
  const template = f.rows[0];
  f.available_leads = leads;
  f.missing_leads = f.expected_leads.filter((h) => !leads.includes(h));
  f.forecast_complete = f.missing_leads.length === 0;
  f.rows = leads.map((lead) =>
    template.map((v, i) =>
      i === 0
        ? lead
        : f.fields[i] === "temperature"
          ? (v as number[]).map(() => temperature)
          : v,
    ),
  );
  return model === "hrrr" ? withNativeClouds(f) : f;
}
const now = Date.parse("2026-10-18T21:15Z");

describe("forecast initialization overlays", () => {
  it("overlays the newest 18-hour HRRR on the complete 48-hour run without GFS or time interpolation", () => {
    const base = run(
      "hrrr",
      "2026-10-18T18:00:00",
      Array.from({ length: 49 }, (_, i) => i),
      true,
      10,
    );
    const latest = run(
      "hrrr",
      "2026-10-18T20:00:00",
      Array.from({ length: 19 }, (_, i) => i),
      false,
      24,
    );
    const merged = mergeForecastRuns(
      [base, latest].map((f) =>
        transformNativeForecast(parseNativeForecast(f, "hrrr"), now, {
          allowSparse: true,
        }),
      ),
      now,
    );
    expect(merged.data).toHaveLength(46);
    expect(merged.data[0].date.toISOString()).toBe("2026-10-18T21:00:00.000Z");
    expect(merged.data.at(-1)!.date.toISOString()).toBe(
      "2026-10-20T18:00:00.000Z",
    );
    expect(
      merged.data.filter((c) => c.initializedAt === "2026-10-18T20:00:00.000Z"),
    ).toHaveLength(18);
    expect(merged.data[17].cloud[0].temperature).toBe(24);
    expect(merged.data[18].cloud[0].temperature).toBe(10);
    expect(
      merged.data.every(
        (c) => c.nativeCloudLayers?.length === 50 && c.durationMinutes === 60,
      ),
    ).toBe(true);
    expect(merged.source.runs?.map((r) => r.forecastLengthHours)).toEqual([
      18, 48,
    ]);
    expect(initializationBoundaries(merged.data)).toEqual([
      {
        date: new Date("2026-10-19T15:00Z"),
        from: "2026-10-18T20:00:00.000Z",
        to: "2026-10-18T18:00:00.000Z",
      },
    ]);
  });
  it("uses the complete run for gaps and ignores an early partial run with no current sample", () => {
    const base = run(
      "hrrr",
      "2026-10-18T18:00:00",
      Array.from({ length: 49 }, (_, i) => i),
      true,
    );
    const newest = run("hrrr", "2026-10-18T20:00:00", [1, 3], false, 24);
    const normal = (f: typeof base) =>
      transformNativeForecast(f, now, { allowSparse: true });
    const data = mergeForecastRuns([normal(base), normal(newest)], now).data;
    expect(data.slice(0, 4).map((c) => c.cloud[0].temperature)).toEqual([
      24, 20, 24, 20,
    ]);
    expect(initializationBoundaries(data)).toHaveLength(3);
    const sparse = normal(run("hrrr", "2026-10-18T20:00:00", [0]));
    expect(sparse.data).toEqual([]);
    expect(
      mergeForecastRuns([normal(base), sparse], now).source.initializedAt,
    ).toBe("2026-10-18T18:00:00.000Z");
  });
  it("preserves a coarse native sample on both sides of a newer interval", () => {
    const f = transformNativeForecast(
      nativeFixture("gfs", "2026-10-18T18:00:00"),
      Date.parse("2026-10-18T18:00Z"),
    );
    const column = f.data[0];
    const source = (initializedAt: string): WeatherSource => ({
      ...f.source,
      initializedAt,
    });
    const base = {
      source: source("2026-10-18T12:00Z"),
      data: [
        {
          ...column,
          date: new Date("2026-10-18T21:00Z"),
          durationMinutes: 180,
        },
        {
          ...column,
          date: new Date("2026-10-19T00:00Z"),
          durationMinutes: 180,
        },
      ],
    };
    const latest = {
      source: source("2026-10-18T18:00Z"),
      data: [
        { ...column, date: new Date("2026-10-18T22:00Z"), durationMinutes: 60 },
      ],
    };
    const data = mergeForecastRuns([base, latest], now).data;
    expect(data.map((c) => [c.date.getUTCHours(), c.durationMinutes])).toEqual([
      [21, 60],
      [22, 60],
      [23, 60],
      [0, 180],
    ]);
    expect(data[0].cloud).toBe(column.cloud);
    expect(data[2].sampleDate?.toISOString()).toBe("2026-10-18T21:00:00.000Z");
    expect(data[2].cloud).toBe(column.cloud);
  });
  it("deduplicates the same initialization and retains its longest horizon metadata", () => {
    const base = run(
      "hrrr",
      "2026-10-18T18:00:00",
      Array.from({ length: 49 }, (_, i) => i),
      true,
    );
    const latest = run(
      "hrrr",
      "2026-10-18T18:00:00",
      Array.from({ length: 19 }, (_, i) => i),
    );
    const m = mergeForecastRuns(
      [base, latest].map((f) =>
        transformNativeForecast(f, now, { allowSparse: true }),
      ),
      now,
    );
    expect(initializationBoundaries(m.data)).toEqual([]);
    expect(m.source.runs).toBeUndefined();
    expect(m.source.forecastLengthHours).toBe(48);
  });
  it.each(["hrrr", "gfs", "aifs", "rrfs"] as const)(
    "marks %s initialization changes in the chart",
    (model) => {
      const d = transformNativeForecast(
        nativeFixture(model, "2026-10-18T18:00:00"),
        Date.parse("2026-10-18T18:00Z"),
      );
      const data: CloudColumn[] = [
        { ...d.data[0], initializedAt: "2026-10-18T18:00:00.000Z" },
        { ...d.data[1], initializedAt: "2026-10-18T12:00:00.000Z" },
      ];
      const { container } = render(
        <svg>
          <RunBoundaries
            weatherData={data}
            dateScale={(date) => date.getTime() / 3600000}
            xMax={800}
            yMax={600}
          />
        </svg>,
      );
      const boundary = container.querySelector("[data-run-boundary]")!;
      expect(boundary.getAttribute("data-run-from")).toBe(
        "2026-10-18T18:00:00.000Z",
      );
      expect(boundary.getAttribute("data-run-to")).toBe(
        "2026-10-18T12:00:00.000Z",
      );
      expect(container.textContent).toContain("Init 10-18 12Z");
    },
  );
  it("does not invent initialization markers when run metadata is unavailable", () => {
    const d = transformNativeForecast(
      nativeFixture(),
      Date.parse("2026-10-18T19:15Z"),
    );
    const data = d.data.map((c) => ({ ...c, initializedAt: undefined }));
    expect(initializationBoundaries(data)).toEqual([]);
    const { container } = render(
      <svg>
        <RunBoundaries
          weatherData={data}
          dateScale={() => 0}
          xMax={800}
          yMax={600}
        />
      </svg>,
    );
    expect(container.querySelector("[data-run-boundary]")).toBeNull();
  });
});

it("keeps every dense initialization switch while preventing label overlap", () => {
  const original = transformNativeForecast(
    nativeFixture(),
    Date.parse("2026-10-18T19:15Z"),
  ).data[0];
  const start = Date.parse("2026-10-18T21:00Z");
  const data = Array.from({ length: 30 }, (_, i) => ({
    ...original,
    date: new Date(start + i * 3600000),
    initializedAt:
      i % 2 ? "2026-10-18T12:00:00.000Z" : "2026-10-18T18:00:00.000Z",
  }));
  const { container } = render(
    <svg>
      <RunBoundaries
        weatherData={data}
        dateScale={(date) => ((date.getTime() - start) / 3600000) * 8}
        xMax={300}
        yMax={600}
      />
    </svg>,
  );
  expect(container.querySelectorAll("[data-run-boundary]")).toHaveLength(29);
  const labels = Array.from(
    container.querySelectorAll('rect[height="19"]'),
  ).map((r) => ({
    x: Number(r.getAttribute("x")),
    y: Number(r.getAttribute("y")),
    width: Number(r.getAttribute("width")),
  }));
  for (const y of new Set(labels.map((r) => r.y))) {
    const lane = labels.filter((r) => r.y === y).sort((a, b) => a.x - b.x);
    for (let i = 1; i < lane.length; i++)
      expect(lane[i].x).toBeGreaterThanOrEqual(
        lane[i - 1].x + lane[i - 1].width,
      );
  }
});

it("retains the previous complete short run under a newly arriving partial HRRR run", () => {
  const forecasts = [
    run(
      "hrrr",
      "2026-10-18T18:00:00",
      Array.from({ length: 49 }, (_, i) => i),
      true,
      10,
    ),
    run(
      "hrrr",
      "2026-10-18T20:00:00",
      Array.from({ length: 19 }, (_, i) => i),
      false,
      24,
    ),
    run("hrrr", "2026-10-18T21:00:00", [0], false, 26),
  ];
  const merged = mergeForecastRuns(
    forecasts.map((f) =>
      transformNativeForecast(f, now, { allowSparse: true }),
    ),
    now,
  );
  expect(merged.data).toHaveLength(46);
  expect(merged.data.slice(0, 2).map((c) => c.cloud[0].temperature)).toEqual([
    26, 24,
  ]);
  expect(
    initializationBoundaries(merged.data).map((b) => [
      b.date.toISOString(),
      b.to,
    ]),
  ).toEqual([
    ["2026-10-18T22:00:00.000Z", "2026-10-18T20:00:00.000Z"],
    ["2026-10-19T15:00:00.000Z", "2026-10-18T18:00:00.000Z"],
  ]);
  expect(merged.source.runs?.map((r) => r.initializedAt.slice(11, 13))).toEqual(
    ["21", "20", "18"],
  );
});
