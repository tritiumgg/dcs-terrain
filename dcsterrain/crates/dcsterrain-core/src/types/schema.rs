//! The operation list and the JSON schema of its types.
//!
//! The list is the one place an operation is named. The CLI's `schema`
//! prints from it, and the MCP tools are registered from it, so an operation
//! added here appears on every surface at once.

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
    op::<CoverageRequest, CoverageResponse>("coverage"),
    op::<ApproachSpectrumRequest, ApproachSpectrumResponse>("approach_spectrum"),
    op::<UnmaskProfileRequest, UnmaskProfileResponse>("unmask_profile"),
    op::<RouteRequest, RouteResponse>("route"),
    op::<RouteAlternativesRequest, RouteAlternativesResponse>("route_alternatives"),
    op::<NearestRequest, NearestResponse>("nearest"),
    op::<SceneryInRequest, SceneryInResponse>("scenery_in"),
    op::<ChokepointsRequest, ChokepointsResponse>("chokepoints"),
    op::<TrafficabilityRequest, TrafficabilityResponse>("trafficability"),
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
/// operation's `request` and `response`, and an `error` property, the issue
/// list any operation returns instead of a result. Every type is a `$defs`
/// entry under its own name, so a code generator emits one class per type.
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
