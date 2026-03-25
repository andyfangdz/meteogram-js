"use client";

import React from "react";
import { formatNumber } from "../../utils/meteogram";
import type { RouteWaypoint } from "../../types/weather";
import type { RouteScales } from "../../hooks/useRouteScales";

interface TerrainProfileProps {
  waypoints: RouteWaypoint[];
  elevations: Array<number | null>;
  scales: RouteScales;
  yMax: number;
}

const TerrainProfile: React.FC<TerrainProfileProps> = ({
  waypoints,
  elevations,
  scales,
  yMax,
}) => {
  const pathD = React.useMemo(() => {
    if (waypoints.length === 0) return "";

    const points = waypoints.map((wp, i) => {
      const x = formatNumber(scales.distanceScale(wp.distanceNM));
      const elev = elevations[i] ?? 0;
      const y = formatNumber(scales.mslScale(Math.max(0, elev)));
      return `${x} ${y}`;
    });

    const firstX = formatNumber(scales.distanceScale(waypoints[0].distanceNM));
    const lastX = formatNumber(scales.distanceScale(waypoints[waypoints.length - 1].distanceNM));

    return `M ${points[0]} ${points.slice(1).map((p) => `L ${p}`).join(" ")} L ${lastX} ${yMax} L ${firstX} ${yMax} Z`;
  }, [waypoints, elevations, scales, yMax]);

  if (!pathD) return null;

  return (
    <path
      d={pathD}
      fill="#8B7355"
      fillOpacity={0.6}
      stroke="#6B5B45"
      strokeWidth={1}
      pointerEvents="none"
    />
  );
};

export default React.memo(TerrainProfile);
