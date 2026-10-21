"use client";

import Link from "next/link";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import {
  focusRun,
  parseIngestionStatus,
  utcTime,
  type IngestionRun,
  type IngestionStatus,
  type ModelIngestion,
} from "@/utils/ingestion";
import styles from "./ingestion.module.css";

const META = {
  hrrr: {
    name: "HRRR",
    detail: "Regional · hourly forecasts",
    color: "#087f8c",
  },
  gfs: {
    name: "GFS",
    detail: "Global · hourly / 3-hour forecasts",
    color: "#3668b0",
  },
  aifs: {
    name: "AIFS",
    detail: "Global AI · 6-hour forecasts",
    color: "#8061ab",
  },
  rrfs: {
    name: "RRFS",
    detail: "Regional · hourly forecasts",
    color: "#b46c20",
  },
};
function RunCoverage({ run }: { run: IngestionRun }) {
  const have = new Set(run.available_leads);
  const ready = run.source_ready === null ? null : new Set(run.source_ready);
  return (
    <div
      className={styles.coverage}
      style={
        {
          "--coverage-count": Math.min(49, run.expected_leads.length),
        } as CSSProperties
      }
      role="img"
      aria-label={`${have.size} of ${run.expected_leads.length} forecast times ingested. Missing: ${run.missing_leads.map((h) => `+${h}h`).join(", ") || "none"}.`}
    >
      {run.expected_leads.map((h) => (
        <span
          key={h}
          title={`+${h}h · ${have.has(h) ? "Ingested" : ready?.has(h) ? "Ready at source" : ready ? "Waiting for source" : "Not ingested; source status unknown"}`}
          className={
            have.has(h)
              ? styles.ingested
              : ready?.has(h)
                ? styles.ready
                : styles.waiting
          }
        />
      ))}
    </div>
  );
}
function ModelCard({ data }: { data: ModelIngestion }) {
  const meta = META[data.model];
  const focus = focusRun(data);
  const latest = data.runs.find((r) => r.roles.includes("latest"));
  const complete = data.runs.find((r) => r.roles.includes("complete"));
  const long = data.runs.find((r) => r.roles.includes("long"));
  const peak = Math.max(1, ...data.activity);
  return (
    <article
      className={styles.card}
      style={{ "--model-color": meta.color } as CSSProperties}
    >
      <header className={styles.cardHeader}>
        <div>
          <h2>
            <span className={styles.modelDot} />
            {meta.name}
          </h2>
          <p>{meta.detail}</p>
        </div>
        <span
          className={`${styles.badge} ${focus?.complete ? styles.complete : ""}`}
        >
          {!focus ? "No data" : focus.complete ? "Run complete" : "Catching up"}
        </span>
      </header>
      <div className={styles.focusLabel}>
        {focus?.target ? "Completing this run" : "Newest retained run"}
        <span>{focus ? utcTime(focus.init_time) : "—"}</span>
      </div>
      <div className={styles.progressHeading}>
        <strong>
          {focus?.available_leads.length ?? 0}
          <small> / {focus?.expected_leads.length ?? "—"}</small>
        </strong>
        <span>forecast times ingested</span>
        <b>
          {focus
            ? Math.round(
                (100 * focus.available_leads.length) /
                  focus.expected_leads.length,
              )
            : 0}
          %
        </b>
      </div>
      <progress
        className={styles.progress}
        aria-label={`${meta.name} ingestion progress`}
        max={focus?.expected_leads.length || 1}
        value={focus?.available_leads.length || 0}
      />
      {focus ? (
        <>
          <RunCoverage run={focus} />
          <div className={styles.horizon}>
            <span>+0h</span>
            <span>+{Math.max(...focus.expected_leads)}h forecast horizon</span>
          </div>
        </>
      ) : (
        <p>No run has been published yet.</p>
      )}
      <dl className={styles.runFacts}>
        <div>
          <dt>Newest run</dt>
          <dd>
            {latest ? utcTime(latest.init_time) : "Not available"}
            {latest && (
              <small>
                {latest.available_leads.length}/{latest.expected_leads.length}{" "}
                ingested
              </small>
            )}
          </dd>
        </div>
        <div>
          <dt>Latest complete</dt>
          <dd>
            {complete ? utcTime(complete.init_time) : "Not available"}
            <small>
              {complete
                ? complete.freshness.available
                  ? "Within freshness window"
                  : "Outside freshness window"
                : "Waiting for a complete run"}
            </small>
          </dd>
        </div>
        {data.model === "hrrr" && (
          <div>
            <dt>48-hour base</dt>
            <dd>
              {long ? utcTime(long.init_time) : "Not available"}
              <small>
                {long
                  ? long.freshness.available
                    ? "Within freshness window"
                    : "Outside freshness window"
                  : "Waiting for a complete long run"}
              </small>
            </dd>
          </div>
        )}
      </dl>
      <div className={styles.activityHeader}>
        <span>Last 60 minutes</span>
        <strong>
          {data.published_last_hour} published <span>·</span>{" "}
          {data.failed_last_hour} failed attempts
        </strong>
      </div>
      <div
        className={styles.sparkline}
        role="img"
        aria-label={`${data.published_last_hour} forecast times published in the last hour, in five-minute intervals`}
      >
        {data.activity.map((count, i) => (
          <div
            key={i}
            title={`${60 - i * 5}–${55 - i * 5} minutes ago: ${count} published`}
          >
            <span
              style={{
                height: `${Math.max(3, (100 * count) / peak)}%`,
                opacity: count ? 1 : 0.15,
              }}
            />
          </div>
        ))}
      </div>
      <div className={styles.horizon}>
        <span>60 min ago</span>
        <span>Now</span>
      </div>
      <p className={styles.lastPublication}>
        Last publication: {utcTime(data.last_publication)}
      </p>
      {data.last_failure && (
        <p className={styles.failure}>
          {data.last_failure.reason} · {utcTime(data.last_failure.time)}
        </p>
      )}
      <details className={styles.details}>
        <summary>
          Inspect retained runs <span>{data.runs.length}</span>
        </summary>
        <div className={styles.tableScroll}>
          <table>
            <thead>
              <tr>
                <th>Initialization (UTC)</th>
                <th>Progress</th>
                <th>Role</th>
              </tr>
            </thead>
            <tbody>
              {data.runs.map((r) => (
                <tr key={r.dataset + r.init_time}>
                  <td>{utcTime(r.init_time).replace(" UTC", "")}</td>
                  <td>
                    {r.available_leads.length}/{r.expected_leads.length}
                  </td>
                  <td>
                    {r.target
                      ? "Completion target"
                      : r.roles.includes("long")
                        ? "48-hour base"
                        : r.roles.includes("complete")
                          ? "Complete"
                          : r.roles.includes("latest")
                            ? "Newest"
                            : "Building"}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </details>
    </article>
  );
}
export default function IngestionDashboard() {
  const [data, setData] = useState<IngestionStatus | null>(null);
  const [error, setError] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const request = useRef<AbortController | null>(null);
  const refresh = useCallback(async () => {
    if (request.current) return;
    const controller = new AbortController();
    request.current = controller;
    setRefreshing(true);
    try {
      const response = await fetch("/api/ingestion", {
        cache: "no-store",
        signal: controller.signal,
      });
      if (!response.ok) throw new Error("Unavailable");
      const next = parseIngestionStatus(await response.json());
      if (!controller.signal.aborted) {
        setData(next);
        setError(false);
      }
    } catch {
      if (!controller.signal.aborted) setError(true);
    } finally {
      if (request.current === controller) {
        request.current = null;
        if (!controller.signal.aborted) setRefreshing(false);
      }
    }
  }, []);
  useEffect(() => {
    void refresh();
    const timer = setInterval(() => {
      if (!document.hidden) void refresh();
    }, 30000);
    return () => {
      clearInterval(timer);
      request.current?.abort();
      request.current = null;
    };
  }, [refresh]);
  const stale = data !== null && Date.now() / 1000 - data.generated_at > 90;
  const published =
    data?.models.reduce((n, m) => n + m.published_last_hour, 0) ?? 0;
  const failures =
    data?.models.reduce((n, m) => n + m.failed_last_hour, 0) ?? 0;
  const complete =
    data?.models.filter((m) =>
      m.runs.some((r) => r.roles.includes("latest") && r.complete),
    ).length ?? 0;
  const seconds = (n: number | null | undefined) =>
    n == null ? "—" : `${n.toFixed(1)}s`;
  return (
    <div className={styles.page}>
      <nav className={styles.nav} aria-label="Page navigation">
        <Link href="/" className={styles.brand}>
          Meteogram<span>/</span>
          <span>Ingestion</span>
        </Link>
        <Link href="/">Back to forecasts ↗</Link>
      </nav>
      <main className={styles.main}>
        <header className={styles.hero}>
          <div>
            <p className={styles.eyebrow}>FORECAST OPERATIONS</p>
            <h1>From source to forecast.</h1>
            <p className={styles.intro}>
              Live CONUS ingestion across four weather models. Follow each run
              as forecast hours are validated and published.
            </p>
          </div>
          <div className={styles.liveBox}>
            <span
              className={`${styles.liveStatus} ${!data?.worker_running || error || stale ? styles.offline : ""}`}
            >
              <i />
              {error
                ? "Status unavailable"
                : stale
                  ? "Snapshot out of date"
                  : !data
                    ? "Connecting"
                    : data.worker_running
                      ? "Updater running"
                      : "No recent updater heartbeat"}
            </span>
            <button
              type="button"
              onClick={() => void refresh()}
              disabled={refreshing}
            >
              {refreshing ? "Refreshing…" : "Refresh status"}{" "}
              <span aria-hidden="true">↻</span>
            </button>
            <small>Refreshes every 30 seconds · all times UTC</small>
          </div>
        </header>
        <div
          aria-live="polite"
          className={error || stale ? styles.alert : styles.srOnly}
        >
          {error || stale
            ? `Unable to confirm current ingestion status.${data ? " Showing the last successful snapshot from " + utcTime(data.generated_at) + "." : " Please try refreshing shortly."}`
            : data
              ? "Ingestion status updated."
              : "Loading ingestion status."}
        </div>
        {data ? (
          <>
            <section className={styles.metrics} aria-label="Ingestion overview">
              <div>
                <span>Published in the last hour</span>
                <strong>
                  {published}
                  <small> forecast times</small>
                </strong>
              </div>
              <div>
                <span>Newest runs complete</span>
                <strong>
                  {complete}
                  <small> / 4 models</small>
                </strong>
              </div>
              <div>
                <span>Failed attempts · last hour</span>
                <strong className={failures ? styles.warning : ""}>
                  {failures}
                  <small>{failures ? " recorded" : " recorded failures"}</small>
                </strong>
              </div>
              <div>
                <span>Last cycle published</span>
                <strong>
                  {data.last_cycle.published ?? "—"}
                  <small> forecast times</small>
                </strong>
              </div>
            </section>
            <div className={styles.sectionHeading}>
              <h2>Model progress</h2>
              <div className={styles.legend}>
                <span>
                  <i className={styles.ingested} />
                  Ingested
                </span>
                <span>
                  <i className={styles.ready} />
                  Ready at source
                </span>
                <span>
                  <i className={styles.waiting} />
                  Waiting / unknown
                </span>
              </div>
            </div>
            <section
              className={styles.grid}
              aria-label="Model ingestion progress"
            >
              {data.models.map((m) => (
                <ModelCard key={m.model} data={m} />
              ))}
            </section>
            <section className={styles.pipeline}>
              <div>
                <p className={styles.eyebrow}>LAST FINISHED CYCLE</p>
                <h2>Time spent preparing each update</h2>
                <p>
                  {utcTime(data.last_cycle.time)} ·{" "}
                  {data.last_cycle.failures ?? "—"} recorded failures
                </p>
              </div>
              <dl>
                <div>
                  <dt>Cleanup</dt>
                  <dd>{seconds(data.last_cycle.maintenance_seconds)}</dd>
                </div>
                <div>
                  <dt>Source discovery</dt>
                  <dd>{seconds(data.last_cycle.discovery_seconds)}</dd>
                </div>
                <div>
                  <dt>Ingestion</dt>
                  <dd>{seconds(data.last_cycle.ingestion_seconds)}</dd>
                </div>
              </dl>
            </section>
            <footer className={styles.footer}>
              <p>
                A completion target can be older than the newest run. The
                updater finishes ready forecasts before moving on. Each square
                represents a forecast time, not an elapsed hour; model cadences
                differ. Source readiness covers pressure and surface fields;
                HRRR cloud data can arrive later.
              </p>
              <p>
                Snapshot: {utcTime(data.generated_at)}. Activity counts reflect
                recorded attempts in the last hour, not server uptime. Forecast
                pages apply their own coverage checks and may use Open-Meteo
                while native data catches up.
              </p>
            </footer>
          </>
        ) : !error ? (
          <div className={styles.loading} aria-label="Loading model status">
            {["HRRR", "GFS", "AIFS", "RRFS"].map((m) => (
              <div key={m}>
                <h2>{m}</h2>
                <p>Loading forecast progress…</p>
              </div>
            ))}
          </div>
        ) : null}
      </main>
    </div>
  );
}
