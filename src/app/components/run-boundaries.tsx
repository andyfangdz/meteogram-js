import React, { useMemo } from "react";
import type { CloudColumn } from "@/types/weather";
import { initializationBoundaries } from "@/utils/forecast-runs";

interface Props {
  weatherData: CloudColumn[];
  dateScale: (date: Date) => number;
  xMax: number;
  yMax: number;
}
export default React.memo(function RunBoundaries({
  weatherData,
  dateScale,
  xMax,
  yMax,
}: Props) {
  const boundaries = useMemo(
    () => initializationBoundaries(weatherData),
    [weatherData],
  );
  if (!boundaries.length) return null;
  const first = weatherData.find((column) => column.initializedAt)!;
  const labels = [
    {
      date: first.date,
      to: first.initializedAt!,
      from: undefined as string | undefined,
    },
    ...boundaries,
  ];
  const palette = [
    { stroke: "#855000", fill: "#fff4d8" },
    { stroke: "#15517c", fill: "#e7f3ff" },
    { stroke: "#554080", fill: "#f0eafa" },
  ];
  const inits = Array.from(new Set(labels.map((b) => b.to))).sort(
    (a, b) => Date.parse(b) - Date.parse(a),
  );
  const color = (init: string) => palette[inits.indexOf(init) % palette.length];
  const last = weatherData.at(-1)!;
  const end = new Date(
    last.date.getTime() + (last.durationMinutes ?? 60) * 60000,
  );
  const laneEnds = [-Infinity, -Infinity];
  const width = 96;
  return (
    <g
      className="forecast-run-boundaries"
      pointerEvents="none"
      aria-label="Forecast initialization changes"
    >
      {labels.map((boundary, index) => {
        const x = dateScale(boundary.date);
        const left = Math.max(0, Math.min(x + 4, xMax - width));
        const lane = laneEnds.findIndex((end) => left >= end + 6);
        if (lane >= 0) laneEnds[lane] = left + width;
        const y = 9 + Math.max(0, lane) * 21;
        const next = dateScale(labels[index + 1]?.date ?? end);
        const shade = color(boundary.to);
        const label = `Init ${boundary.to.slice(5, 10)} ${boundary.to.slice(11, 13)}Z`;
        return (
          <g
            key={`${boundary.date.toISOString()}-${boundary.to}`}
            data-run-initialization={boundary.to}
            data-run-boundary={
              boundary.from ? boundary.date.toISOString() : undefined
            }
            data-run-from={boundary.from}
            data-run-to={boundary.from ? boundary.to : undefined}
          >
            <title>
              {boundary.from
                ? `Initialization changes from ${boundary.from} to ${boundary.to} at ${boundary.date.toISOString()}`
                : `Forecast initialized ${boundary.to}`}
            </title>
            <rect
              x={x}
              y={0}
              width={Math.max(0, next - x)}
              height={5}
              fill={shade.stroke}
            />
            {boundary.from && (
              <line
                x1={x}
                x2={x}
                y1={0}
                y2={yMax}
                stroke={shade.stroke}
                strokeWidth={1.5}
                strokeDasharray="5 4"
              />
            )}
            {lane >= 0 && (
              <>
                <rect
                  x={left}
                  y={y}
                  width={width}
                  height={19}
                  rx={3}
                  fill={shade.fill}
                  fillOpacity={0.95}
                  stroke={shade.stroke}
                  strokeWidth={0.7}
                />
                <text x={left + 5} y={y + 13} fontSize={10} fill="#663300">
                  {label}
                </text>
              </>
            )}
          </g>
        );
      })}
    </g>
  );
});
