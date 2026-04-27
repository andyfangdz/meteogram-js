import { CloudCell, CloudColumn } from "../types/weather";
import { FEET_PER_METER } from "../config/weather";
import { DALR_C_PER_KM } from "./lapseRate";

// Espy approximation: LCL height above ground ≈ 125 m per °C of dewpoint
// depression. Captures the dry-adiabatic cooling rate (≈9.8 °C/km) closing
// against the dewpoint depression decrease rate (≈1.8 °C/km).
const M_PER_C_DEPRESSION = 125;

export interface CondensationLevels {
  lclMslFt: number | null;
  lfcMslFt: number | null;
}

interface ComputeArgs {
  surfaceTempC: number;
  surfaceDewPointC: number;
  surfaceMslFt: number;
  // Cells sorted by ascending altitude (descending pressure).
  cells: CloudCell[];
}

export function computeCondensationLevels({
  surfaceTempC,
  surfaceDewPointC,
  surfaceMslFt,
  cells,
}: ComputeArgs): CondensationLevels {
  const depression = surfaceTempC - surfaceDewPointC;
  if (!Number.isFinite(depression) || depression < 0) {
    return { lclMslFt: null, lfcMslFt: null };
  }

  const lclAglFt = depression * M_PER_C_DEPRESSION * FEET_PER_METER;
  const lclMslFt = surfaceMslFt + lclAglFt;
  const lclTempC = surfaceTempC - DALR_C_PER_KM * (lclAglFt / FEET_PER_METER / 1000);

  // Walk cells lifting a surface parcel: dry-adiabatic to LCL, then moist-
  // adiabatic above. LFC is the lowest cell above the LCL where the parcel
  // becomes warmer than the environment (positive buoyancy).
  let prevZKm = surfaceMslFt / FEET_PER_METER / 1000;
  let parcelTempC = surfaceTempC;
  let lfcMslFt: number | null = null;
  const lclZKm = lclMslFt / FEET_PER_METER / 1000;

  for (const cell of cells) {
    const zKm = cell.mslFt / FEET_PER_METER / 1000;
    if (zKm <= prevZKm) continue;

    if (zKm <= lclZKm) {
      parcelTempC = surfaceTempC - DALR_C_PER_KM * (zKm - surfaceMslFt / FEET_PER_METER / 1000);
    } else {
      const startZKm = Math.max(prevZKm, lclZKm);
      const startT = prevZKm >= lclZKm ? parcelTempC : lclTempC;
      parcelTempC = startT - cell.malrCPerKm * (zKm - startZKm);
    }

    if (lfcMslFt == null && zKm > lclZKm && parcelTempC > cell.temperature) {
      lfcMslFt = cell.mslFt;
    }

    prevZKm = zKm;
  }

  return { lclMslFt, lfcMslFt };
}

export function computeColumnCondensationLevels(
  column: CloudColumn,
  surfaceMslFt: number,
): CondensationLevels {
  const lowest = column.cloud[0];
  if (!lowest) return { lclMslFt: null, lfcMslFt: null };
  const surfaceTempC = Number.isFinite(column.groundTemp)
    ? column.groundTemp
    : lowest.temperature;
  const surfaceDewPointC = Number.isFinite(column.groundDewPoint)
    ? column.groundDewPoint
    : lowest.dewPoint;
  return computeCondensationLevels({
    surfaceTempC,
    surfaceDewPointC,
    surfaceMslFt,
    cells: column.cloud,
  });
}
