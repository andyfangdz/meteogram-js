import React from "react";
import { CloudCell } from "../../types/weather";
import { hPaToInHg, kmhToKnots, formatNumber } from "../../utils/meteogram";

interface MeteogramTooltipProps {
  date: Date;
  cloudCell: CloudCell;
  x: number;
  y: number;
  useLocalTime: boolean;
  frozen?: boolean;
}

const MeteogramTooltip: React.FC<MeteogramTooltipProps> = ({
  date,
  cloudCell,
  x,
  y,
  useLocalTime,
  frozen = false,
}) => {
  return (
    <foreignObject
      x={x}
      y={y}
      width="200"
      height="200"
      style={{
        pointerEvents: frozen ? "auto" : "none",
      }}
    >
      <div
        style={{
          backgroundColor: "rgba(255,255,255,0.9)",
          padding: "8px",
          borderRadius: "4px",
          boxShadow: "0 1px 3px rgba(0,0,0,0.2)",
          pointerEvents: frozen ? "auto" : "none",
          userSelect: frozen ? "text" : "none",
          fontSize: "12px",
          zIndex: 100,
        }}
      >
        <div>{`Time: ${
          useLocalTime
            ? date.toLocaleTimeString()
            : date.toUTCString().split(" ")[4]
        } ${useLocalTime ? "Local" : "UTC"}`}</div>
        {cloudCell.hpa != null && (
          <div>{`Pressure: ${hPaToInHg(cloudCell.hpa)} inHg (${
            cloudCell.hpa
          } hPa)`}</div>
        )}
        {cloudCell.mslFt != null && (
          <div>{`MSL Height: ${formatNumber(cloudCell.mslFt)} ft`}</div>
        )}
        {cloudCell.mslFtTop != null && cloudCell.mslFtBottom != null && (
          <div>{`Height Range: ${formatNumber(cloudCell.mslFtTop)} - ${formatNumber(
            cloudCell.mslFtBottom,
          )} ft`}</div>
        )}
        {cloudCell.cloudCoverage != null && (
          <div>{`Cloud Cover: ${formatNumber(cloudCell.cloudCoverage)}%`}</div>
        )}
        {cloudCell.temperature != null && (
          <div>{`Temperature: ${formatNumber(cloudCell.temperature)}°C`}</div>
        )}
        {cloudCell.windSpeed != null && (
          <div>{`Wind Speed: ${kmhToKnots(cloudCell.windSpeed)} kt`}</div>
        )}
        {cloudCell.windDirection != null && (
          <div>{`Wind Direction: ${formatNumber(cloudCell.windDirection)}°`}</div>
        )}
      </div>
    </foreignObject>
  );
};

export default React.memo(MeteogramTooltip);
