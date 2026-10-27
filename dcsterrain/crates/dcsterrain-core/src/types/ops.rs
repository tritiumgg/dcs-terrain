//! One request and one response per operation.
//!
//! Every response flattens a `Provenance` into its top level, so each one
//! carries `terrain`, `dcs_build`, `terrain_fingerprint`, `cell_size` and
//! `elapsed_ms` beside its own fields. A field with a stated default is
//! optional in the request and takes that default when omitted. A default
//! that depends on the terrain, such as a step of twice the cell size, is an
//! `Option` resolved by the operation.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    BoundingBox, Criterion, Footprint, LatLon, Network, Observer, Point, Point3, Polygon,
    Provenance, Region,
};

// describe

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DescribeRequest {}

/// What a terrain is, which files answer for it, and how much of it is known.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DescribeResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    /// The `timestamp` beside the build in `autoupdate.cfg`.
    pub dcs_build_timestamp: String,
    /// The box the terrain's cells cover; null until its mesh has been read.
    pub extent: Option<BoundingBox>,
    /// Each terrain file a decoder reads, as its header states it.
    pub files: Vec<TerrainFile>,
    pub counts: Counts,
    /// The last live validation's figures; null when none has run.
    pub validation: Option<Validation>,
}

/// One terrain file, its path relative to the install.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TerrainFile {
    pub role: String,
    pub path: String,
    /// The class and format version the container header names; null for
    /// Lua source and `assets.assets`, which have no header.
    pub class: Option<String>,
    pub version: Option<u32>,
    pub size: u64,
}

/// How many of each thing the terrain holds. A count is null until what it
/// is counted from has been read.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Counts {
    pub airdromes: Option<u64>,
    pub runways: Option<u64>,
    pub stands: Option<u64>,
    pub beacons: Option<u64>,
    pub towns: Option<u64>,
    pub nodes: Option<u64>,
    pub road_edges: Option<u64>,
    pub scenery_objects: Option<u64>,
}

/// The figures a live validation sortie measured against the running sim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Validation {
    pub height_rmse_m: f64,
    /// The share of sightlines on which `visible` agreed with the sim.
    pub los_agreement: f64,
    /// Route length over the sim's own route length.
    pub route_ratio: f64,
    pub validated_at: String,
}

// sample

/// Either `region` with `step`, or `points`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SampleRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<Region>,
    /// Lattice spacing inside `region`, at least the cell size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<Point>>,
    pub layers: Vec<String>,
    /// More rows than this is an error, never a truncated answer.
    #[serde(default = "default_max_rows")]
    pub max_rows: u32,
}

fn default_max_rows() -> u32 {
    10_000
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SampleResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    pub rows: Vec<SampleRow>,
}

/// `height` is sampled bilinearly, every other layer at the nearest cell.
/// A value is null where the layer is nodata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SampleRow {
    pub x: f64,
    pub z: f64,
    pub values: BTreeMap<String, Option<f64>>,
}

// geo

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GeoRequest {
    #[serde(default)]
    pub to_latlon: Vec<Point>,
    #[serde(default)]
    pub to_xz: Vec<LatLon>,
}

/// Each list in the order of the request's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GeoResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    pub to_latlon: Vec<LatLon>,
    pub to_xz: Vec<Point>,
}

// score_site

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScoreSiteRequest {
    pub x: f64,
    pub z: f64,
    pub footprint: Footprint,
    #[serde(deserialize_with = "super::criteria::deserialize_criteria")]
    pub criteria: Vec<Criterion>,
}

/// The site at its best orientation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScoreSiteResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    pub orientation_deg: f64,
    pub terms: Vec<Term>,
    pub hard_fail: bool,
}

/// One criterion's contribution: the measured `value`, whether the hard test
/// passed, and the soft score from 0 to 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Term {
    pub name: String,
    pub value: f64,
    pub pass: bool,
    pub score: f64,
}

// find_sites

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FindSitesRequest {
    pub region: Region,
    pub footprint: Footprint,
    #[serde(deserialize_with = "super::criteria::deserialize_criteria")]
    pub criteria: Vec<Criterion>,
    /// Candidate lattice spacing; twice the cell size when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    /// No two candidates returned are closer than this.
    pub spacing: f64,
    /// At most 200.
    #[serde(default = "default_find_sites_limit")]
    pub limit: u32,
    /// Weight per criterion name in the ranking sum; 1 for any not named.
    #[serde(default)]
    pub weights: BTreeMap<String, f64>,
}

