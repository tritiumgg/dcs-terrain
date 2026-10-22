//! The closed set of siting criteria. Each has a hard test, which a site
//! passes or fails, and a soft score from 0 to 1, where 1 is best.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Observer, Point3};

/// One criterion, written `{"name": "slope_max", "params": {"deg": 8}}`.
/// `valid` takes no params and is applied whether asked for or not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "name", content = "params", rename_all = "snake_case")]
pub enum Criterion {
    /// Steepest slope over the footprint, at most `deg`.
    SlopeMax {
        deg: f64,
    },
    /// Highest minus lowest height over the footprint's corners and centre,
    /// at most `m`.
    SpanMax {
        m: f64,
    },
    /// Every footprint cell's water class is one of `classes`.
    SurfaceIn {
        classes: Vec<WaterClass>,
    },
    /// Every footprint cell's `surface` code is one of `values`.
    SurfaceEnumIn {
        values: Vec<u8>,
    },
    DistRoadBand {
        min: f64,
        max: f64,
    },
    DistRailBand {
        min: f64,
        max: f64,
    },
    DistWaterMin {
        m: f64,
    },
    /// Distance to the nearest airdrome, or to `airdrome_id` when given.
    DistAirdromeBand {
        min: f64,
        max: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        airdrome_id: Option<i64>,
    },
    /// Scenery objects of `class` within `radius`, at most `max`.
    SceneryMax {
        class: String,
        radius: f64,
        max: u32,
    },
    /// Scenery objects overlapping the footprint, at most `max`.
    SceneryInFootprintMax {
        max: u32,
    },
    /// Objects of any class within `radius`: what a heliport static would
    /// remove at spawn.
    ClearingMax {
        #[serde(default = "default_clearing_radius")]
        radius: f64,
        max: u32,
    },
    EnclosureMin {
        deg: f64,
    },
    /// A ridge that way: every horizon bin in the sector rises at least
    /// `min_elev_deg`.
    MaskedFrom {
        bearing_deg: f64,
        half_width_deg: f64,
        min_elev_deg: f64,
    },
    /// The fraction of `observers` that can see the site at `own_alt_agl`,
    /// at most `max_fraction`.
    NotVisibleFrom {
        observers: Vec<Observer>,
        max_fraction: f64,
        #[serde(default = "default_own_alt_agl")]
        own_alt_agl: f64,
    },
    /// The fraction of `targets` the site sees from `own_alt_agl`, at least
    /// `min_fraction`.
    VisibleTo {
        targets: Vec<Point3>,
        min_fraction: f64,
        #[serde(default = "default_own_alt_agl")]
        own_alt_agl: f64,
    },
    /// The site's landform class is one of `classes`, judged at
    /// `threshold_m` of local and regional relief.
    TpiClassIn {
        classes: Vec<TpiClass>,
        #[serde(default = "default_tpi_threshold")]
        threshold_m: f64,
    },
    Valid,
}

fn default_clearing_radius() -> f64 {
    150.0
}

fn default_own_alt_agl() -> f64 {
    2.0
}

fn default_tpi_threshold() -> f64 {
    10.0
}

/// The `water` layer's classes by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WaterClass {
    Land,
    Lake,
    Sea,
    River,
}

/// A landform class from local relief, then regional relief, then slope:
/// the first that holds of `ridge`, `valley`, `upper`, `lower`, `mid`, and
/// `flat` otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TpiClass {
    Ridge,
    Valley,
    Upper,
    Lower,
    Mid,
    Flat,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_criterion_is_a_name_and_its_params() {
        let c: Criterion =
            serde_json::from_value(json!({"name": "slope_max", "params": {"deg": 8}})).unwrap();
        assert_eq!(c, Criterion::SlopeMax { deg: 8.0 });
        assert_eq!(
            serde_json::to_value(&c).unwrap(),
            json!({"name": "slope_max", "params": {"deg": 8.0}})
        );
    }

    #[test]
    fn valid_takes_no_params() {
        let c: Criterion = serde_json::from_value(json!({"name": "valid"})).unwrap();
        assert_eq!(c, Criterion::Valid);
    }

    #[test]
    fn omitted_params_take_their_defaults() {
        let c: Criterion =
            serde_json::from_value(json!({"name": "clearing_max", "params": {"max": 3}})).unwrap();
        assert_eq!(
            c,
            Criterion::ClearingMax {
                radius: 150.0,
                max: 3
            }
        );
        let c: Criterion = serde_json::from_value(json!({
            "name": "tpi_class_in", "params": {"classes": ["ridge", "upper"]}
        }))
        .unwrap();
        assert_eq!(
            c,
            Criterion::TpiClassIn {
                classes: vec![TpiClass::Ridge, TpiClass::Upper],
                threshold_m: 10.0
            }
        );
        let c: Criterion = serde_json::from_value(json!({
            "name": "not_visible_from", "params": {"observers": [], "max_fraction": 0.1}
        }))
        .unwrap();
        assert!(matches!(c, Criterion::NotVisibleFrom { own_alt_agl, .. } if own_alt_agl == 2.0));
    }

    #[test]
    fn an_unknown_criterion_is_refused() {
        let r = serde_json::from_value::<Criterion>(json!({"name": "slope_min", "params": {}}));
        assert!(r.is_err());
    }
}
