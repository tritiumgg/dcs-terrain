//! The request and response types every surface shares.
//!
//! An operation is one function in core and one `dcsterrain query` request,
//! and both take and return these structs. Keeping them in one place is what
//! stops the schemas drifting apart; `schema` prints them, and a snapshot
//! test makes any change to them a deliberate diff.
//!
//! Lengths are metres and angles degrees. Radians never cross this boundary.
//! Positions are DCS `x` (north) and `z` (east).

mod criteria;
mod ops;
mod schema;

pub use criteria::*;
pub use ops::*;
pub use schema::{ISSUE_CODES, OPERATIONS, Operation, UnknownOperation, schema};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// An axis-aligned box in DCS metres, the canonical region.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename = "Box")]
pub struct BoundingBox {
    pub min_x: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_z: f64,
}

/// A region a request is bounded by. Each form is converted to its bounding
/// box plus a membership test before any grid is read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum Region {
    Box(BoundingBox),
    Circle {
        x: f64,
        z: f64,
        radius: f64,
    },
    Polygon(Polygon),
    /// The ring between two distances from an airdrome's reference point.
    AirdromeBand {
        airdrome: AirdromeRef,
        min_dist: f64,
        max_dist: f64,
    },
}

/// An airdrome as a request names it: a number is its id; a string is its
/// `name_id` or, where no `name_id` matches, its id written in digits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum AirdromeRef {
    Id(i64),
    Name(String),
}

/// A simple polygon as `[x, z]` vertices, closed by its first point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Polygon {
    pub points: Vec<[f64; 2]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Point {
    pub x: f64,
    pub z: f64,
}

/// A point with a height, given either absolute or above the ground.
///
/// Exactly one of `alt` and `alt_agl` is set; `Point3::check` reports both or
/// neither as an issue. The pair is two fields rather than an enum so the JSON
/// reads `{"x": .., "z": .., "alt_agl": 2}`, the way a caller writes it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Point3 {
    pub x: f64,
    pub z: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt_agl: Option<f64>,
}

impl Point3 {
    /// The issue with this point's height, if it has one. `path` names the
    /// point in the request, so the caller can find it.
    pub fn check(&self, path: &str) -> Option<Issue> {
        match (self.alt, self.alt_agl) {
            (Some(_), None) | (None, Some(_)) => None,
            (Some(_), Some(_)) => Some(Issue::new(
                "point3_two_heights",
                "set alt or alt_agl, not both",
                path,
            )),
            (None, None) => Some(Issue::new(
                "point3_no_height",
                "set one of alt or alt_agl",
                path,
            )),
        }
    }
}

/// Someone looking: a position and an eye height above the ground.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Observer {
    pub x: f64,
    pub z: f64,
    pub alt_agl: f64,
}

/// A rectangle to be placed, tried at each of its orientations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Footprint {
    pub width: f64,
    pub depth: f64,
    #[serde(default = "default_orientations")]
    pub orientations_deg: Vec<f64>,
}

fn default_orientations() -> Vec<f64> {
    vec![0.0, 90.0]
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

// What answered: carried by every response, so a consumer can always tell
// which terrain, build and resolution a result came from. A plain comment
// rather than a doc comment, because the schema of a struct that flattens
// this one and has no doc of its own would take this one's as its own.
//
// The build is the terrain version, since ED ships terrain only inside
// updates; the fingerprint, the terrain files' sizes, records what answered
// and needs no hash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Provenance {
    pub terrain: String,
    pub dcs_build: String,
    pub terrain_fingerprint: String,
    pub cell_size: f64,
    pub elapsed_ms: u64,
}

/// One thing wrong with a request or with the file it was asked of. An
/// operation returns a full result or a list of these, never a partial.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Issue {
    /// Stable, machine-readable: `snake_case`, never reworded once shipped.
    pub code: String,
    pub message: String,
    /// Where in the request, dotted: `criteria.2.params.deg`. Empty when the
    /// issue is not about one field.
    pub path: String,
}

impl Issue {
    pub fn new(code: &str, message: &str, path: &str) -> Self {
        Issue {
            code: code.into(),
            message: message.into(),
            path: path.into(),
        }
    }
}

/// One line of a batch: an operation and its request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BatchRequest {
    /// Any JSON value but null, returned as given on the line that answers
    /// this one; null is no id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    /// The operation's name.
    pub op: String,
    /// The operation's request; `{}` when omitted.
    #[serde(default = "empty_request")]
    pub request: Value,
}

fn empty_request() -> Value {
    Value::Object(Default::default())
}

/// The line answering one line of a batch, in the order they came.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BatchResponse {
    /// The line of the input this answers, the first being 1, blank lines
    /// counted.
    pub line: u64,
    /// The `id` the line gave.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    /// The operation the line named; absent when it could not be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "String")]
    pub op: Option<String>,
    #[serde(flatten)]
    pub outcome: BatchOutcome,
}

/// An operation's full response, or what stopped it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BatchOutcome {
    /// The response of the operation the line named.
    Result(Value),
    Issues(Vec<Issue>),
}

/// A road network an operation runs over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Network {
    Roads,
    Railroads,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn region_forms_read_as_written() {
        let regions: Vec<Region> = serde_json::from_value(json!([
            {"Box": {"min_x": 0, "min_z": 1, "max_x": 2, "max_z": 3}},
            {"Circle": {"x": 1, "z": 2, "radius": 3}},
            {"Polygon": {"points": [[0, 0], [1, 0], [0, 1]]}},
            {"AirdromeBand": {"airdrome": 22, "min_dist": 5000, "max_dist": 20000}},
            {"AirdromeBand": {"airdrome": "Batumi", "min_dist": 5000, "max_dist": 20000}},
        ]))
        .unwrap();
        let named = |r: &Region| match r {
            Region::AirdromeBand { airdrome, .. } => airdrome.clone(),
            _ => unreachable!(),
        };
        assert_eq!(named(&regions[3]), AirdromeRef::Id(22));
        assert_eq!(named(&regions[4]), AirdromeRef::Name("Batumi".into()));
        assert_eq!(
            regions[0],
            Region::Box(BoundingBox {
                min_x: 0.0,
                min_z: 1.0,
                max_x: 2.0,
                max_z: 3.0
            })
        );
        for region in &regions {
            let text = serde_json::to_string(region).unwrap();
            assert_eq!(&serde_json::from_str::<Region>(&text).unwrap(), region);
        }
    }

    #[test]
    fn a_point3_needs_exactly_one_height() {
        let point = |v| serde_json::from_value::<Point3>(v).unwrap();
        assert_eq!(point(json!({"x": 0, "z": 0, "alt": 5})).check("a"), None);
        assert_eq!(
            point(json!({"x": 0, "z": 0, "alt_agl": 2})).check("a"),
            None
        );
        let both = point(json!({"x": 0, "z": 0, "alt": 5, "alt_agl": 2}));
        assert_eq!(both.check("a").unwrap().code, "point3_two_heights");
        let neither = point(json!({"x": 0, "z": 0}));
        let issue = neither.check("route.3").unwrap();
        assert_eq!(
            (issue.code.as_str(), issue.path.as_str()),
            ("point3_no_height", "route.3")
        );
    }

    #[test]
    fn a_footprint_defaults_to_two_orientations() {
        let f: Footprint = serde_json::from_value(json!({"width": 60, "depth": 40})).unwrap();
        assert_eq!(f.orientations_deg, vec![0.0, 90.0]);
    }
}
