import { CloudCell } from "../types/weather";
import { FEET_PER_METER } from "../config/weather";

export const DALR_C_PER_KM = 9.8;
export const ISA_C_PER_KM = 6.5;

const FT_PER_KM = FEET_PER_METER * 1000;
const KAPPA = 287.05 / 1004; // R_d / c_p (Poisson constant for dry air)
const L_VAPORIZATION = 2.501e6; // J/kg
const C_P = 1004; // J/(kg·K)

export const cPerKmToCPerKft = (cPerKm: number): number =>
  (cPerKm * 1000) / FT_PER_KM;

// Saturated (moist) adiabatic lapse rate, °C/km.
// Bolton (1980) saturation vapor pressure:
//   Γ_w = g * (1 + L r_s / (R_d T)) / (c_pd + L^2 r_s / (R_v T^2))
// where T is in Kelvin, p in hPa.
export function computeMALR(temperatureC: number, pressureHpa: number): number {
  const T = temperatureC + 273.15;
  const g = 9.81;
  const Rd = 287.05;
  const Rv = 461.5;
  // Saturation mixing ratio cap. Real atmospheric values stay well under 0.04
  // (≈30 °C at 1000 hPa); past that the parcel is so loaded with vapor that
  // the MALR formula is undefined, and a runaway rs would push the result
  // toward zero and misclassify stability.
  const RS_MAX = 0.04;

  const esHpa = 6.112 * Math.exp((17.67 * temperatureC) / (temperatureC + 243.5));
  const denomHpa = pressureHpa - esHpa;
  const rs =
    denomHpa > 0
      ? Math.min((Rd / Rv) * (esHpa / denomHpa), RS_MAX)
      : RS_MAX;

  const numerator = 1 + (L_VAPORIZATION * rs) / (Rd * T);
  const denominator =
    C_P + (L_VAPORIZATION * L_VAPORIZATION * rs) / (Rv * T * T);
  const gammaPerMeter = (g * numerator) / denominator;

  return gammaPerMeter * 1000;
}

// Environmental lapse rate between two cells, in °C/km.
// `lower` has higher pressure / lower altitude than `upper`.
export function computeELR(
  lower: Pick<CloudCell, "mslFt" | "temperature">,
  upper: Pick<CloudCell, "mslFt" | "temperature">,
): number | null {
  const dHFt = upper.mslFt - lower.mslFt;
  if (!Number.isFinite(dHFt) || dHFt <= 0) return null;
  const dHKm = dHFt / FT_PER_KM;
  const dT = lower.temperature - upper.temperature;
  return dT / dHKm;
}

// Potential temperature θ (K) — the temperature a parcel of given (T, p)
// would have if brought dry-adiabatically to 1000 hPa.
export function computeTheta(temperatureC: number, pressureHpa: number): number {
  return (temperatureC + 273.15) * Math.pow(1000 / pressureHpa, KAPPA);
}

// Equivalent potential temperature θe (K) — same as θ but accounting for the
// latent heat that would be released if all water vapor condensed. Bolton
// (1980) approximate form:  θe ≈ θ exp(L r / c_p T)  with r the actual
// (dewpoint-derived) mixing ratio.
export function computeThetaE(
  temperatureC: number,
  dewPointC: number,
  pressureHpa: number,
): number {
  const T = temperatureC + 273.15;
  const eHpa =
    6.112 * Math.exp((17.67 * dewPointC) / (dewPointC + 243.5));
  const denomHpa = Math.max(pressureHpa - eHpa, 1e-6);
  const r = (0.622 * eHpa) / denomHpa;
  const theta = computeTheta(temperatureC, pressureHpa);
  return theta * Math.exp((L_VAPORIZATION * r) / (C_P * T));
}

// Cell-level saturation flag: significant cloud cover or T-Td below ~1°C.
const SATURATION_DEW_POINT_DEPRESSION_C = 1;
const SATURATION_CLOUD_COVERAGE_PCT = 50;
export function isCellSaturated(cell: {
  cloudCoverage: number;
  temperature: number;
  dewPoint: number;
}): boolean {
  if (cell.cloudCoverage > SATURATION_CLOUD_COVERAGE_PCT) return true;
  const depression = cell.temperature - cell.dewPoint;
  return (
    Number.isFinite(depression) && depression < SATURATION_DEW_POINT_DEPRESSION_C
  );
}

