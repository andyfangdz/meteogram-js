import React from "react";
import { CloudCell, NativeCloudLayer } from "../../types/weather";
import { hPaToInHg, kmhToKnots, formatNumber } from "../../utils/meteogram";
import {
  getInstabilityColor,
  getInstabilityLabel,
  isCellSaturated,
  DALR_C_PER_KM,
  ISA_C_PER_KM,
  cPerKmToCPerKft,
} from "../../utils/lapseRate";

interface MeteogramTooltipProps {
  date: Date;
  cloudCell: CloudCell;
  nativeCloudLayer?: NativeCloudLayer;
  initializedAt?: string;
  sampleDate?: Date;
  x: number;
  y: number;
  useLocalTime: boolean;
  frozen?: boolean;
}

const MeteogramTooltip: React.FC<MeteogramTooltipProps> = ({
  date,
  cloudCell,
  nativeCloudLayer,
  initializedAt,
  sampleDate,
  x,
  y,
  useLocalTime,
  frozen = false,
}) => {
  const cloud = nativeCloudLayer ?? cloudCell;
  const heightLabel = nativeCloudLayer ? Math.round : formatNumber;
  const elr = cloudCell.lapseRateAboveCPerKm;
  const malr = cloudCell.malrCPerKm;
  const instability = cloudCell.instabilityKPerKm;
  const saturated = isCellSaturated(cloudCell);
  const instabilityFill =
    instability != null ? getInstabilityColor(instability, saturated) : null;
  // Two independent conditions: the instability badge needs a non-deadband
  // score, the ELR/DALR/MALR row needs the lapse-rate fields. Today both
  // become null together (same dHFt <= 0 guard upstream), but splitting them
  // here means a future tweak to either compute path can't silently suppress
  // the other half of the block.
  const showInstabilityBadge = instability != null && instabilityFill != null;
  const showElrRow = elr != null && malr != null;
  const showStabilityBlock = showInstabilityBadge || showElrRow;
  // Instability score is labeled in K/kft. The K vs °C distinction is a
  // convention — magnitudes are identical for differences — but "K" signals
  // that this is a θe gradient, not a temperature lapse rate like the
  // °C/kft rows below.
  const instabilityKPerKft =
    instability != null ? cPerKmToCPerKft(instability) : null;
  const instabilityDisplay =
    instability != null && instabilityKPerKft != null
      ? `${getInstabilityLabel(instability, saturated)}: ${
          instability >= 0 ? "+" : ""
        }${instabilityKPerKft.toFixed(2)} K/kft`
      : null;
  return (
    <foreignObject
      x={x}
      y={y}
      width="240"
      height={
        (nativeCloudLayer ? 320 : 280) +
        (nativeCloudLayer?.geometry === "derived" ? 20 : 0) +
        (initializedAt ? 20 : 0) +
        (sampleDate ? 20 : 0)
      }
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
        {initializedAt && (
          <div>{`Initialized: ${initializedAt.slice(0, 10)} ${initializedAt.slice(11, 16)} UTC`}</div>
        )}
        {sampleDate && (
          <div>{`Native sample: ${sampleDate.toISOString().slice(0, 16).replace("T", " ")} UTC`}</div>
        )}
        {nativeCloudLayer && (
          <div>{`Native cloud · model level ${nativeCloudLayer.modelLevel}`}</div>
        )}
        {nativeCloudLayer?.geometry === "derived" && (
          <div>Height and pressure estimated</div>
        )}
        {cloud.hpa != null && (
          <div>{`Pressure: ${hPaToInHg(cloud.hpa)} inHg (${
            nativeCloudLayer ? cloud.hpa.toFixed(1) : cloud.hpa
          } hPa)`}</div>
        )}
        {cloud.mslFt != null && (
          <div>{`MSL Height: ${heightLabel(cloud.mslFt)} ft`}</div>
        )}
        {cloud.mslFtTop != null && cloud.mslFtBottom != null && (
          <div>{`Height Range: ${heightLabel(cloud.mslFtTop)} - ${heightLabel(
            cloud.mslFtBottom,
          )} ft`}</div>
        )}
        {cloud.cloudCoverage != null && (
          <div>{`Cloud Cover: ${formatNumber(cloud.cloudCoverage)}%`}</div>
        )}
        {nativeCloudLayer && (
          <div
            style={{ marginTop: 4, fontWeight: 600 }}
          >{`Atmosphere at ${cloudCell.hpa} hPa`}</div>
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
        {showStabilityBlock && (
          <div
            style={{
              marginTop: "6px",
              paddingTop: "6px",
              borderTop: "1px solid rgba(0,0,0,0.1)",
            }}
          >
            {showInstabilityBadge && (
              <div
                style={{
                  display: "inline-block",
                  padding: "1px 6px",
                  marginBottom: "2px",
                  borderRadius: "3px",
                  backgroundColor: instabilityFill!,
                  fontWeight: 600,
                }}
              >
                {instabilityDisplay}
              </div>
            )}
            {showElrRow && (
              <>
                <div>{`ELR: ${cPerKmToCPerKft(elr!).toFixed(2)} °C/kft`}</div>
                <div style={{ color: "#666" }}>{`DALR ${cPerKmToCPerKft(
                  DALR_C_PER_KM,
                ).toFixed(2)} · MALR ${cPerKmToCPerKft(malr!).toFixed(
                  2,
                )} · ISA ${cPerKmToCPerKft(ISA_C_PER_KM).toFixed(2)} °C/kft`}</div>
              </>
            )}
          </div>
        )}
      </div>
    </foreignObject>
  );
};

export default React.memo(MeteogramTooltip);
