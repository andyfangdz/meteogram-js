import { CloudCell } from "../types/weather";
import { FEET_PER_METER } from "../config/weather";

export const DALR_C_PER_KM = 9.8;
export const ISA_C_PER_KM = 6.5;

const FT_PER_KM = FEET_PER_METER * 1000;

export const cPerKmToCPerKft = (cPerKm: number): number =>
  (cPerKm * 1000) / FT_PER_KM;

export type StabilityCategory =
  | "absolutely-stable"
  | "conditionally-unstable"
  | "absolutely-unstable";

// Saturated (moist) adiabatic lapse rate, °C/km.
// Bolton (1980) saturation vapor pressure:
//   Γ_w = g * (1 + L r_s / (R_d T)) / (c_pd + L^2 r_s / (R_v T^2))
// where T is in Kelvin, p in hPa.
export function computeMALR(temperatureC: number, pressureHpa: number): number {
  const T = temperatureC + 273.15;
  const g = 9.81;
  const Rd = 287.05;
  const Rv = 461.5;
  const cp = 1004;
  const L = 2.501e6;
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

  const numerator = 1 + (L * rs) / (Rd * T);
  const denominator = cp + (L * L * rs) / (Rv * T * T);
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

export function getStabilityCategory(
  elrCPerKm: number,
  malrCPerKm: number,
): StabilityCategory {
  if (elrCPerKm >= DALR_C_PER_KM) return "absolutely-unstable";
  if (elrCPerKm <= malrCPerKm) return "absolutely-stable";
  return "conditionally-unstable";
}

const STABILITY_RGB: Record<StabilityCategory, [number, number, number]> = {
  "absolutely-stable": [34, 197, 94],
  "conditionally-unstable": [234, 179, 8],
  "absolutely-unstable": [239, 68, 68],
};

const STABILITY_DEFAULT_ALPHA: Record<StabilityCategory, number> = {
  "absolutely-stable": 0.28,
  "conditionally-unstable": 0.32,
  "absolutely-unstable": 0.38,
};

export function getStabilityColor(
  category: StabilityCategory,
  alpha?: number,
): string {
  const [r, g, b] = STABILITY_RGB[category];
  const a = alpha ?? STABILITY_DEFAULT_ALPHA[category];
  return `rgba(${r}, ${g}, ${b}, ${a})`;
}

export const STABILITY_LABELS: Record<StabilityCategory, string> = {
  "absolutely-stable": "Stable",
  "conditionally-unstable": "Conditionally unstable",
  "absolutely-unstable": "Absolutely unstable",
};
