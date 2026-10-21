export const INGESTION_MODELS = ["hrrr", "gfs", "aifs", "rrfs"] as const;
export type IngestionModel = (typeof INGESTION_MODELS)[number];
export interface IngestionRun {
  init_time: string;
  dataset: string;
  available_leads: number[];
  expected_leads: number[];
  missing_leads: number[];
  complete: boolean;
  target: boolean;
  roles: string[];
  source_ready: number[] | null;
  cloud_ready: number[] | null;
  cloud_leads: number[];
  freshness: { available: boolean };
}
export interface ModelIngestion {
  model: IngestionModel;
  active: {
    dataset: string;
    init_time: string;
    lead: number;
    started_at: number;
  } | null;
  runs: IngestionRun[];
  published_last_hour: number;
  failed_last_hour: number;
  waiting_last_hour: number;
  lag_seconds: number | null;
  median_lag_seconds: number | null;
  lag_samples: number;
  activity: number[];
  last_publication: number | null;
  last_failure: { time: number; reason: string } | null;
}
export interface IngestionStatus {
  generated_at: number;
  worker_running: boolean;
  heartbeat: number | null;
  plan_at: number | null;
  source_checked_at: number | null;
  last_cycle: {
    time: number | null;
    published: number | null;
    failures: number | null;
    waiting: number | null;
    maintenance_seconds: number | null;
    discovery_seconds: number | null;
    ingestion_seconds: number | null;
  };
  models: ModelIngestion[];
}
export function utcTime(value: string | number | null) {
  if (value === null) return "Not recorded";
  const date = new Date(
    typeof value === "number"
      ? value * 1000
      : /Z$|[+-]\d\d:\d\d$/.test(value)
        ? value
        : value + "Z",
  );
  if (!Number.isFinite(date.getTime())) return "Not recorded";
  return (
    new Intl.DateTimeFormat("en-GB", {
      timeZone: "UTC",
      month: "short",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      hourCycle: "h23",
    }).format(date) + " UTC"
  );
}
export function parseIngestionStatus(value: unknown): IngestionStatus {
  const object = (v: unknown): v is Record<string, unknown> =>
    typeof v === "object" && v !== null;
  const number = (v: unknown): v is number =>
    typeof v === "number" && Number.isFinite(v) && v >= 0;
  const optional = (v: unknown) => v === null || number(v);
  const hours = (v: unknown): v is number[] =>
    Array.isArray(v) &&
    v.every((n) => number(n) && Number.isInteger(n)) &&
    new Set(v).size === v.length;
  if (
    !object(value) ||
    !number(value.generated_at) ||
    typeof value.worker_running !== "boolean" ||
    !optional(value.heartbeat) ||
    !optional(value.plan_at) ||
    !optional(value.source_checked_at) ||
    !object(value.last_cycle) ||
    ![
      "time",
      "published",
      "failures",
      "waiting",
      "maintenance_seconds",
      "discovery_seconds",
      "ingestion_seconds",
    ].every((k) =>
      optional(
        value.last_cycle && (value.last_cycle as Record<string, unknown>)[k],
      ),
    ) ||
    !Array.isArray(value.models) ||
    value.models.length !== 4
  )
    throw new Error("Invalid ingestion status");
  const names = new Set();
  for (const m of value.models) {
    if (
      !object(m) ||
      !INGESTION_MODELS.includes(m.model as IngestionModel) ||
      names.has(m.model) ||
      !Array.isArray(m.runs) ||
      !(
        m.active === null ||
        (object(m.active) &&
          typeof m.active.dataset === "string" &&
          typeof m.active.init_time === "string" &&
          Number.isFinite(epoch(m.active.init_time)) &&
          number(m.active.lead) &&
          Number.isInteger(m.active.lead) &&
          number(m.active.started_at))
      ) ||
      !number(m.published_last_hour) ||
      !number(m.failed_last_hour) ||
      !number(m.waiting_last_hour) ||
      !optional(m.lag_seconds) ||
      !optional(m.median_lag_seconds) ||
      !number(m.lag_samples) ||
      !optional(m.last_publication) ||
      !Array.isArray(m.activity) ||
      m.activity.length !== 12 ||
      !m.activity.every(number) ||
      !(
        m.last_failure === null ||
        (object(m.last_failure) &&
          number(m.last_failure.time) &&
          typeof m.last_failure.reason === "string")
      )
    )
      throw new Error("Invalid model status");
    names.add(m.model);
    for (const r of m.runs) {
      if (
        !object(r) ||
        typeof r.init_time !== "string" ||
        !Number.isFinite(
          Date.parse(
            r.init_time + (/Z$|[+-]\d\d:\d\d$/.test(r.init_time) ? "" : "Z"),
          ),
        ) ||
        typeof r.dataset !== "string" ||
        !hours(r.available_leads) ||
        !hours(r.expected_leads) ||
        r.expected_leads.length === 0 ||
        !r.available_leads.every((h) =>
          (r.expected_leads as number[]).includes(h),
        ) ||
        !hours(r.missing_leads) ||
        typeof r.complete !== "boolean" ||
        r.complete !== (r.available_leads.length === r.expected_leads.length) ||
        typeof r.target !== "boolean" ||
        !Array.isArray(r.roles) ||
        !r.roles.every((v) => typeof v === "string") ||
        !(r.source_ready === null || hours(r.source_ready)) ||
        !(r.cloud_ready === null || hours(r.cloud_ready)) ||
        !hours(r.cloud_leads) ||
        !r.cloud_leads.every((h) =>
          (r.available_leads as number[]).includes(h),
        ) ||
        !object(r.freshness) ||
        typeof r.freshness.available !== "boolean"
      )
        throw new Error("Invalid run status");
    }
  }
  return value as unknown as IngestionStatus;
}

