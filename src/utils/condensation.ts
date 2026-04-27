import { CloudCell, CloudColumn } from "../types/weather";
import { FEET_PER_METER } from "../config/weather";
import { DALR_C_PER_KM, computeMALR } from "./lapseRate";

// Espy approximation: LCL height above ground ≈ 125 m per °C of dewpoint
// depression. Captures the dry-adiabatic cooling rate (≈9.8 °C/km) closing
// against the dewpoint depression decrease rate (≈1.8 °C/km).
const M_PER_C_DEPRESSION = 125;

export interface ParcelProfile {
  lclMslFt: number | null;
  lfcMslFt: number | null;
  // Lifted-parcel temperature at each cell (parallel to the input cells
  // array). null if the parcel walk wasn't possible for that column.
  parcelTempC: (number | null)[];
}

interface ComputeArgs {
  surfaceTempC: number;
  surfaceDewPointC: number;
  surfaceMslFt: number;
  // Cells sorted by ascending altitude (descending pressure).
  cells: CloudCell[];
}

export function computeParcelProfile({
  surfaceTempC,
  surfaceDewPointC,
  surfaceMslFt,
  cells,
}: ComputeArgs): ParcelProfile {
  const empty: (number | null)[] = cells.map(() => null);
  const depression = surfaceTempC - surfaceDewPointC;
  if (!Number.isFinite(depression) || depression < 0) {
    return { lclMslFt: null, lfcMslFt: null, parcelTempC: empty };
  }

  const surfaceZKm = surfaceMslFt / FEET_PER_METER / 1000;
  const lclAglFt = depression * M_PER_C_DEPRESSION * FEET_PER_METER;
  const lclMslFt = surfaceMslFt + lclAglFt;
  const lclZKm = lclMslFt / FEET_PER_METER / 1000;
  const lclTempC = surfaceTempC - DALR_C_PER_KM * (lclZKm - surfaceZKm);

  // Walk cells lifting a surface parcel: dry-adiabatic to LCL, then moist-
  // adiabatic above. LFC is the lowest cell above the LCL where the parcel
  // becomes warmer than the environment (positive buoyancy).
  let prevZKm = surfaceZKm;
  let parcelTempC = surfaceTempC;
  let lfcMslFt: number | null = null;
  const parcelTempProfile: (number | null)[] = [];

  for (const cell of cells) {
    const zKm = cell.mslFt / FEET_PER_METER / 1000;
    if (zKm <= prevZKm) {
      parcelTempProfile.push(null);
      continue;
    }

    if (zKm <= lclZKm) {
      parcelTempC = surfaceTempC - DALR_C_PER_KM * (zKm - surfaceZKm);
    } else {
      const startZKm = Math.max(prevZKm, lclZKm);
      const startT = prevZKm >= lclZKm ? parcelTempC : lclTempC;
      // Compute MALR at the parcel's temperature, not the environment's.
      // In the CAPE region the parcel is warmer than env → carries more water
      // vapor → has a smaller MALR → cools slower than env's MALR would imply.
      // Using cell.malrCPerKm here over-cools the parcel and shrinks CAPE/EL.
      const parcelMalr = computeMALR(startT, cell.hpa);
      parcelTempC = startT - parcelMalr * (zKm - startZKm);
    }

    parcelTempProfile.push(parcelTempC);

    if (lfcMslFt == null && zKm > lclZKm && parcelTempC > cell.temperature) {
      lfcMslFt = cell.mslFt;
    }

    prevZKm = zKm;
  }

  return { lclMslFt, lfcMslFt, parcelTempC: parcelTempProfile };
}

export function computeColumnParcelProfile(
  column: CloudColumn,
  surfaceMslFt: number,
): ParcelProfile {
  const lowest = column.cloud[0];
  if (!lowest) {
    return {
      lclMslFt: null,
      lfcMslFt: null,
      parcelTempC: column.cloud.map(() => null),
    };
  }
  const surfaceTempC = Number.isFinite(column.groundTemp)
    ? column.groundTemp
    : lowest.temperature;
  const surfaceDewPointC = Number.isFinite(column.groundDewPoint)
    ? column.groundDewPoint
    : lowest.dewPoint;
  return computeParcelProfile({
    surfaceTempC,
    surfaceDewPointC,
    surfaceMslFt,
    cells: column.cloud,
  });
}
