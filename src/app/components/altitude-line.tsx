"use client";
import React from "react";
import type { CloudColumn, RouteWaypoint } from "../../types/weather";
import { formatNumber } from "../../utils/meteogram";
import type { RouteScales } from "../../hooks/useRouteScales";
import WindBarb from "./wind-barb";

interface AltitudeLineProps {
  cruiseAltitudeFt: number;
  waypoints: RouteWaypoint[];
  crossSectionData: CloudColumn[];
  scales: RouteScales;
  showWindBarbs: boolean;
}

const AltitudeLine = React.memo(function AltitudeLine({
  cruiseAltitudeFt,
  waypoints,
  crossSectionData,
  scales,
  showWindBarbs,
}: AltitudeLineProps) {
  if (waypoints.length === 0) return null;

  const { distanceScale, mslScale } = scales;

  const firstWaypoint = waypoints[0];
  const lastWaypoint = waypoints[waypoints.length - 1];

  const x1 = formatNumber(distanceScale(firstWaypoint.distanceNM));
  const x2 = formatNumber(distanceScale(lastWaypoint.distanceNM));
  const y = formatNumber(mslScale(cruiseAltitudeFt));

  const labelText = `${Math.round(cruiseAltitudeFt).toLocaleString()} ft`;

  // Find closest CloudCell by mslFt for wind barbs every 3rd data point
  const windBarbEntries: Array<{
    x: number;
    speed: number;
    direction: number;
  }> = [];

  if (showWindBarbs) {
    crossSectionData.forEach((column, colIndex) => {
      if (colIndex % 3 !== 0) return;

      // Find the waypoint index closest to this column (by position in array)
      const waypointIndex = Math.round(
        (colIndex / (crossSectionData.length - 1)) * (waypoints.length - 1),
      );
      const waypoint = waypoints[Math.min(waypointIndex, waypoints.length - 1)];

      // Find closest cell to cruise altitude
      let closestCell = column.cloud[0];
      let minDiff = Infinity;
      for (const cell of column.cloud) {
        const diff = Math.abs(cell.mslFt - cruiseAltitudeFt);
        if (diff < minDiff) {
          minDiff = diff;
          closestCell = cell;
        }
      }

      if (
        closestCell &&
        Number.isFinite(closestCell.windSpeed) &&
        Number.isFinite(closestCell.windDirection)
      ) {
        windBarbEntries.push({
          x: formatNumber(distanceScale(waypoint.distanceNM)),
          speed: closestCell.windSpeed,
          direction: closestCell.windDirection,
        });
      }
    });
  }

  return (
    <g>
      {/* Dashed horizontal line at cruise altitude */}
      <line
        x1={x1}
        y1={y}
        x2={x2}
        y2={y}
        stroke="#FF4500"
        strokeWidth={2}
        strokeDasharray="8,4"
      />
      {/* Label at right end */}
      <text
        x={x2 + 6}
        y={y}
        dominantBaseline="middle"
        fontSize={11}
        fill="#FF4500"
        fontWeight="bold"
      >
        {labelText}
      </text>
      {/* Wind barbs along the altitude line */}
      {showWindBarbs &&
        windBarbEntries.map((entry, i) => (
          <WindBarb
            key={i}
            x={entry.x}
            y={y}
            speed={entry.speed}
            direction={entry.direction}
            size={20}
          />
        ))}
    </g>
  );
});

export default AltitudeLine;
