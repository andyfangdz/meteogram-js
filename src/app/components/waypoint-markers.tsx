"use client";
import React from "react";
import type { RouteWaypoint } from "../../types/weather";
import { formatNumber } from "../../utils/meteogram";
import type { RouteScales } from "../../hooks/useRouteScales";

interface WaypointMarkersProps {
  waypoints: RouteWaypoint[];
  scales: RouteScales;
  yMax: number;
  model: string;
}

const WaypointMarkers = React.memo(function WaypointMarkers({
  waypoints,
  scales,
  yMax,
  model,
}: WaypointMarkersProps) {
  const { distanceScale } = scales;

  const userDefinedWaypoints = waypoints.filter((wp) => wp.isUserDefined);

  if (userDefinedWaypoints.length === 0) return null;

  return (
    <g>
      {userDefinedWaypoints.map((wp) => {
        const x = formatNumber(distanceScale(wp.distanceNM));
        return (
          <g key={wp.name}>
            {/* Vertical dashed line from y=0 to y=yMax */}
            <line
              x1={x}
              y1={0}
              x2={x}
              y2={yMax}
              stroke="#555"
              strokeWidth={1}
              strokeDasharray="4,3"
            />
            {/* Clickable label above the line */}
            <foreignObject x={x - 30} y={-20} width={60} height={20}>
              <a
                href={`/${encodeURIComponent(wp.name)}/${model}`}
                style={{
                  display: "block",
                  textAlign: "center",
                  fontSize: "11px",
                  color: "#333",
                  textDecoration: "none",
                  whiteSpace: "nowrap",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                }}
              >
                {wp.name}
              </a>
            </foreignObject>
          </g>
        );
      })}
    </g>
  );
});

export default WaypointMarkers;
