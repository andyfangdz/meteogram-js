import React from "react";
import { CloudColumn } from "../../types/weather";
import { formatNumber } from "../../utils/meteogram";

interface PressureLinesProps {
  weatherData: CloudColumn[];
  scales: {
    dateScale: any;
    mslScale: any;
  };
  pressureLevels: number[];
}

const PressureLines: React.FC<PressureLinesProps> = ({
  weatherData,
  scales,
  pressureLevels,
}) => {
  return (
    <>
      {pressureLevels.map((hpa) => {
        const points = weatherData.map((d) => {
          const cloud = d.cloud.find((c) => c.hpa === hpa);
          return {
            x: formatNumber(scales.dateScale(d.date)),
            y: formatNumber(scales.mslScale(cloud?.geopotentialFt || 0)),
          };
        });

        const pathD = points.reduce((path: string, point, i: number) => {
          if (i === 0) return `M ${point.x} ${point.y}`;
          return `${path} L ${point.x} ${point.y}`;
        }, "");

        return (
          <path
            className={`pressure-line pressure-line-${hpa}`}
            key={`pressure-line-${hpa}`}
            d={pathD}
            stroke="gray"
            strokeWidth={1}
            strokeDasharray="4,4"
            opacity={0.5}
            fill="none"
          />
        );
      })}
    </>
  );
};

export default React.memo(PressureLines);
