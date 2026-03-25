import React from "react";
import { CloudColumn } from "../../types/weather";
import type { RouteWaypoint } from "../../types/weather";
import { formatNumber } from "../../utils/meteogram";
import type { RouteScales } from "../../hooks/useRouteScales";

interface RoutePressureLinesProps {
  crossSectionData: CloudColumn[];
  waypoints: RouteWaypoint[];
  scales: RouteScales;
  pressureLevels: number[];
}

const RoutePressureLines: React.FC<RoutePressureLinesProps> = ({
  crossSectionData,
  waypoints,
  scales,
  pressureLevels,
}) => {
  const pressureLinesData = React.useMemo(() => {
    return pressureLevels.map((hpa) => {
      const points = crossSectionData.map((d, i) => {
        const cloud = d.cloud.find((c) => c.hpa === hpa);
        return {
          x: formatNumber(scales.distanceScale(waypoints[i].distanceNM)),
          y: formatNumber(scales.mslScale(cloud?.geopotentialFt || 0)),
        };
      });

      const pathD = points.reduce((path: string, point, i: number) => {
        if (i === 0) return `M ${point.x} ${point.y}`;
        return `${path} L ${point.x} ${point.y}`;
      }, "");

      return { hpa, pathD, points };
    });
  }, [crossSectionData, waypoints, scales, pressureLevels]);

  return (
    <>
      {pressureLinesData.map(({ hpa, pathD, points }) => (
        <g key={`pressure-${hpa}`} pointerEvents="none">
          <path
            className={`pressure-line pressure-line-${hpa}`}
            d={pathD}
            stroke="gray"
            strokeWidth={1}
            strokeDasharray="4,4"
            opacity={0.5}
            fill="none"
          />
          {points.length > 0 && (
            <g className="pressure-tick">
              <line
                x1={0}
                x2={6}
                y1={points[0].y}
                y2={points[0].y}
                stroke="black"
              />
              <text
                x={9}
                y={points[0].y}
                fontSize={11}
                fill="black"
                dominantBaseline="middle"
              >
                {hpa}
              </text>
            </g>
          )}
        </g>
      ))}
    </>
  );
};

export default React.memo(RoutePressureLines);
