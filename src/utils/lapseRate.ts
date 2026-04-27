import { CloudCell } from "../types/weather";
import { FEET_PER_METER } from "../config/weather";

export const DALR_C_PER_KM = 9.8;
export const ISA_C_PER_KM = 6.5;

export type StabilityCategory =
  | "absolutely-stable"
  | "conditionally-unstable"
  | "absolutely-unstable";

// Saturated (moist) adiabatic lapse rate, °C/km.
// Standard formula using Bolton (1980) for saturation vapor pressure.
//   Γ_w = g * (1 + L r_s / (R_d T)) / (c_pd + L^2 r_s / (R_v T^2))
// where T is in Kelvin, p in hPa.
export function computeMALR(temperatureC: number, pressureHpa: number): number {
  const T = temperatureC + 273.15;
  const g = 9.81;
  const Rd = 287.05;
  const Rv = 461.5;
  const cp = 1004;
  const L = 2.501e6;

  const esHpa = 6.112 * Math.exp((17.67 * temperatureC) / (temperatureC + 243.5));
  const rs = (Rd / Rv) * (esHpa / Math.max(pressureHpa - esHpa, 1e-6));

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
  const dHKm = dHFt / FEET_PER_METER / 1000;
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

const STABILITY_COLORS: Record<StabilityCategory, string> = {
  "absolutely-stable": "rgba(34, 197, 94, 0.18)",
  "conditionally-unstable": "rgba(234, 179, 8, 0.22)",
  "absolutely-unstable": "rgba(239, 68, 68, 0.28)",
};

export function getStabilityColor(category: StabilityCategory): string {
  return STABILITY_COLORS[category];
}

export const STABILITY_LABELS: Record<StabilityCategory, string> = {
  "absolutely-stable": "Stable",
  "conditionally-unstable": "Conditionally unstable",
  "absolutely-unstable": "Absolutely unstable",
};
