import type {
  CloudColumn,
  WeatherSource,
  ForecastRunSource,
} from "@/types/weather";

export interface NormalizedForecastRun {
  data: CloudColumn[];
  source: WeatherSource;
}

/** Overlay exact native intervals by initialization, retaining the complete run's horizon.
 * Coarse intervals may be split at a newer interval's edges; their values and
 * original sample time are preserved. No forecast values are interpolated.
 */
export function mergeForecastRuns(
  runs: NormalizedForecastRun[],
  now = Date.now(),
): NormalizedForecastRun {
  const usable = runs.filter((run) => run.data.length > 0);
  if (!usable.length) throw new Error("No usable native forecast runs");
  if (
    usable.some(
      (run) =>
        run.source.model !== usable[0].source.model ||
        (run.source.provider !== usable[0].source.provider &&
          !(
            run.source.model === "aifs" &&
            [run.source.provider, usable[0].source.provider].every(
              (p) => p === "ecmwf" || p === "dynamical",
            )
          )),
    )
  ) {
    throw new Error("Cannot combine different forecast models/providers");
  }
  const frames = usable
    .flatMap((run, priority) => {
      const initializedAt = run.source.initializedAt;
      const init = Date.parse(initializedAt ?? "");
      if (!Number.isFinite(init))
        throw new Error("Missing forecast initialization");
      return run.data.map((column) => ({
        column,
        run,
        priority,
        init,
        initializedAt: initializedAt!,
        start: column.date.getTime(),
        end: column.date.getTime() + (column.durationMinutes ?? 60) * 60000,
      }));
    })
    .sort((a, b) => b.init - a.init || b.priority - a.priority);
  const edges = Array.from(
    new Set(frames.flatMap((f) => [f.start, f.end])),
  ).sort((a, b) => a - b);
  const selected: {
    column: CloudColumn;
    run: NormalizedForecastRun;
    frame: (typeof frames)[number];
  }[] = [];
  for (let i = 0; i < edges.length - 1; i++) {
    const start = edges[i],
      end = edges[i + 1];
    const frame = frames.find((f) => f.start <= start && f.end > start);
    if (!frame) continue;
    // Rejoin artificial edges when they did not change the winning native frame.
    const previous = selected.at(-1);
    const sample = frame.column.sampleDate ?? frame.column.date;
    if (
      previous &&
      previous.frame === frame &&
      previous.column.date.getTime() +
        (previous.column.durationMinutes ?? 60) * 60000 ===
        start
    ) {
      previous.column.durationMinutes =
        (end - previous.column.date.getTime()) / 60000;
      continue;
    }
    selected.push({
      run: frame.run,
      frame,
      column: {
        ...frame.column,
        date: new Date(start),
        durationMinutes: (end - start) / 60000,
        initializedAt: frame.initializedAt,
        ...(start !== sample.getTime() ? { sampleDate: sample } : {}),
      },
    });
  }
  const data = selected.map((s) => s.column);
  if (data.length < 2 || !data.some((d) => d.date.getTime() >= now))
    throw new Error("Native forecast has insufficient current/future hours");
  const used = usable
    .filter((run) => selected.some((s) => s.run === run))
    .sort(
      (a, b) =>
        Date.parse(b.source.initializedAt!) -
        Date.parse(a.source.initializedAt!),
    );
  const byInitialization = new Map<string, NormalizedForecastRun>();
  for (const run of used) {
    const key = run.source.initializedAt!;
    const prior = byInitialization.get(key);
    if (
      !prior ||
      (run.source.forecastLengthHours ?? 0) >
        (prior.source.forecastLengthHours ?? 0) ||
      ((run.source.forecastLengthHours ?? 0) ===
        (prior.source.forecastLengthHours ?? 0) &&
        (run.source.availableHours ?? 0) > (prior.source.availableHours ?? 0))
    ) {
      byInitialization.set(key, run);
    }
  }
  const distinct = Array.from(byInitialization.values());
  const source = { ...distinct[0].source };
  if (
    used.some(
      (run) => run.source.nativeProviders || run.source.provider === "ecmwf",
    )
  ) {
    const providers = used.flatMap(
      (run) =>
        run.source.nativeProviders ??
        (run.source.provider === "dynamical" || run.source.provider === "ecmwf"
          ? [run.source.provider]
          : []),
    );
    source.nativeProviders = Array.from(new Set(providers));
    if (source.nativeProviders.includes("ecmwf")) source.provider = "ecmwf";
  }
  const nativeHours = data.filter((d) => d.nativeCloudLayers).length;
  const modeled = data.filter((d) => d.cloudMethod === "model").length;
  source.cloudMethod =
    modeled === data.length
      ? "model"
      : modeled > 0
        ? "mixed"
        : "humidity-estimate";
  if (nativeHours) {
    source.nativeCloudLevels = 50;
    source.nativeCloudHours = nativeHours;
    source.displayedHours = data.length;
    if (
      data.some((d) =>
        d.nativeCloudLayers?.some((l) => l.geometry === "derived"),
      )
    ) {
      source.nativeCloudGeometry = "derived";
    } else delete source.nativeCloudGeometry;
  } else {
    delete source.nativeCloudLevels;
    delete source.nativeCloudHours;
    delete source.displayedHours;
    delete source.nativeCloudGeometry;
  }
  if (distinct.length > 1) {
    source.runs = distinct.map(
      ({ source: s }): ForecastRunSource => ({
        initializedAt: s.initializedAt!,
        complete: s.complete,
        availableHours: s.availableHours,
        expectedHours: s.expectedHours,
        forecastLengthHours: s.forecastLengthHours,
      }),
    );
  } else delete source.runs;
  return { data, source };
}

export function initializationBoundaries(data: CloudColumn[]) {
  return data.flatMap((column, i) => {
    const from = data[i - 1]?.initializedAt,
      to = column.initializedAt;
    return from && to && from !== to ? [{ date: column.date, from, to }] : [];
  });
}