fn default_find_sites_limit() -> u32 {
    20
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FindSitesResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    /// Best first.
    pub candidates: Vec<Candidate>,
    pub stats: FindSitesStats,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Candidate {
    pub x: f64,
    pub z: f64,
    pub orientation_deg: f64,
    pub score: f64,
    pub terms: Vec<Term>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FindSitesStats {
    pub cells_considered: u64,
    pub cells_passed: u64,
    pub tiles_skipped: u64,
}

// visible

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VisibleRequest {
    pub a: Point3,
    pub b: Point3,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct VisibleResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    pub visible: bool,
    /// The first terrain point in the way, with an absolute `alt`.
    pub blocking: Option<Point3>,
    /// The least vertical gap between sightline and terrain; negative when
    /// blocked.
    pub clearance_m: f64,
}

// viewshed

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ViewshedRequest {
    pub observer: Observer,
    pub radius: f64,
    pub target_alt_agl: f64,
    /// Mask cell size; the file's cell size when omitted, and may be coarser.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<f64>,
    #[serde(default)]
    pub as_polygons: bool,
}

/// The mask is `width` × `height` bits, row-major from the origin corner,
/// base64-encoded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ViewshedResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    pub origin_x: f64,
    pub origin_z: f64,
    pub cell: f64,
    pub width: u32,
    pub height: u32,
    pub bits: String,
    pub visible_fraction: f64,
    /// The mask's outlines, when `as_polygons` was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rings: Option<Vec<Polygon>>,
}

// coverage

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CoverageRequest {
    pub site: Observer,
    pub envelope: Envelope,
    /// An area whose coverage is reported on its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset: Option<Region>,
    #[serde(default = "default_min_masked_range")]
    pub min_masked_range: f64,
}

fn default_min_masked_range() -> f64 {
    8_000.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Envelope {
    pub radius: f64,
    pub alt_bands_agl: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CoverageResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    /// One per requested band, in order.
    pub bands: Vec<CoverageBand>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CoverageBand {
    pub alt_agl: f64,
    pub covered_fraction: f64,
    pub dead_zones: Vec<Polygon>,
    /// Present when the request named an `asset`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_covered_fraction: Option<f64>,
    /// Some radial stays masked from the envelope's radius to inside
    /// `min_masked_range`.
    pub blind_approach_exists: bool,
}

// approach_spectrum

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ApproachSpectrumRequest {
    pub site: Observer,
    /// The engagement or search radius each radial is marched in from.
    pub radius: f64,
    pub alt_bands_agl: Vec<f64>,
    #[serde(default = "default_azimuth_step")]
    pub azimuth_step_deg: f64,
    #[serde(default = "default_min_masked_range")]
    pub min_masked_range: f64,
    /// Summaries are also returned restricted to this sector.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<Sector>,
}

fn default_azimuth_step() -> f64 {
    5.0
}

/// An arc of bearings, clockwise from `from_deg` to `to_deg`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Sector {
    pub from_deg: f64,
    pub to_deg: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ApproachSpectrumResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    /// One per requested band, in order.
    pub bands: Vec<SpectrumBand>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SpectrumBand {
    pub alt_agl: f64,
    pub radials: Vec<Radial>,
    #[serde(flatten)]
    pub summary: SpectrumSummary,
    /// The same summaries over the requested `sector` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector_summary: Option<SpectrumSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Radial {
    pub bearing_deg: f64,
    /// The range of the first step the site sees; null when the whole radial
    /// is masked.
    pub unmask_dist: Option<f64>,
    pub masked_fraction: f64,
    /// Masked from the radius in to `min_masked_range`.
    pub blind: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SpectrumSummary {
    pub blind_arc_fraction: f64,
    pub widest_blind_arc_deg: f64,
    pub blind_arcs: Vec<Sector>,
    pub unmask_dist_min: Option<f64>,
    pub unmask_dist_median: Option<f64>,
    pub unmask_dist_max: Option<f64>,
}

// unmask_profile

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UnmaskProfileRequest {
    pub site: Observer,
    pub route: Vec<Point3>,
    #[serde(default = "default_unmask_step")]
    pub step: f64,
}

fn default_unmask_step() -> f64 {
    100.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UnmaskProfileResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    /// Where along the route the site first sees it; null when never.
    pub first_seen: Option<Point3>,
    pub first_seen_dist_from_site: Option<f64>,
    pub first_seen_dist_along_route: Option<f64>,
    pub masked_segments: Vec<RouteSpan>,
    pub visible_fraction: f64,
}

/// A stretch of a route, as distances along it from its first point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RouteSpan {
    pub from: f64,
    pub to: f64,
}

// route

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RouteRequest {
    pub network: Network,
    pub from: Point,
    pub to: Point,
    /// No edge crossing any of these is used.
    #[serde(default)]
    pub avoid: Vec<Region>,
    /// Edges these observers see cost more, by `exposure_weight`.
    #[serde(default)]
    pub exposure_observers: Vec<Observer>,
    #[serde(default = "default_exposure_weight")]
    pub exposure_weight: f64,
    #[serde(default = "default_max_detour_factor")]
    pub max_detour_factor: f64,
}

fn default_exposure_weight() -> f64 {
    1.0
}

fn default_max_detour_factor() -> f64 {
    3.0
}

/// The route found, with the file that answered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RouteResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    #[serde(flatten)]
    pub route: Route,
}

/// A route over the network between the two snapped endpoints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Route {
    /// At most one point per 500 m, and every graph node kept.
    pub waypoints: Vec<Point>,
    /// Planar length of the road geometry.
    pub length: f64,
    pub ascent: f64,
    pub descent: f64,
    /// The steepest 100 m window.
    pub max_grade_pct: f64,
    pub legs: Vec<Leg>,
    pub snapped_from: Point,
    pub snapped_to: Point,
    /// The distance between the endpoints, so `length / straight_line` is the
    /// detour factor.
    pub straight_line: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Leg {
    pub from: Point,
    pub to: Point,
    pub length: f64,
    pub ascent: f64,
    pub descent: f64,
    pub max_grade_pct: f64,
    /// The fraction of the leg the exposure observers see.
    pub exposure: f64,
}

// route_alternatives

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RouteAlternativesRequest {
    #[serde(flatten)]
    pub route: RouteRequest,
    /// At most 10.
    #[serde(default = "default_k")]
    pub k: u32,
    /// No alternative longer than this times the shortest.
    #[serde(default = "default_max_stretch")]
    pub max_stretch: f64,
    /// No two alternatives share more than this fraction of their length.
    #[serde(default = "default_max_overlap")]
    pub max_overlap: f64,
    /// The weight multiplier on edges already used.
    #[serde(default = "default_penalty")]
    pub penalty: f64,
}

fn default_k() -> u32 {
    3
}

fn default_max_stretch() -> f64 {
    1.5
}

fn default_max_overlap() -> f64 {
    0.5
}

fn default_penalty() -> f64 {
    1.6
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RouteAlternativesResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    /// Shortest first.
    pub routes: Vec<Alternative>,
}

/// One alternative: a full route, and how it relates to the others.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Alternative {
    #[serde(flatten)]
    pub route: Route,
    /// Length over the shortest route's.
    pub stretch: f64,
    /// Shared length fraction against each other returned route, in order.
    pub overlap_with: Vec<f64>,
    /// Edges every alternative uses: the pair's true chokepoints.
    pub shared_edges_with_all: Vec<i64>,
    /// Edges of this route in the top decile of betweenness.
    pub chokepoints: Vec<i64>,
}

// nearest

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NearestRequest {
    pub kind: NearestKind,
    pub x: f64,
    pub z: f64,
    /// At most 50.
    #[serde(default = "default_nearest_n")]
    pub n: u32,
    /// Scenery class filter, for `kind` `scenery`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
}

fn default_nearest_n() -> u32 {
    1
}

/// `node` matches both sides' nodes; each result's `kind` says which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NearestKind {
    Road,
    Rail,
    Airdrome,
    Town,
    Node,
    Beacon,
    Scenery,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NearestResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    /// Nearest first.
    pub results: Vec<Nearby>,
}

