import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import {
  focusRun,
  latestCloud,
  leadState,
  queuedLeads,
  duration,
  parseIngestionStatus,
  utcTime,
  INGESTION_MODELS,
  type IngestionStatus,
} from "@/utils/ingestion";
import Dashboard from "@/app/ingestion/dashboard";
import { GET } from "@/app/api/ingestion/route";
const fixture = (): IngestionStatus => ({
  generated_at: 1792553000,
  worker_running: true,
  heartbeat: 1792553000,
  plan_at: 1792552990,
  source_checked_at: 1792552990,
  last_cycle: {
    time: 1792552900,
    published: 11,
    failures: 0,
    waiting: 0,
    maintenance_seconds: 1,
    discovery_seconds: 14,
    ingestion_seconds: 64,
  },
  models: INGESTION_MODELS.map((model) => ({
    model,
    runs: [
      {
        init_time: "2026-10-21T00:00:00",
        dataset: model,
        available_leads: [0, 1],
        expected_leads: [0, 1, 2],
        missing_leads: [2],
        complete: false,
        target: true,
        roles: ["latest", "building"],
        source_ready: [0, 1, 2],
        cloud_ready: model === "hrrr" ? [0, 1] : null,
        cloud_leads: model === "hrrr" ? [0, 1] : [],
        freshness: { available: true },
      },
    ],
    published_last_hour: 2,
    failed_last_hour: 0,
    waiting_last_hour: 3,
    lag_seconds: 80,
    median_lag_seconds: 60,
    lag_samples: 2,
    activity: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1],
    last_publication: 1792553000,
    last_failure: null,
  })),
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.unstubAllEnvs();
  vi.useRealTimers();
});
describe("ingestion status", () => {
  it("distinguishes unavailable clouds from ready weather and unknown source status", () => {
    const run = fixture().models[0].runs[0];
    expect(leadState(run, 2)).toBe("ready");
    expect(leadState(run, 2, true)).toBe("waiting");
    expect(queuedLeads(run)).toBe(0);
    run.cloud_ready = [0, 1, 2];
    expect(queuedLeads(run)).toBe(1);
    run.source_ready = null;
    expect(leadState(run, 2)).toBe("unknown");
    expect(leadState(run, 1)).toBe("ingested");
  });
  it("reports the newest cloud initialization and its actual available horizon", () => {
    const run = fixture().models[0].runs[0];
    const newer = {
      ...run,
      init_time: "2026-10-21T01:00:00",
      cloud_leads: [],
      cloud_ready: [0],
    };
    expect(latestCloud([run, newer])?.run).toBe(newer);
    expect(latestCloud([run, newer])?.lead).toBe(0);
    expect(latestCloud([run, newer])?.valid).toBe(
      Date.parse("2026-10-21T01:00:00Z") / 1000,
    );
    expect(
      latestCloud([{ ...run, cloud_leads: [], cloud_ready: [] }]),
    ).toBeNull();
    expect(duration(null)).toBe("—");
    expect(duration(125)).toBe("2m");
  });
  it("validates data and rejects misleading completeness or unknown models", () => {
    expect(parseIngestionStatus(fixture()).models).toHaveLength(4);
    const data = fixture();
    data.models[0].runs[0].complete = true;
    expect(() => parseIngestionStatus(data)).toThrow();
    expect(() => parseIngestionStatus({ ...fixture(), models: [] })).toThrow();
  });
  it("interprets native timestamps as UTC and prefers the long HRRR target", () => {
    expect(utcTime("2026-10-21T00:00:00")).toContain("00:00 UTC");
    const model = fixture().models[0];
    model.runs.push({
      ...model.runs[0],
      dataset: "long",
      expected_leads: Array.from({ length: 49 }, (_, i) => i),
    });
    expect(focusRun(model)?.dataset).toBe("long");
  });
  it("keeps authentication on the server and returns a generic unavailable response", async () => {
    vi.stubEnv("METEOGRAM_API_URL", "https://weather.example");
    vi.stubEnv("METEOGRAM_API_TOKEN", "private-token");
    const fetcher = vi.fn().mockResolvedValue(Response.json(fixture()));
    vi.stubGlobal("fetch", fetcher);
    const response = await GET();
    expect(response.status).toBe(200);
    expect(fetcher.mock.calls[0][1].headers.Authorization).toBe(
      "Bearer private-token",
    );
    expect(await response.text()).not.toContain("private-token");
    fetcher.mockRejectedValue(new Error("private-token"));
    const failure = await GET();
    expect(failure.status).toBe(503);
    expect(await failure.text()).not.toContain("private-token");
  });
  it("renders all models and keeps old data visibly stale on refresh failure", async () => {
    const fetcher = vi.fn().mockResolvedValue(Response.json(fixture()));
    vi.stubGlobal("fetch", fetcher);
    vi.setSystemTime(new Date(1792553000 * 1000));
    render(<Dashboard />);
    expect(await screen.findByText("Updater running")).toBeInTheDocument();
    for (const model of INGESTION_MODELS)
      expect(
        screen.getByRole("heading", { name: model.toUpperCase() }),
      ).toBeInTheDocument();
    expect(screen.getAllByRole("progressbar")).toHaveLength(4);
    fetcher.mockResolvedValue(new Response("Unavailable", { status: 503 }));
    fireEvent.click(screen.getByRole("button", { name: /Refresh status/ }));
    expect(
      await screen.findByText(/Showing the last successful snapshot/),
    ).toBeInTheDocument();
    expect(screen.getByText("Status unavailable")).toBeInTheDocument();
  });
});
