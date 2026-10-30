//! The operation list and the JSON schema of its types.
//!
//! The list is the one place an operation is named: `schema` prints from it
//! and `query` answers what it lists, so an operation added here appears on
//! both at once.

use std::fmt;

use schemars::generate::SchemaSettings;
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde_json::{Map, Value, json};

use super::*;

/// An operation by name, with its request and response types.
pub struct Operation {
    pub name: &'static str,
    request: fn(&mut SchemaGenerator) -> Schema,
    response: fn(&mut SchemaGenerator) -> Schema,
}

const fn op<Req: JsonSchema, Resp: JsonSchema>(name: &'static str) -> Operation {
    Operation {
        name,
        request: |g| g.subschema_for::<Req>(),
        response: |g| g.subschema_for::<Resp>(),
    }
}

pub const OPERATIONS: &[Operation] = &[
    op::<DescribeRequest, DescribeResponse>("describe"),
    op::<SampleRequest, SampleResponse>("sample"),
    op::<GeoRequest, GeoResponse>("geo"),
    op::<ScoreSiteRequest, ScoreSiteResponse>("score_site"),
    op::<FindSitesRequest, FindSitesResponse>("find_sites"),
    op::<VisibleRequest, VisibleResponse>("visible"),
    op::<ViewshedRequest, ViewshedResponse>("viewshed"),
    op::<RadarFloorRequest, RadarFloorResponse>("radar_floor"),
    op::<CoverageRequest, CoverageResponse>("coverage"),
    op::<ExposureRequest, ExposureResponse>("exposure"),
    op::<ApproachSpectrumRequest, ApproachSpectrumResponse>("approach_spectrum"),
    op::<UnmaskProfileRequest, UnmaskProfileResponse>("unmask_profile"),
    op::<RouteRequest, RouteResponse>("route"),
    op::<RouteAlternativesRequest, RouteAlternativesResponse>("route_alternatives"),
    op::<NearestRequest, NearestResponse>("nearest"),
    op::<SceneryInRequest, SceneryInResponse>("scenery_in"),
    op::<ChokepointsRequest, ChokepointsResponse>("chokepoints"),
    op::<TrafficabilityRequest, TrafficabilityResponse>("trafficability"),
    op::<PointInfoRequest, PointInfoResponse>("point_info"),
    op::<AirfieldInfoRequest, AirfieldInfoResponse>("airfield_info"),
    op::<ValidatePlacementsRequest, ValidatePlacementsResponse>("validate_placements"),
    op::<TownsRequest, TownsResponse>("towns"),
];

/// Every `Issue::code` a surface returns and what it means. A code is never
/// reworded once shipped; a test holds this list to the source.
pub const ISSUE_CODES: &[(&str, &str)] = &[
    ("internal_error", "a bug: the operation failed unexpectedly"),
    (
        "invalid_criterion",
        "a criterion's name or params are wrong",
    ),
    ("invalid_flag", "a command-line flag does not apply here"),
    ("invalid_point", "a point is outside what can be converted"),
    ("invalid_region", "a region is empty or malformed"),
    ("invalid_request", "the request is not what the schema says"),
    ("no_cache", "no cache directory could be found"),
    ("no_install", "no DCS install was given or found"),
    ("no_network", "the terrain has no edge of that network"),
    ("no_route", "no route joins the ends under the constraints"),
    (
        "not_ready",
        "a cache entry `index` builds does not exist yet",
    ),
    ("off_terrain", "a point lies off the terrain's mesh"),
    ("point3_no_height", "a point sets neither alt nor alt_agl"),
    ("point3_two_heights", "a point sets both alt and alt_agl"),
    ("refused", "a terrain file or cache entry cannot be read"),
    ("terrain_refused", "the terrain cannot be opened"),
    ("too_far", "the nearest road is too far to judge"),
    ("too_large", "the request's work is over its limit"),
    ("too_many_rows", "the answer is over `max_rows`"),
    ("unknown_airdrome", "no airdrome has that id or name_id"),
    ("unknown_class", "a class name is not in the closed set"),
    ("unknown_layer", "a layer name is not a layer"),
    ("unknown_operation", "no operation has that name"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownOperation(pub String);

impl fmt::Display for UnknownOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<_> = OPERATIONS.iter().map(|o| o.name).collect();
        write!(
            f,
            "no operation `{}`; the operations are {}",
            self.0,
            names.join(", ")
        )
    }
}

impl std::error::Error for UnknownOperation {}

