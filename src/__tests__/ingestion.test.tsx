import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import {
  displayedRuns,
  leadSupersession,
  leadOutsideChart,
  epoch,
  isActiveLead,
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
    active: null,
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
  it("shows a complete base for an incomplete newest run, and retains HRRR long overlays", () => {
    const gfs = fixture().models[1];
    const old = {
      ...gfs.runs[0],
      init_time: "2026-10-20T18:00:00",
      complete: true,
      roles: ["complete"],
    };
    gfs.runs.push(old);
    expect(displayedRuns(gfs)).toEqual([gfs.runs[0], old]);
    gfs.runs[0].complete = true;
    expect(displayedRuns(gfs)).toEqual([gfs.runs[0], old]);
    const hrrr = fixture().models[0];
    hrrr.runs[0].complete = true;
    const long = { ...old, roles: ["long"], dataset: "long" };
    hrrr.runs.push(long);
    expect(displayedRuns(hrrr)).toEqual([hrrr.runs[0], long]);
    const ifs = fixture().models.find((m) => m.model === "ifs")!;
    ifs.runs[0].complete = true;
    const ifsLong = { ...long, expected_leads: [0, 180] };
    ifs.runs.push(ifsLong);
    expect(displayedRuns(ifs)).toEqual([ifs.runs[0], ifsLong]);
    long.init_time = hrrr.runs[0].init_time;
    expect(displayedRuns(hrrr)).toEqual([hrrr.runs[0]]);
  });
  it("shows all three HRRR overlay initializations and deduplicates shared roles", () => {
    const model = fixture().models[0];
    const latest = model.runs[0];
    latest.init_time = "2026-10-21T02:00:00";
    const complete = {
      ...latest,
      init_time: "2026-10-21T01:00:00",
      roles: ["complete"],
      complete: true,
    };
    const long = {
      ...latest,
      init_time: "2026-10-21T00:00:00",
      roles: ["long"],
      complete: true,
    };
    model.runs.push(complete, long);
    expect(displayedRuns(model)).toEqual([latest, complete, long]);
    complete.roles.push("long");
    model.runs.pop();
    expect(displayedRuns(model)).toEqual([latest, complete]);
  });
  it("marks only published newer intervals, preserving gaps and partial coarse samples", () => {
    for (const model of fixture().models) {
      const older = model.runs[0];
      older.roles = ["complete"];
      older.expected_leads = [0, 3, 6, 9];
      older.available_leads = [0, 3, 6, 9];
      const newer = {
        ...older,
        init_time: "2026-10-21T01:00:00",
        roles: ["latest"],
        expected_leads: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9],
        available_leads: [0, 2, 3, 4],
        source_ready: [0, 1, 2, 3, 4, 5, 6],
        cloud_leads: [],
      };
      model.runs.push(newer);
      expect(leadSupersession(model, older, 0)).toBe("partial");
      expect(leadSupersession(model, older, 3)).toBe("full");
      expect(leadSupersession(model, older, 6)).toBe("none");
      expect(leadSupersession(model, newer, 0)).toBe("none");
      newer.freshness = { available: false };
      expect(leadSupersession(model, older, 3)).toBe("none");
    }
  });
  it("distinguishes past times using each model's chart cadence", () => {
    const run = fixture().models[0].runs[0];
    const now = epoch(run.init_time) + 5.5 * 3600;
    expect(leadOutsideChart("hrrr", run, 0, now)).toBe(true);
    expect(leadOutsideChart("ifs", run, 0, now)).toBe(true);
    expect(leadOutsideChart("ifs", run, 3, now)).toBe(false);
    expect(leadOutsideChart("aifs", run, 0, now)).toBe(false);
    expect(leadOutsideChart("aigfs", run, 0, now + 3600)).toBe(true);
  });
  it("toggles all supplying runs and marks older hour zero as past", async () => {
    const status = fixture();
    const hrrr = status.models[0];
    const latest = { ...hrrr.runs[0], init_time: "2026-10-21T02:00:00" };
    hrrr.runs = [
      latest,
      { ...latest, init_time: "2026-10-21T01:00:00", roles: ["complete"] },
      { ...latest, init_time: "2026-10-21T00:00:00", roles: ["long"] },
    ];
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(Response.json(status)));
    vi.setSystemTime(new Date("2026-10-21T02:30:00Z"));
    render(<Dashboard />);
    await screen.findByRole("heading", { name: "HRRR" });
    const article = screen
      .getByRole("heading", { name: "HRRR" })
      .closest("article")!;
    const toggle = screen.getByRole("checkbox", {
      name: "Show all displayed initializations",
    });
    expect(toggle).not.toBeChecked();
    expect(article.querySelectorAll("progress")).toHaveLength(1);
    fireEvent.click(toggle);
    expect(article.querySelectorAll("progress")).toHaveLength(3);
    expect(article.querySelector('[data-state="ingested"]')).toHaveAttribute(
      "data-past",
      "false",
    );
    expect(
      article.querySelectorAll('[data-past="true"]').length,
    ).toBeGreaterThan(0);
    fireEvent.click(toggle);
    expect(article.querySelectorAll("progress")).toHaveLength(1);
  });
  it("only marks the exact active initialization and lead, never a published or queued hour", () => {
    const m = fixture().models[1],
      run = m.runs[0];
    expect(isActiveLead(m, run, 2)).toBe(false);
    m.active = {
      dataset: run.dataset,
      init_time: run.init_time,
      lead: 2,
      started_at: 1792552999,
    };
    expect(isActiveLead(m, run, 2)).toBe(true);
    expect(
      isActiveLead(m, { ...run, init_time: "2026-10-20T18:00:00" }, 2),
    ).toBe(false);
    expect(isActiveLead(m, run, 1)).toBe(false);
    run.available_leads.push(2);
    expect(isActiveLead(m, run, 2)).toBe(false);
  });
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
    expect(parseIngestionStatus(fixture()).models).toHaveLength(
      INGESTION_MODELS.length,
    );
    const data = fixture();
    data.models[0].runs[0].complete = true;
    expect(() => parseIngestionStatus(data)).toThrow();
    expect(() => parseIngestionStatus({ ...fixture(), models: [] })).toThrow();
  });
  it("accepts the prior four-model service during deployment but rejects partial inventories", () => {
    const legacy = { ...fixture(), models: fixture().models.slice(0, 4) };
    expect(parseIngestionStatus(legacy).models).toHaveLength(4);
    expect(() =>
      parseIngestionStatus({
        ...fixture(),
        models: fixture().models.slice(0, 5),
      }),
    ).toThrow();
    expect(() =>
      parseIngestionStatus({ ...fixture(), models: fixture().models.slice(2) }),
    ).toThrow();
  });
  it("interprets native timestamps as UTC", () => {
    expect(utcTime("2026-10-21T00:00:00")).toContain("00:00 UTC");
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
    expect(screen.getAllByRole("progressbar")).toHaveLength(
      INGESTION_MODELS.length,
    );
    for (const model of INGESTION_MODELS) {
      const article = screen
        .getByRole("heading", { name: model.toUpperCase() })
        .closest("article")!;
      const cells = article.querySelectorAll("[data-state]");
      expect([...cells].map((cell) => cell.textContent)).toEqual(
        model === "hrrr" ? ["0", "1", "2", "0", "1", "2"] : ["0", "1", "2"],
      );
    }
    fetcher.mockResolvedValue(new Response("Unavailable", { status: 503 }));
    fireEvent.click(screen.getByRole("button", { name: /Refresh status/ }));
    expect(
      await screen.findByText(/Showing the last successful snapshot/),
    ).toBeInTheDocument();
    expect(screen.getByText("Status unavailable")).toBeInTheDocument();
  });
});
