//! HRRR cloud coordinates reconstructed from the already retained atmosphere.
//!
//! Coverage is always the exact source Float32. Geometry is an estimate: one
//! native bottom-pressure map anchors the hybrid coordinate, and one native
//! top-height map bounds interpolation above the retained pressure profiles.
use anyhow::{Context, Result};
use serde_json::{json, Value};

pub const METHOD: &str = "hrrr_hybrid_moist_log_pressure_v1";
pub const TOP_PA: f64 = 1500.;
const REFERENCE_PA: f64 = 100000.;
const CRITICAL_ETA: f64 = 0.2;
// NOAA HRRR's 51 interfaces define 50 mass/cloud levels:
// https://rapidrefresh.noaa.gov/faq/HRRR.faq.html
const ETA: [f64; 51] = [
    1., 0.998, 0.994, 0.987, 0.975, 0.959, 0.939, 0.916, 0.892, 0.865, 0.835, 0.802, 0.766, 0.727,
    0.685, 0.640, 0.592, 0.542, 0.497, 0.4565, 0.4205, 0.3877, 0.3582, 0.3317, 0.3078, 0.2863,
    0.2670, 0.2496, 0.2329, 0.2188, 0.2047, 0.1906, 0.1765, 0.1624, 0.1483, 0.1342, 0.1201, 0.1060,
    0.0919, 0.0778, 0.0657, 0.0568, 0.0486, 0.0409, 0.0337, 0.0271, 0.0209, 0.0151, 0.0097, 0.0047,
    0.,
];

pub fn description() -> Value {
    json!({"method":METHOD,"coverage":"exact_source_float32","pressure":"hybrid_coordinate_with_profile_moisture","height":"log_pressure_interpolation","anchor_fields":["model_level_1_pressure","model_level_50_geopotential_height"],"model_top_pa":TOP_PA,"reference_pressure_pa":REFERENCE_PA,"critical_eta":CRITICAL_ETA,"eta_interfaces":ETA.to_vec()})
}

fn weight(eta: f64) -> f64 {
    if eta < CRITICAL_ETA {
        return 0.;
    }
    // WRF hybrid_opt=2, full-level coefficients averaged to mass levels.
    // https://github.com/wrf-model/WRF/blob/master/dyn_em/module_initialize_ideal.F
    let e = CRITICAL_ETA;
    (2. * e * e * (1. - e) - e * (4. - 3. * e - e.powi(3)) * eta
        + 2. * (1. - e.powi(3)) * eta * eta
        - (1. - e * e) * eta.powi(3))
        / (1. - e).powi(4)
}

fn coordinate(level: usize) -> (f64, f64) {
    (
        (ETA[level] + ETA[level + 1]) * 0.5,
        (weight(ETA[level]) + weight(ETA[level + 1])) * 0.5,
    )
}

pub fn top_pressure() -> f64 {
    let (eta, b) = coordinate(49);
    assert_eq!(b, 0.);
    TOP_PA + eta * (REFERENCE_PA - TOP_PA)
}

fn interpolate(target: f64, x: &[f64], y: &[f64], logarithmic: bool) -> f64 {
    let k = x
        .partition_point(|v| *v > target)
        .saturating_sub(1)
        .min(x.len() - 2);
    let ratio = if logarithmic {
        (target / x[k]).ln() / (x[k + 1] / x[k]).ln()
    } else {
        (target - x[k]) / (x[k + 1] - x[k])
    };
    y[k] + ratio * (y[k + 1] - y[k])
}

