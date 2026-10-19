use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};
#[derive(Clone, Debug)]
pub struct Lambert {
    r: f64,
    n: f64,
    f: f64,
    rho0: f64,
    lon0: f64,
    x0: f64,
    y0: f64,
    lat0: f64,
    p1: f64,
    p2: f64,
}
impl Lambert {
    pub fn from_crs(v: &Value) -> Result<Self> {
        let r = v["base_crs"]["datum"]["ellipsoid"]["radius"]
            .as_f64()
            .context("Expected spherical native projection")?;
        let ps = v["conversion"]["parameters"]
            .as_array()
            .context("Missing projection parameters")?;
        let p = |code: u64| -> Result<f64> {
            ps.iter()
                .find(|p| p["id"]["code"] == code)
                .and_then(|p| p["value"].as_f64())
                .context("Missing LCC parameter")
        };
        Self::new(
            r,
            p(8821)?,
            p(8822)?,
            p(8823)?,
            p(8824)?,
            p(8826)?,
            p(8827)?,
        )
    }
    pub fn from_cf(v: &Value) -> Result<Self> {
        let number = |s: &str| -> Result<f64> { v[s].as_f64().context(format!("Missing CRS {s}")) };
        let parallels = &v["standard_parallel"];
        let (p1, p2) = if let Some(a) = parallels.as_array() {
            anyhow::ensure!(!a.is_empty(), "Missing standard parallels");
            (
                a[0].as_f64().context("parallel")?,
                a.last().unwrap().as_f64().context("parallel")?,
            )
        } else {
            let p = number("standard_parallel")?;
            (p, p)
        };
        anyhow::ensure!(
            v["inverse_flattening"].as_f64().unwrap_or(0.) == 0.,
            "Only spherical native LCC is supported"
        );
        Self::new(
            v["earth_radius"]
                .as_f64()
                .or_else(|| v["semi_major_axis"].as_f64())
                .context("Missing sphere radius")?,
            number("latitude_of_projection_origin")?,
            number("longitude_of_central_meridian")?,
            p1,
            p2,
            v["false_easting"].as_f64().unwrap_or(0.),
            v["false_northing"].as_f64().unwrap_or(0.),
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub fn new(r: f64, lat0: f64, lon0: f64, p1: f64, p2: f64, x0: f64, y0: f64) -> Result<Self> {
        let (a, b) = (p1.to_radians(), p2.to_radians());
        let n = if (a - b).abs() < 1e-12 {
            a.sin()
        } else {
            (a.cos() / b.cos()).ln()
                / ((FRAC_PI_4 + b / 2.).tan() / (FRAC_PI_4 + a / 2.).tan()).ln()
        };
        if !n.is_finite() || n == 0. || r <= 0. {
            bail!("Invalid LCC");
        }
        let f = a.cos() * (FRAC_PI_4 + a / 2.).tan().powf(n) / n;
        let rho0 = r * f / (FRAC_PI_4 + lat0.to_radians() / 2.).tan().powf(n);
        Ok(Self {
            r,
            n,
            f,
            rho0,
            lon0: lon0.to_radians(),
            x0,
            y0,
            lat0,
            p1,
            p2,
        })
    }
    pub fn forward(&self, lat: f64, lon: f64) -> (f64, f64) {
        let rho = self.r * self.f / (FRAC_PI_4 + lat.to_radians() / 2.).tan().powf(self.n);
        let theta = self.n
            * ((lon.to_radians() - self.lon0 + std::f64::consts::PI)
                .rem_euclid(2. * std::f64::consts::PI)
                - std::f64::consts::PI);
        (
            self.x0 + rho * theta.sin(),
            self.y0 + self.rho0 - rho * theta.cos(),
        )
    }
    /// Clockwise rotation from native grid bearings to true bearings.
    pub fn convergence_degrees(&self, longitude: f64) -> f64 {
        self.n * ((longitude - self.lon0.to_degrees() + 180.).rem_euclid(360.) - 180.)
    }
    pub fn inverse(&self, x: f64, y: f64) -> (f64, f64) {
        let (x, y) = (x - self.x0, self.rho0 - (y - self.y0));
        let rho = x.hypot(y) * self.n.signum();
        let theta = (x * self.n.signum()).atan2(y * self.n.signum());
        let lat = 2. * (self.r * self.f / rho).powf(1. / self.n).atan() - FRAC_PI_2;
        let lon = self.lon0 + theta / self.n;
        (
            lat.to_degrees(),
            (lon.to_degrees() + 180.).rem_euclid(360.) - 180.,
        )
    }
    pub fn crs(&self) -> Value {
        let params = [
            (8821, "Latitude of false origin", self.lat0),
            (8822, "Longitude of false origin", self.lon0.to_degrees()),
            (8823, "Latitude of 1st standard parallel", self.p1),
            (8824, "Latitude of 2nd standard parallel", self.p2),
            (8826, "Easting at false origin", self.x0),
            (8827, "Northing at false origin", self.y0),
        ]
        .into_iter()
        .map(|(c, name, v)| json!({"name":name,"unit":if c<8826{"degree"}else{"metre"},"id":{"authority":"EPSG","code":c},"value":v}))
        .collect::<Vec<_>>();
        json!({"$schema":"https://proj.org/schemas/v0.7/projjson.schema.json","type":"ProjectedCRS","name":"Native spherical LCC","base_crs":{"name":"Native GRIB geographic CRS","datum":{"type":"GeodeticReferenceFrame","name":"Native sphere","ellipsoid":{"name":"Sphere","radius":self.r}},"coordinate_system":{"subtype":"ellipsoidal","axis":[{"name":"Longitude","abbreviation":"lon","direction":"east","unit":"degree"},{"name":"Latitude","abbreviation":"lat","direction":"north","unit":"degree"}]}},"conversion":{"name":"Native LCC","method":{"name":"Lambert Conic Conformal (2SP)","id":{"authority":"EPSG","code":9802}},"parameters":params},"coordinate_system":{"subtype":"Cartesian","axis":[{"name":"Easting","abbreviation":"x","direction":"east","unit":"metre"},{"name":"Northing","abbreviation":"y","direction":"north","unit":"metre"}]}})
    }
}
pub fn nearest(a: &[f64], v: f64, circular: bool) -> usize {
    a.iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let d = |x: f64| {
                if circular {
                    ((x - v + 180.).rem_euclid(360.) - 180.).abs()
                } else {
                    (x - v).abs()
                }
            };
            d(**a).total_cmp(&d(**b))
        })
        .map(|(i, _)| i)
        .unwrap_or(0)
}
pub fn numbers(v: &Value) -> Result<Vec<f64>> {
    v.as_array()
        .context("Missing grid coordinate")?
        .iter()
        .map(|x| x.as_f64().context("Invalid coordinate"))
        .collect()
}
pub fn resolve(m: &Value, ids: &[u32], lat: f64, lon: f64) -> Result<(u32, usize)> {
    if !(24. ..=50.).contains(&lat) || !(-125. ..=-66.).contains(&lon) {
        bail!("Outside the retained region");
    }
    let g = &m["grid"];
    let cell = if g["kind"] == "projected" {
        let proj = Lambert::from_crs(&g["crs"])?;
        let (x, y) = proj.forward(lat, lon);
        let xs = numbers(&g["x"])?;
        let ys = numbers(&g["y"])?;
        let inside = |a: &[f64], v: f64| {
            v >= a.iter().copied().fold(f64::INFINITY, f64::min) - 1e-6
                && v <= a.iter().copied().fold(f64::NEG_INFINITY, f64::max) + 1e-6
        };
        if !inside(&xs, x) || !inside(&ys, y) {
            bail!("Outside the native model domain");
        }
        (nearest(&ys, y, false) * xs.len() + nearest(&xs, x, false)) as u32
    } else {
        let ys = numbers(&g["latitude"])?;
        let xs = numbers(&g["longitude"])?;
        (nearest(&ys, lat, false) * xs.len() + nearest(&xs, lon, true)) as u32
    };
    let i = ids
        .binary_search(&cell)
        .map_err(|_| anyhow::anyhow!("Location was not ingested"))?;
    Ok((cell, i))
}