/// For road and rail, the closest point on the nearest edge, and `id` is the
/// edge's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Nearby {
    pub kind: String,
    pub id: String,
    pub name: Option<String>,
    pub x: f64,
    pub z: f64,
    pub distance: f64,
}

// scenery_in

/// Either `region`, or `footprint_at` for exact overlap with a placed
/// footprint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SceneryInRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<Region>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footprint_at: Option<FootprintAt>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FootprintAt {
    pub x: f64,
    pub z: f64,
    pub footprint: Footprint,
    pub orientation_deg: f64,
}

/// More than 5 000 objects is an error, never a truncated answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SceneryInResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    pub objects: Vec<SceneryObject>,
    pub counts_by_class: BTreeMap<String, u64>,
}

/// `obb_w` and `obb_d` are null for a model with no footprint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SceneryObject {
    pub id: i64,
    pub model: String,
    pub class: String,
    pub x: f64,
    pub z: f64,
    pub rotation_deg: f64,
    pub obb_w: Option<f64>,
    pub obb_d: Option<f64>,
}

// chokepoints

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ChokepointsRequest {
    pub region: Region,
    pub network: Network,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ChokepointsResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    /// Highest betweenness first.
    pub edges: Vec<Chokepoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Chokepoint {
    pub edge_id: i64,
    pub betweenness: f64,
    pub crosses_water: bool,
    /// Mean `tpi_300` along the edge's geometry.
    pub tpi_300_mean: f64,
    pub midpoint: Point,
}

// trafficability

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TrafficabilityRequest {
    pub x: f64,
    pub z: f64,
    pub max_slope_deg: f64,
    pub network: Network,
}

/// Judged along the straight line to the nearest road, sampled every 25 m
/// for slope and water.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TrafficabilityResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    pub reachable: bool,
    pub nearest_road: Point,
    pub distance: f64,
    /// The first sample that fails, when one does.
    pub blocking: Option<Point>,
    /// Why the line is or is not passable, for a reader.
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provenance() -> Provenance {
        Provenance {
            terrain: "synth".into(),
            dcs_build: "0.0.0.0".into(),
            terrain_fingerprint: "synth-1-70-50".into(),
            cell_size: 50.0,
            elapsed_ms: 3,
        }
    }

    #[test]
    fn a_response_carries_provenance_at_its_top_level() {
        let r = VisibleResponse {
            provenance: provenance(),
            visible: true,
            blocking: None,
            clearance_m: 12.5,
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(
            v,
            json!({
                "terrain": "synth", "dcs_build": "0.0.0.0",
                "terrain_fingerprint": "synth-1-70-50", "cell_size": 50.0,
                "elapsed_ms": 3, "visible": true, "blocking": null, "clearance_m": 12.5
            })
        );
        assert_eq!(serde_json::from_value::<VisibleResponse>(v).unwrap(), r);
    }

    #[test]
    fn request_defaults_are_the_stated_ones() {
        let r: SampleRequest = serde_json::from_value(json!({"layers": ["height"]})).unwrap();
        assert_eq!(r.max_rows, 10_000);
        let r: CoverageRequest = serde_json::from_value(json!({
            "site": {"x": 0, "z": 0, "alt_agl": 10},
            "envelope": {"radius": 40000, "alt_bands_agl": [50, 300]}
        }))
        .unwrap();
        assert_eq!(r.min_masked_range, 8_000.0);
        let r: UnmaskProfileRequest = serde_json::from_value(json!({
            "site": {"x": 0, "z": 0, "alt_agl": 10}, "route": []
        }))
        .unwrap();
        assert_eq!(r.step, 100.0);
        let r: FindSitesRequest = serde_json::from_value(json!({
            "region": {"Circle": {"x": 0, "z": 0, "radius": 5000}},
            "footprint": {"width": 60, "depth": 40},
            "criteria": [{"name": "valid"}],
            "spacing": 500
        }))
        .unwrap();
        assert_eq!((r.limit, r.step), (20, None));
        let r: NearestRequest =
            serde_json::from_value(json!({"kind": "town", "x": 0, "z": 0})).unwrap();
        assert_eq!(r.n, 1);
    }

    #[test]
    fn valid_reads_with_or_without_an_empty_params() {
        let r: ScoreSiteRequest = serde_json::from_value(json!({
            "x": 0, "z": 0, "footprint": {"width": 60, "depth": 40},
            "criteria": [
                {"name": "valid"}, {"name": "valid", "params": {}},
                {"name": "valid", "params": null}, {"name": "slope_max", "params": {"deg": 8}}
            ]
        }))
        .unwrap();
        assert_eq!(
            r.criteria[..3],
            [Criterion::Valid, Criterion::Valid, Criterion::Valid]
        );
        let bad = serde_json::from_value::<ScoreSiteRequest>(json!({
            "x": 0, "z": 0, "footprint": {"width": 60, "depth": 40},
            "criteria": [{"name": "valid", "params": {"deg": 8}}]
        }));
        assert!(bad.is_err());
    }

    #[test]
    fn route_alternatives_takes_everything_route_takes() {
        let r: RouteAlternativesRequest = serde_json::from_value(json!({
            "network": "roads", "from": {"x": 0, "z": 0}, "to": {"x": 1000, "z": 0},
            "exposure_weight": 2
        }))
        .unwrap();
        assert_eq!(r.route.exposure_weight, 2.0);
        assert_eq!(r.route.max_detour_factor, 3.0);
        assert_eq!(
            (r.k, r.max_stretch, r.max_overlap, r.penalty),
            (3, 1.5, 0.5, 1.6)
        );
    }

    #[test]
    fn a_spectrum_band_flattens_its_summary() {
        let summary = SpectrumSummary {
            blind_arc_fraction: 0.25,
            widest_blind_arc_deg: 40.0,
            blind_arcs: vec![Sector {
                from_deg: 10.0,
                to_deg: 50.0,
            }],
            unmask_dist_min: Some(2000.0),
            unmask_dist_median: None,
            unmask_dist_max: None,
        };
        let band = SpectrumBand {
            alt_agl: 50.0,
            radials: vec![],
            summary,
            sector_summary: None,
        };
        let v = serde_json::to_value(&band).unwrap();
        assert_eq!(v["widest_blind_arc_deg"], json!(40.0));
        assert!(v.get("summary").is_none() && v.get("sector_summary").is_none());
        assert_eq!(serde_json::from_value::<SpectrumBand>(v).unwrap(), band);
    }
}
