import React from "react";
import { CloudCell } from "../../types/weather";
import { hPaToInHg, kmhToKnots, formatNumber } from "../../utils/meteogram";
import {
  computeMALR,
  getStabilityCategory,
  getStabilityColor,
  STABILITY_LABELS,
  DALR_C_PER_KM,
  ISA_C_PER_KM,
  cPerKmToCPerKft,
} from "../../utils/lapseRate";

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
  const elr = cloudCell.lapseRateAboveCPerKm;
  const malr =
    cloudCell.temperature != null && cloudCell.hpa != null
      ? computeMALR(cloudCell.temperature, cloudCell.hpa)
      : null;
  const stability =
    elr != null && malr != null ? getStabilityCategory(elr, malr) : null;
  return (
    <foreignObject
      x={x}
      y={y}
      width="240"
      height="280"
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
        {cloudCell.dewPoint != null && (
          <div>{`Dew Point: ${formatNumber(cloudCell.dewPoint)}°C`}</div>
        )}
        {cloudCell.temperature != null && cloudCell.dewPoint != null && (
          <div>{`Spread (T-Td): ${formatNumber(cloudCell.temperature - cloudCell.dewPoint)}°C`}</div>
        )}
        {cloudCell.windSpeed != null && (
          <div>{`Wind Speed: ${kmhToKnots(cloudCell.windSpeed)} kt`}</div>
        )}
        {cloudCell.windDirection != null && (
          <div>{`Wind Direction: ${formatNumber(cloudCell.windDirection)}°`}</div>
        )}
        {elr != null && malr != null && stability != null && (
          <div
            style={{
              marginTop: "6px",
              paddingTop: "6px",
              borderTop: "1px solid rgba(0,0,0,0.1)",
            }}
          >
            <div
              style={{
                display: "inline-block",
                padding: "1px 6px",
                marginBottom: "2px",
                borderRadius: "3px",
                backgroundColor: getStabilityColor(stability).replace(
                  /[\d.]+\)$/,
                  "0.6)",
                ),
                fontWeight: 600,
              }}
            >
              {STABILITY_LABELS[stability]}
            </div>
            <div>{`ELR: ${cPerKmToCPerKft(elr).toFixed(2)} °C/kft`}</div>
            <div style={{ color: "#666" }}>{`DALR ${cPerKmToCPerKft(
              DALR_C_PER_KM,
            ).toFixed(2)} · MALR ${cPerKmToCPerKft(malr).toFixed(
              2,
            )} · ISA ${cPerKmToCPerKft(ISA_C_PER_KM).toFixed(2)} °C/kft`}</div>
          </div>
        )}
      </div>
    </foreignObject>
  );
};

export default React.memo(MeteogramTooltip);
