import { CloudCell, CloudColumn, ParcelMode, PARCEL_MODES } from "../types/weather";
import { FEET_PER_METER } from "../config/weather";
import { DALR_C_PER_KM, computeMALR } from "./lapseRate";

export { PARCEL_MODES };
export type { ParcelMode };

// Poisson constant R_d / c_p for dry air.
const KAPPA = 287.05 / 1004;

// Bolton (1980) saturation vapor pressure, hPa.
function esHpa(tempC: number): number {
  return 6.112 * Math.exp((17.67 * tempC) / (tempC + 243.5));
}

// Water vapor mixing ratio (kg/kg) from dewpoint and pressure.
function mixingRatio(dewPointC: number, pressureHpa: number): number {
  const e = esHpa(dewPointC);
  const denom = Math.max(pressureHpa - e, 1e-6);
  return (0.622 * e) / denom;
}

// Inverse — dewpoint from mixing ratio and pressure.
function dewPointFromMixingRatio(r: number, pressureHpa: number): number {
  const e = (r * pressureHpa) / (0.622 + r);
  const ln = Math.log(Math.max(e, 1e-6) / 6.112);
  return (243.5 * ln) / (17.67 - ln);
}

// Potential temperature (K).
function potentialTempK(tempC: number, pressureHpa: number): number {
  return (tempC + 273.15) * Math.pow(1000 / pressureHpa, KAPPA);
}

// Temperature (°C) at a given pressure for a parcel of given θ.
function tempFromPotentialTempK(thetaK: number, pressureHpa: number): number {
  return thetaK * Math.pow(pressureHpa / 1000, KAPPA) - 273.15;
}

// Espy approximation: LCL height above ground ≈ 125 m per °C of dewpoint
// depression. Captures the dry-adiabatic cooling rate (≈9.8 °C/km) closing
// against the dewpoint depression decrease rate (≈1.8 °C/km).
const M_PER_C_DEPRESSION = 125;

export const PARCEL_MODE_LABELS: Record<ParcelMode, string> = {
  surface: "Surface",
  "mixed-100": "Mixed-Layer 100hPa",
  "most-unstable": "Most-Unstable",
};

const MIXED_LAYER_DEPTH_HPA = 100;
const MOST_UNSTABLE_SEARCH_DEPTH_HPA = 300;

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
    // Cells strictly below the surface have no parcel value. A cell at
    // exactly the surface altitude gets the surface T (DALR formula at
    // zKm == surfaceZKm naturally yields surfaceTempC).
    if (zKm < prevZKm) {
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

function emptyProfile(cells: CloudCell[]): ParcelProfile {
  return {
    lclMslFt: null,
    lfcMslFt: null,
    parcelTempC: cells.map(() => null),
  };
}

// Ranking-only proxy for CAPE: sum of positive parcel-minus-env over altitude.
// Good enough to pick the most-buoyant starting parcel; not a calibrated J/kg.
function capeProxy(profile: ParcelProfile, cells: CloudCell[]): number {
  let cape = 0;
  for (let i = 0; i < cells.length; i++) {
    const pt = profile.parcelTempC[i];
    if (pt == null) continue;
    const buoyancy = pt - cells[i].temperature;
    if (buoyancy > 0) cape += buoyancy;
  }
  return cape;
}

function mean(xs: number[]): number {
  return xs.reduce((s, x) => s + x, 0) / xs.length;
}

export function computeColumnParcelProfile(
  column: CloudColumn,
  surfaceMslFt: number,
  mode: ParcelMode = "surface",
): ParcelProfile {
  const cells = column.cloud;
  const lowest = cells[0];
  if (!lowest) return emptyProfile(cells);

  if (mode === "surface") {
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
      cells,
    });
  }

  if (mode === "mixed-100") {
    // ML-CAPE: average potential temperature θ and water vapor mixing ratio
    // r over the lowest 100 hPa, then back-project to the surface pressure.
    // Averaging raw T/Td would systematically under-warm the parcel because
    // temperature drops with altitude inside the layer.
    const surfaceHpa = lowest.hpa;
    const mlCells = cells.filter(
      (c) => c.hpa >= surfaceHpa - MIXED_LAYER_DEPTH_HPA,
    );
    if (mlCells.length === 0) return emptyProfile(cells);
    const avgThetaK = mean(
      mlCells.map((c) => potentialTempK(c.temperature, c.hpa)),
    );
    const avgR = mean(mlCells.map((c) => mixingRatio(c.dewPoint, c.hpa)));
    return computeParcelProfile({
      surfaceTempC: tempFromPotentialTempK(avgThetaK, surfaceHpa),
      surfaceDewPointC: dewPointFromMixingRatio(avgR, surfaceHpa),
      surfaceMslFt,
      cells,
    });
  }

  // most-unstable: try each cell in the lowest 300hPa as the starting
  // parcel and keep the one with the largest CAPE proxy. Surfaces above
  // the elevation are launched from their own MSL.
  const surfaceHpa = lowest.hpa;
  const candidates = cells.filter(
    (c) => c.hpa >= surfaceHpa - MOST_UNSTABLE_SEARCH_DEPTH_HPA,
  );
  let bestProfile: ParcelProfile | null = null;
  let bestCape = -Infinity;
  for (const candidate of candidates) {
    const profile = computeParcelProfile({
      surfaceTempC: candidate.temperature,
      surfaceDewPointC: candidate.dewPoint,
      surfaceMslFt: candidate.mslFt,
      cells,
    });
    const cape = capeProxy(profile, cells);
    if (cape > bestCape) {
      bestCape = cape;
      bestProfile = profile;
    }
  }
  return bestProfile ?? emptyProfile(cells);
}
