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
  freshness: { available: boolean };
}
export interface ModelIngestion {
  model: IngestionModel;
  runs: IngestionRun[];
  published_last_hour: number;
  failed_last_hour: number;
  activity: number[];
  last_publication: number | null;
  last_failure: { time: number; reason: string } | null;
}
export interface IngestionStatus {
  generated_at: number;
  worker_running: boolean;
  heartbeat: number | null;
  plan_at: number | null;
  last_cycle: {
    time: number | null;
    published: number | null;
    failures: number | null;
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
export function focusRun(model: ModelIngestion) {
  return (
    model.runs.find(
      (r) =>
        r.target && r.expected_leads.length === 49 && model.model === "hrrr",
    ) ??
    model.runs.find((r) => r.target) ??
    model.runs.find((r) => r.roles.includes("latest"))
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
    !object(value.last_cycle) ||
    ![
      "time",
      "published",
      "failures",
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
      !number(m.published_last_hour) ||
      !number(m.failed_last_hour) ||
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
        !object(r.freshness) ||
        typeof r.freshness.available !== "boolean"
      )
        throw new Error("Invalid run status");
    }
  }
  return value as unknown as IngestionStatus;
}