/// One JSON Schema (draft 2020-12) document for every operation, or for the
/// one named.
///
/// The document is an object with an `operations` property holding each
/// operation's `request` and `response`, an `error` property, the issue list
/// any operation returns instead of a result, `issue_codes`, what an issue's
/// `code` can be, and `batch`, the `request` and `response` of one line of a
/// JSON-lines batch. Every type is a `$defs` entry under its own name, so a
/// code generator emits one class per type.
pub fn schema(only: Option<&str>) -> Result<Value, UnknownOperation> {
    let chosen: Vec<&Operation> = match only {
        None => OPERATIONS.iter().collect(),
        Some(name) => vec![
            OPERATIONS
                .iter()
                .find(|o| o.name == name)
                .ok_or_else(|| UnknownOperation(name.into()))?,
        ],
    };

    let mut generator = SchemaGenerator::new(SchemaSettings::draft2020_12());
    let mut operations = Map::new();
    for o in &chosen {
        let pair = json!({
            "type": "object",
            "properties": {
                "request": (o.request)(&mut generator),
                "response": (o.response)(&mut generator),
            },
            "required": ["request", "response"],
        });
        operations.insert(o.name.into(), pair);
    }
    let names: Vec<_> = chosen.iter().map(|o| o.name).collect();
    let error = json!({"type": "array", "items": generator.subschema_for::<Issue>()});
    let issue_codes: Vec<Value> = ISSUE_CODES
        .iter()
        .map(|(code, meaning)| json!({"const": code, "description": meaning}))
        .collect();
    let batch = json!({
        "type": "object",
        "properties": {
            "request": generator.subschema_for::<BatchRequest>(),
            "response": generator.subschema_for::<BatchResponse>(),
        },
        "required": ["request", "response"],
    });

    Ok(json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "dcsterrain",
        "type": "object",
        "properties": {
            "operations": {
                "type": "object",
                "properties": operations,
                "required": names,
            },
            "error": error,
            "issue_codes": {"oneOf": issue_codes},
            "batch": batch,
        },
        "$defs": generator.take_definitions(true),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `$ref` in `v`, collected.
    fn refs<'a>(v: &'a Value, out: &mut Vec<&'a str>) {
        match v {
            Value::Object(m) => {
                if let Some(Value::String(r)) = m.get("$ref") {
                    out.push(r);
                }
                m.values().for_each(|x| refs(x, out));
            }
            Value::Array(a) => a.iter().for_each(|x| refs(x, out)),
            _ => {}
        }
    }

    #[test]
    fn every_reference_resolves() {
        let doc = schema(None).unwrap();
        let defs = doc["$defs"].as_object().unwrap();
        let mut found = Vec::new();
        refs(&doc, &mut found);
        assert!(!found.is_empty());
        for r in found {
            let name = r.strip_prefix("#/$defs/").expect(r);
            assert!(defs.contains_key(name), "{r} has no definition");
        }
    }

    /// Every code the source hands an issue: the string that opens the
    /// arguments of each call that makes one.
    fn codes_in(dir: &std::path::Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                codes_in(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("tests.rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                let text = text.split("#[cfg(test)]").next().unwrap();
                for maker in ["Issue::new(", "issue(", "refused("] {
                    for (at, _) in text.match_indices(maker) {
                        let rest = text[at + maker.len()..].trim_start();
                        if let Some(rest) = rest.strip_prefix('"') {
                            let word = rest.split('"').next().unwrap();
                            if word
                                .bytes()
                                .all(|b| b.is_ascii_lowercase() || b == b'_' || b.is_ascii_digit())
                            {
                                out.push(word.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_codes_are_the_ones_the_source_returns() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut found = Vec::new();
        codes_in(&root.join("src"), &mut found);
        codes_in(&root.join("../dcsterrain-cli/src"), &mut found);
        found.sort_unstable();
        found.dedup();
        assert!(found.len() > 15, "{found:?}");
        for code in &found {
            assert!(
                ISSUE_CODES.iter().any(|(c, _)| c == code),
                "{code} is not listed"
            );
        }
        let listed: Vec<_> = ISSUE_CODES.iter().map(|(c, _)| *c).collect();
        assert!(listed.is_sorted(), "listed in order");
        for code in &listed {
            let source = found.iter().any(|c| c == code);
            assert!(source, "{code} is listed and nothing returns it");
        }
    }

    #[test]
    fn the_list_names_each_operation_once() {
        let mut names: Vec<_> = OPERATIONS.iter().map(|o| o.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), OPERATIONS.len());
    }

    #[test]
    fn one_operation_brings_only_its_own_types() {
        let doc = schema(Some("visible")).unwrap();
        let ops = doc["properties"]["operations"]["properties"]
            .as_object()
            .unwrap();
        assert_eq!(ops.keys().collect::<Vec<_>>(), ["visible"]);
        let defs = doc["$defs"].as_object().unwrap();
        assert!(defs.contains_key("Point3") && !defs.contains_key("Criterion"));
    }

    #[test]
    fn an_unknown_operation_names_the_known_ones() {
        let e = schema(Some("visble")).unwrap_err();
        assert!(e.to_string().contains("visible"));
    }
}