// Continuous instability score, K/km — moist Brunt–Väisälä framing:
//   unsaturated layer: -dθ/dz   (a dry parcel cools at DALR)
//   saturated layer:   -dθe/dz  (a saturated parcel cools at MALR)
// Positive = unstable, negative = stable, near-zero = neutral. Using θe in
// unsaturated air would conflate "potential instability" (would be unstable
// upon lifting to saturation) with realized instability — the air isn't
// actually saturating right now, so the dry comparison is the honest one.
export function computeInstability(
  lower: Pick<
    CloudCell,
    "mslFt" | "temperature" | "dewPoint" | "hpa" | "cloudCoverage"
  >,
  upper: Pick<CloudCell, "mslFt" | "temperature" | "dewPoint" | "hpa">,
): number | null {
  const dHFt = upper.mslFt - lower.mslFt;
  if (!Number.isFinite(dHFt) || dHFt <= 0) return null;
  const dHKm = dHFt / FT_PER_KM;
  if (isCellSaturated(lower)) {
    const thetaELower = computeThetaE(
      lower.temperature,
      lower.dewPoint,
      lower.hpa,
    );
    const thetaEUpper = computeThetaE(
      upper.temperature,
      upper.dewPoint,
      upper.hpa,
    );
    return (thetaELower - thetaEUpper) / dHKm;
  }
  const thetaLower = computeTheta(lower.temperature, lower.hpa);
  const thetaUpper = computeTheta(upper.temperature, upper.hpa);
  return (thetaLower - thetaUpper) / dHKm;
}

// Continuous color for the instability score (K/km).
//   stable (negative)   → faint green, fading at neutrality
//   neutral (~0)        → no tint
//   unstable (positive) → yellow→red ramp, alpha grows with magnitude
//
// Asymmetric clamps reflect that strong instability (>5 K/km) is the
// actionable signal and should pop visually, while strong stability (<-10
// K/km) is the common case and reads quieter.
const INSTABILITY_NEUTRAL_DEADBAND = 1;
const INSTABILITY_UNSTABLE_CLAMP = 10;
const INSTABILITY_STABLE_CLAMP = 15;

export function getInstabilityColor(scoreKPerKm: number): string | null {
  if (!Number.isFinite(scoreKPerKm)) return null;
  if (Math.abs(scoreKPerKm) < INSTABILITY_NEUTRAL_DEADBAND) return null;
  if (scoreKPerKm > 0) {
    const t = Math.min(scoreKPerKm / INSTABILITY_UNSTABLE_CLAMP, 1);
    // Yellow (234,179,8) → red (239,68,68) interpolation.
    const r = Math.round(234 + (239 - 234) * t);
    const g = Math.round(179 + (68 - 179) * t);
    const b = Math.round(8 + (68 - 8) * t);
    const alpha = 0.18 + 0.22 * t;
    return `rgba(${r}, ${g}, ${b}, ${alpha.toFixed(3)})`;
  }
  const t = Math.min(-scoreKPerKm / INSTABILITY_STABLE_CLAMP, 1);
  const alpha = 0.1 + 0.2 * t;
  return `rgba(34, 197, 94, ${alpha.toFixed(3)})`;
}

export function getInstabilityLabel(scoreKPerKm: number): string {
  if (!Number.isFinite(scoreKPerKm)) return "—";
  if (scoreKPerKm > 5) return "Strongly unstable";
  if (scoreKPerKm > INSTABILITY_NEUTRAL_DEADBAND) return "Unstable";
  if (scoreKPerKm < -5) return "Strongly stable";
  if (scoreKPerKm < -INSTABILITY_NEUTRAL_DEADBAND) return "Stable";
  return "Neutral";
}

// Parcel buoyancy tint: positive (parcel warmer than env, CAPE) → red,
// negative (parcel colder than env, CIN) → blue. Magnitude clamped at 4 °C
// for opacity scaling so a typical CAPE region reads as a saturated tint.
const BUOYANCY_CLAMP_C = 4;
const BUOYANCY_MAX_ALPHA = 0.45;

export function getBuoyancyColor(buoyancyC: number): string | null {
  if (!Number.isFinite(buoyancyC) || buoyancyC === 0) return null;
  const magnitude = Math.min(Math.abs(buoyancyC) / BUOYANCY_CLAMP_C, 1);
  const alpha = magnitude * BUOYANCY_MAX_ALPHA;
  if (alpha < 0.04) return null;
  return buoyancyC > 0
    ? `rgba(239, 68, 68, ${alpha.toFixed(3)})`
    : `rgba(59, 130, 246, ${alpha.toFixed(3)})`;
}