/// The two anchors are native level-1 pressure (Pa) and level-50 height (m).
/// All remaining inputs are the point's existing 30-level pressure forecast.
pub fn derive(
    anchors: [f32; 2],
    levels_hpa: &[f64],
    heights: &[f64],
    dewpoints: &[f64],
) -> Result<[Vec<f32>; 2]> {
    let n = levels_hpa.len();
    anyhow::ensure!(
        n == 30
            && heights.len() == n
            && dewpoints.len() == n
            && levels_hpa
                .iter()
                .enumerate()
                .all(|(i, p)| *p == 1000. - i as f64 * 25.)
            && heights.iter().all(|v| v.is_finite())
            && heights.windows(2).all(|v| v[1] > v[0])
            && anchors.iter().all(|v| v.is_finite())
            && anchors[0] > 10000.
            && anchors[0] < 120000.
            && f64::from(anchors[1]) > heights[n - 1],
        "Invalid cloud geometry anchors/atmosphere"
    );
    let mut pressure: Vec<_> = levels_hpa.iter().map(|p| p * 100.).collect();
    let mut q = Vec::with_capacity(n);
    for (td, p) in dewpoints.iter().zip(&pressure) {
        anyhow::ensure!(
            td.is_finite() && *td > -200. && *td < 100.,
            "Invalid dew point for cloud geometry"
        );
        // Bolton (1980) saturation over water; dew point, rather than RH,
        // avoids introducing another mixed-phase relative-humidity convention.
        let e = 611.2 * (17.67 * td / (td + 243.5)).exp();
        let specific = 0.622 * e / (p - 0.378 * e);
        anyhow::ensure!(
            specific.is_finite() && (0. ..0.1).contains(&specific),
            "Invalid profile moisture for cloud geometry"
        );
        q.push(specific);
    }
    // Integral of specific humidity from the model top downwards, in Pa.
    // Above 275 hPa, taper the small remaining humidity to zero at p=0.
    let mut water = vec![0.; n];
    water[n - 1] = 0.5 * q[n - 1] * pressure[n - 1];
    for i in (0..n - 1).rev() {
        water[i] = water[i + 1] + 0.5 * (q[i] + q[i + 1]) * (pressure[i] - pressure[i + 1]);
    }
    let bottom = f64::from(anchors[0]);
    let dry_bottom = bottom - interpolate(bottom, &pressure, &water, false);
    let mut dry: Vec<_> = pressure.iter().zip(&water).map(|(p, w)| p - w).collect();
    pressure.push(0.);
    dry.push(0.);
    anyhow::ensure!(
        dry.windows(2).all(|v| v[0] > v[1]),
        "Nonmonotonic dry-pressure profile"
    );
    let (eta0, b0) = coordinate(0);
    let mass = (dry_bottom - (eta0 - b0) * (REFERENCE_PA - TOP_PA) - TOP_PA) / b0;
    anyhow::ensure!(mass.is_finite() && mass > 0., "Invalid hybrid column mass");
    let mut native_pressure = Vec::with_capacity(50);
    for l in 0..50 {
        let (eta, b) = coordinate(l);
        let pd = b * mass + (eta - b) * (REFERENCE_PA - TOP_PA) + TOP_PA;
        let p = if l == 0 {
            bottom
        } else {
            interpolate(pd, &dry, &pressure, false)
        };
        native_pressure.push(p as f32);
    }
    let last = n - 1;
    let top_pressure = f64::from(native_pressure[49]);
    let height = native_pressure
        .iter()
        .enumerate()
        .map(|(l, p)| {
            let p = f64::from(*p);
            if l == 49 {
                anchors[1]
            } else if p >= pressure[last] {
                interpolate(p, &pressure[..n], heights, true) as f32
            } else {
                interpolate(
                    p,
                    &[pressure[last], top_pressure],
                    &[heights[last], f64::from(anchors[1])],
                    true,
                ) as f32
            }
        })
        .collect::<Vec<_>>();
    anyhow::ensure!(
        native_pressure.iter().all(|p| p.is_finite() && *p > 0.)
            && native_pressure.windows(2).all(|v| v[0] > v[1])
            && height.iter().all(|h| h.is_finite())
            && height.windows(2).all(|v| v[1] > v[0]),
        "Invalid derived cloud geometry"
    );
    Ok([height, native_pressure])
}

pub fn from_payload(payload: &Value, lead: u16, anchors: [f32; 2]) -> Result<[Vec<f32>; 2]> {
    let fields = payload["fields"].as_array().context("Atmospheric fields")?;
    let rows = payload["rows"].as_array().context("Atmospheric rows")?;
    let row = rows
        .iter()
        .find(|row| row[0].as_u64() == Some(u64::from(lead)))
        .context("Cloud hour lacks atmospheric profile")?;
    let values = |name: &str| -> Result<Vec<f64>> {
        let index = fields
            .iter()
            .position(|f| f == name)
            .context("Geometry field")?;
        row[index]
            .as_array()
            .context("Geometry field width")?
            .iter()
            .map(|v| v.as_f64().context("Geometry field value"))
            .collect()
    };
    let levels: Vec<f64> = serde_json::from_value(payload["levels_hpa"].clone())?;
    derive(
        anchors,
        &levels,
        &values("geopotential_height")?,
        &values("dew_point_temperature")?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dry_isothermal_column_preserves_anchor_and_monotonic_levels() {
        let levels: Vec<_> = (0..30).map(|i| 1000. - f64::from(i) * 25.).collect();
        let height: Vec<_> = levels.iter().map(|p| 8000. * (1000. / p).ln()).collect();
        let top = (8000. * (100000. / top_pressure()).ln()) as f32;
        let [h, p] = derive([99901.5, top], &levels, &height, &[-150.; 30]).unwrap();
        assert_eq!(p[0].to_bits(), 99901.5f32.to_bits());
        assert_eq!(h[49].to_bits(), top.to_bits());
        assert!((f64::from(p[49]) - 1731.475).abs() < 0.01);
        for (h, p) in h.iter().zip(&p) {
            assert!((f64::from(*h) - 8000. * (100000. / f64::from(*p)).ln()).abs() < 0.01);
        }
        assert!(derive([99901.5, 10.], &levels, &height, &[-150.; 30]).is_err());
    }
}