export function epoch(value: string) {
  return (
    Date.parse(value + (/Z$|[+-]\d\d:\d\d$/.test(value) ? "" : "Z")) / 1000
  );
}
export function duration(seconds: number | null) {
  if (seconds === null) return "—";
  if (seconds < 60) return `${Math.round(seconds)}s`;
  if (seconds < 3600) return `${Math.round(seconds / 60)}m`;
  return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
}
export function newestRun(runs: IngestionRun[]) {
  return runs.reduce<IngestionRun | undefined>(
    (latest, run) =>
      !latest || epoch(run.init_time) > epoch(latest.init_time) ? run : latest,
    undefined,
  );
}
export function leadState(run: IngestionRun, hour: number, clouds = false) {
  if ((clouds ? run.cloud_leads : run.available_leads).includes(hour))
    return "ingested";
  const ready = clouds ? run.cloud_ready : run.source_ready;
  return ready === null
    ? "unknown"
    : ready.includes(hour)
      ? "ready"
      : "waiting";
}
export function queuedLeads(run: IngestionRun) {
  return (run.source_ready ?? []).filter(
    (h) =>
      !run.available_leads.includes(h) &&
      (run.cloud_ready === null || run.cloud_ready.includes(h)),
  ).length;
}
export function latestCloud(runs: IngestionRun[]) {
  const run = newestRun(
    runs.filter((r) => r.cloud_leads.length || r.cloud_ready?.length),
  );
  if (!run) return null;
  const leads = [...run.cloud_leads, ...(run.cloud_ready ?? [])];
  const lead = Math.max(...leads);
  return { run, lead, valid: epoch(run.init_time) + lead * 3600 };
}

/** Display the newest initialization and the complete run supplying its base. */
export function displayedRuns(model: ModelIngestion): IngestionRun[] {
  const newest = newestRun(model.runs);
  if (!newest) return [];
  const different = (r: IngestionRun) =>
    epoch(r.init_time) < epoch(newest.init_time);
  const base =
    model.model === "hrrr"
      ? model.runs.find((r) => r.roles.includes("long") && different(r))
      : undefined;
  const previous = !newest.complete
    ? (model.runs.find((r) => r.roles.includes("complete") && different(r)) ??
      newestRun(
        model.runs.filter((r) => different(r) && r.available_leads.length > 0),
      ))
    : undefined;
  const companion = base ?? previous;
  return companion ? [newest, companion] : [newest];
}
export function isActiveLead(
  model: ModelIngestion,
  run: IngestionRun,
  lead: number,
) {
  return (
    model.active !== null &&
    model.active.dataset === run.dataset &&
    epoch(model.active.init_time) === epoch(run.init_time) &&
    model.active.lead === lead &&
    !run.available_leads.includes(lead)
  );
}
