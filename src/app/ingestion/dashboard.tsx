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
  duration,
  epoch,
  focusRun,
  latestCloud,
  leadState,
  newestRun,
  parseIngestionStatus,
  queuedLeads,
  utcTime,
  type IngestionRun,
  type IngestionStatus,
  type ModelIngestion,
} from "@/utils/ingestion";
import styles from "./ingestion.module.css";

const META = {
  hrrr: { name: "HRRR", detail: "Hourly regional", color: "#126e82" },
  gfs: { name: "GFS", detail: "Global · 1–3h steps", color: "#3867a8" },
  aifs: { name: "AIFS", detail: "Global AI · 6h steps", color: "#795aa0" },
  rrfs: { name: "RRFS", detail: "Hourly regional", color: "#99651d" },
};
const LABEL = {
  ingested: "Published",
  ready: "Ready at source",
  waiting: "Source not available",
  unknown: "Source status unknown",
};
function Coverage({
  run,
  clouds = false,
}: {
  run: IngestionRun;
  clouds?: boolean;
}) {
  const have = clouds ? run.cloud_leads : run.available_leads;
  return (
    <div className={styles.coverageRow}>
      <span className={styles.stripLabel}>{clouds ? "Clouds" : "Weather"}</span>
      <div
        className={styles.coverage}
        role="img"
        aria-label={`${clouds ? "Cloud" : "Weather"} coverage: ${have.length} of ${run.expected_leads.length} forecast times published`}
      >
        {run.expected_leads.map((h) => {
          const state = leadState(run, h, clouds);
          return (
            <span
              key={h}
              className={styles[state]}
              title={`+${h}h: ${LABEL[state]}`}
            />
          );
        })}
      </div>
      <span className={styles.count}>
        {have.length}/{run.expected_leads.length}
      </span>
    </div>
  );
}
function ModelRow({ data, now }: { data: ModelIngestion; now: number }) {
  const meta = META[data.model];
  const focus = focusRun(data) ?? newestRun(data.runs);
  const newest = newestRun(data.runs);
  const complete = data.runs.find((r) => r.roles.includes("complete"));
  const long = data.runs.find((r) => r.roles.includes("long"));
  const cloud = latestCloud(data.runs);
  const queue = data.runs.reduce((n, r) => n + queuedLeads(r), 0);
  const peak = Math.max(1, ...data.activity);
  return (
    <article
      className={styles.model}
      style={{ "--model-color": meta.color } as CSSProperties}
    >
      <header className={styles.modelHeader}>
        <h2>{meta.name}</h2>
        <p>{meta.detail}</p>
        <span
          className={`${styles.badge} ${focus?.complete ? styles.complete : ""}`}
        >
          {!focus
            ? "No data"
            : focus.complete
              ? "Complete"
              : queue
                ? "Ingesting"
                : "Waiting for source"}
        </span>
      </header>
      <div className={styles.run}>
        <div className={styles.runHeading}>
          <strong>{focus?.target ? "Target run" : "Published run"}</strong>
          <span>{focus ? utcTime(focus.init_time) : "Not available"}</span>
        </div>
        {focus && (
          <>
            <progress
              className={styles.srOnly}
              aria-label={`${meta.name} ingestion progress`}
              max={focus.expected_leads.length}
              value={focus.available_leads.length}
            />
            <Coverage run={focus} />
            {data.model === "hrrr" && <Coverage run={focus} clouds />}
            <div className={styles.horizon}>
              <span>Forecast lead +0h</span>
              <span>+{Math.max(...focus.expected_leads)}h</span>
            </div>
          </>
        )}
        {newest && newest !== focus && (
          <div className={styles.upstream}>
            <div className={styles.runHeading}>
              <strong>Newest source run</strong>
              <span>{utcTime(newest.init_time)}</span>
            </div>
            <Coverage run={newest} />
            {data.model === "hrrr" && <Coverage run={newest} clouds />}
          </div>
        )}
        {data.model === "hrrr" && (
          <p className={styles.cloudNote}>
            <strong>Latest cloud data</strong>{" "}
            {cloud ? (
              <>
                {utcTime(cloud.run.init_time)} run, through +{cloud.lead}h{" "}
                <span>(valid {utcTime(cloud.valid)})</span>
              </>
            ) : (
              "Not observed yet"
            )}
          </p>
        )}
      </div>
      <div className={styles.telemetry}>
        <dl className={styles.numbers}>
          <div title="Most recent measured source-ready to publication delay. Includes queue and processing time; observed at polling cadence.">
            <dt>Ingestion lag</dt>
            <dd>{duration(data.lag_seconds)}</dd>
          </div>
          <div title="Median source-ready to publication delay over measured publications in the last hour.">
            <dt>Median / 1h</dt>
            <dd>{duration(data.median_lag_seconds)}</dd>
          </div>
          <div title="Forecast times with all required source references ready, but not yet published.">
            <dt>Queued</dt>
            <dd>{queue}</dd>
          </div>
          <div title="Time since the initialization of the displayed target or published run; includes upstream production time.">
            <dt>Run age</dt>
            <dd>
              {focus
                ? duration(Math.max(0, now - epoch(focus.init_time)))
                : "—"}
            </dd>
          </div>
        </dl>
        <div className={styles.activity}>
          <div
            className={styles.sparkline}
            role="img"
            aria-label={`${data.published_last_hour} forecast times published in the last hour`}
          >
            {data.activity.map((n, i) => (
              <span
                key={i}
                title={`${60 - i * 5}–${55 - i * 5} minutes ago: ${n} published`}
                style={{
                  height: `${Math.max(5, (n / peak) * 100)}%`,
                  opacity: n ? 1 : 0.15,
                }}
              />
            ))}
          </div>
          <p>
            <strong>{data.published_last_hour}</strong> published / 1h
            <br />
            <span>{data.waiting_last_hour} source waits</span>
            <span className={data.failed_last_hour ? styles.errorText : ""}>
              {" "}
              · {data.failed_last_hour} failures
            </span>
          </p>
        </div>
      </div>
      <details className={styles.details}>
        <summary>
          Run details{" "}
          <span>Last publication {utcTime(data.last_publication)}</span>
        </summary>
        <div className={styles.detailBody}>
          <p>
            Latest complete:{" "}
            {complete ? utcTime(complete.init_time) : "Not available"}
            {complete && !complete.freshness.available
              ? " (outside freshness window)"
              : ""}
            {data.model === "hrrr"
              ? ` · 48-hour base: ${long ? utcTime(long.init_time) : "Not available"}`
              : ""}
            . Lag based on {data.lag_samples} publications in the last hour.
          </p>
          {data.last_failure && (
            <p className={styles.errorText}>
              {data.last_failure.reason}: {utcTime(data.last_failure.time)}
            </p>
          )}
          <div className={styles.tableScroll}>
            <table>
              <thead>
                <tr>
                  <th>Run initialization (UTC)</th>
                  <th>Published</th>
                  <th>Source ready</th>
                  <th>Role</th>
                </tr>
              </thead>
              <tbody>
                {data.runs.map((r) => (
                  <tr key={r.dataset + r.init_time}>
                    <td>{utcTime(r.init_time)}</td>
                    <td>
                      {r.available_leads.length}/{r.expected_leads.length}
                    </td>
                    <td>{r.source_ready?.length ?? "Not checked"}</td>
                    <td>
                      {r.roles.join(", ")}
                      {r.target ? ", target" : ""}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </details>
    </article>
  );
}
export default function IngestionDashboard() {
  const [data, setData] = useState<IngestionStatus | null>(null);
  const [error, setError] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [clock, setClock] = useState(() => Date.now() / 1000);
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
      setClock(Date.now() / 1000);
      if (!document.hidden) void refresh();
    }, 30000);
    return () => {
      clearInterval(timer);
      request.current?.abort();
      request.current = null;
    };
  }, [refresh]);
  const stale = data !== null && clock - data.generated_at > 90;
  const sourceStale =
    data !== null &&
    (data.source_checked_at === null || clock - data.source_checked_at > 180);
  const sum = (
    key: "published_last_hour" | "waiting_last_hour" | "failed_last_hour",
  ) => data?.models.reduce((n, m) => n + m[key], 0) ?? 0;
  return (
    <div className={styles.page}>
      <nav className={styles.nav} aria-label="Page navigation">
        <Link href="/">Meteogram</Link>
        <Link href="/">Back to forecasts</Link>
      </nav>
      <main className={styles.main}>
        <header className={styles.header}>
          <div>
            <h1>Forecast ingestion</h1>
            <p>CONUS source availability and publication. All times UTC.</p>
          </div>
          <div className={styles.controls}>
            <span
              className={
                error || stale || !data?.worker_running
                  ? styles.offline
                  : styles.live
              }
            >
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
            <button onClick={() => void refresh()} disabled={refreshing}>
              {refreshing ? "Refreshing…" : "Refresh status"}
            </button>
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
            <section
              className={styles.overview}
              aria-label="Last hour activity"
            >
              <span>Last hour</span>
              <span>
                <strong>{sum("published_last_hour")}</strong> published
              </span>
              <span>
                <strong>{sum("waiting_last_hour")}</strong> source waits
              </span>
              <span className={sum("failed_last_hour") ? styles.errorText : ""}>
                <strong>{sum("failed_last_hour")}</strong> failures
              </span>
              <small>Refreshes every 30s</small>
            </section>
            <div className={styles.legend}>
              {Object.entries(LABEL).map(([state, label]) => (
                <span key={state}>
                  <i className={styles[state]} />
                  {label}
                </span>
              ))}
            </div>
            {sourceStale && (
              <p className={styles.alert}>
                Source checks are{" "}
                {data.source_checked_at
                  ? `${duration(Math.max(0, clock - data.source_checked_at))} old`
                  : "not recorded yet"}
                . Strips show the last observed availability.
              </p>
            )}
            <section
              className={styles.models}
              aria-label="Model ingestion progress"
            >
              {data.models.map((m) => (
                <ModelRow key={m.model} data={m} now={data.generated_at} />
              ))}
            </section>
            <section className={styles.cycle} aria-label="Last completed cycle">
              <strong>Last cycle</strong>
              <span>{utcTime(data.last_cycle.time)}</span>
              <span>
                Cleanup {duration(data.last_cycle.maintenance_seconds)}
              </span>
              <span>
                Discovery {duration(data.last_cycle.discovery_seconds)}
              </span>
              <span>
                Ingestion {duration(data.last_cycle.ingestion_seconds)}
              </span>
              <span>{data.last_cycle.waiting ?? "—"} waiting</span>
            </section>
            <footer className={styles.footer}>
              <p>
                Ingestion lag starts when all required source references are
                first observed ready, and ends after publication. It includes
                queue time; polling adds up to one cycle of uncertainty. A dash
                means no measured publication in the last hour. Run age includes
                upstream production time.
              </p>
              <p>
                Each segment is one forecast time. HRRR clouds require all 50
                levels and geometry anchors. Source waits are availability
                delays, counted separately from failures. Completed runs may no
                longer be checked upstream. Source checked{" "}
                {utcTime(data.source_checked_at)}. Snapshot{" "}
                {utcTime(data.generated_at)}.
              </p>
            </footer>
          </>
        ) : !error ? (
          <p className={styles.loading}>Loading model progress…</p>
        ) : null}
      </main>
    </div>
  );
}
