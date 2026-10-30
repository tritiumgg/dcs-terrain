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

use crate::places::{Form, SizeClass};

use super::{
    AirdromeRef, BoundingBox, CoverClass, Criterion, Footprint, LatLon, Network, Observer, Point,
    Point3, Polygon, Provenance, Region, WaterClass,
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
    /// Each cache entry as it stands; empty for the synthetic terrain,
    /// which has no cache.
    pub cache: Vec<CacheEntry>,
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

/// One cache entry: `current`, `missing`, or `stale` with the reason.
/// `size` is its file's bytes, null when there is none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CacheEntry {
    pub name: String,
    pub state: String,
    pub why: Option<String>,
    pub size: Option<u64>,
}

/// How many of each thing the terrain holds. The tables' counts are always
/// given; road edges and scenery objects are null until a request in the
/// same process has read the road graph or the scenery.
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
    /// Some distance found nothing within its cap and reads as the cap.
    pub distance_capped: bool,
}

/// Over a region, `height` is bilinear between cell centers and every other
/// layer the cell holding the point; at given points, the ground layers are
/// the mesh triangle's own and the derived layers the cell's. A value is
/// null where the layer is undefined.
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
    pub to_latlon: Vec<GeoPoint>,
    pub to_xz: Vec<Point>,
}

/// A position in WGS84 degrees, with its MGRS reference to the meter; the
/// reference is null outside 80° S to 84° N, where MGRS uses polar grids.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
    pub mgrs: Option<String>,
}

// score_site

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScoreSiteRequest {
    pub x: f64,
    pub z: f64,
    pub footprint: Footprint,
    #[serde(deserialize_with = "super::criteria::deserialize_criteria")]
    pub criteria: Vec<Criterion>,
    /// Also return the site and its footprint as GeoJSON.
    #[serde(default)]
    pub geojson: bool,
}

/// The site at its best orientation: one that passes every hard test if
/// any does, the highest score among those, the first given on a tie.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScoreSiteResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    pub orientation_deg: f64,
    /// The sum of the term scores, as `find_sites` ranks with every weight
    /// 1.
    pub score: f64,
    pub terms: Vec<Term>,
    pub hard_fail: bool,
    /// The site and its footprint, when `geojson` was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geojson: Option<serde_json::Value>,
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
    /// Also return the candidates and their footprints as GeoJSON.
    #[serde(default)]
    pub geojson: bool,
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
    /// Each candidate and its footprint, when `geojson` was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geojson: Option<serde_json::Value>,
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
    /// Also return the answer as GeoJSON.
    #[serde(default)]
    pub geojson: bool,
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
    /// The sightline, its ends and the blocking point, when `geojson` was
    /// asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geojson: Option<serde_json::Value>,
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
    /// Also return the visible area as GeoJSON.
    #[serde(default)]
    pub geojson: bool,
}

/// The mask is `height` rows running north, each `width` cells running
/// east, from the corner at `origin_x`, `origin_z`; cell `(row, col)` is bit
/// `row × width + col`, most significant first within each byte, base64
/// encoded. A set bit is a cell whose center lies within `radius` and at
/// which a target `target_alt_agl` above the ground is seen.
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
    /// The share of the cells within `radius` that have ground which are
    /// seen.
    pub visible_fraction: f64,
    /// The outlines of the set cells as `[x, z]`, north then east, when
    /// `as_polygons` was asked for: each polygon counterclockwise on a map,
    /// then the holes in it clockwise, so a hole is told by its winding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rings: Option<Vec<Polygon>>,
    /// The visible area as one `MultiPolygon`, with the observer, when
    /// `geojson` was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geojson: Option<serde_json::Value>,
}

// radar_floor

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RadarFloorRequest {
    pub site: Observer,
    pub radius: f64,
    /// Raster cell size; the file's cell size when omitted, and may be
    /// coarser.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<f64>,
    /// Answer at these points instead of over a raster.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<Point>>,
    /// The heights above ground the bands are drawn at.
    #[serde(default = "default_floor_bands")]
    pub bands_agl: Vec<f64>,
    /// Also return the bands, or the points, as GeoJSON.
    #[serde(default)]
    pub geojson: bool,
}

fn default_floor_bands() -> Vec<f64> {
    vec![30.0, 100.0, 300.0, 1000.0]
}

/// The floor is a raster without `points`, and one row per point with them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RadarFloorResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raster: Option<FloorRaster>,
    /// The share of the raster's cells seen at each band, lowest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bands: Vec<FloorBand>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub points: Option<Vec<PointFloor>>,
    /// One `MultiPolygon` per band, lowest first, or a `Point` per point,
    /// with the site, when `geojson` was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geojson: Option<serde_json::Value>,
}

/// Laid out as `viewshed`'s mask: `height` rows running north, each
/// `width` cells running east, from the corner at `origin_x`, `origin_z`.
/// Each layer is one little-endian `f32` per cell, base64 encoded, NaN
/// where a cell's center lies beyond `radius` or has no ground.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FloorRaster {
    pub origin_x: f64,
    pub origin_z: f64,
    pub cell: f64,
    pub width: u32,
    pub height: u32,
    /// The lowest height above the cell's ground at which the site sees a
    /// target.
    pub floor_agl: String,
    /// The same as an altitude above sea level.
    pub floor_msl: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FloorBand {
    pub alt_agl: f64,
    /// The share of the cells within `radius` with ground whose floor is at
    /// or below `alt_agl`.
    pub seen_fraction: f64,
}

/// A point's floor; the floors are null beyond `radius` or off the mesh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PointFloor {
    pub x: f64,
    pub z: f64,
    pub ground: Option<f64>,
    pub floor_agl: Option<f64>,
    pub floor_msl: Option<f64>,
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
    /// Mask cell size; the file's cell size when omitted, and may be coarser.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<f64>,
    /// Also return the dead zones as GeoJSON.
    #[serde(default)]
    pub geojson: bool,
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
    /// Each band's dead zones as one `MultiPolygon`, the site, and the
    /// asset's outline when given, when `geojson` was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geojson: Option<serde_json::Value>,
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
    /// Raster cell size; the file's cell size when omitted, and may be
    /// coarser.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<f64>,
    /// Also return each blind arc as a wedge in GeoJSON.
    #[serde(default)]
    pub geojson: bool,
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
    /// A wedge per blind arc out to `radius`, with the site, when `geojson`
    /// was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geojson: Option<serde_json::Value>,
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
    /// Also return the route, its masked stretches and the first-seen
    /// point as GeoJSON.
    #[serde(default)]
    pub geojson: bool,
}

fn default_unmask_step() -> f64 {
    100.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UnmaskProfileResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    /// Where along the route the site first sees it, with an absolute
    /// `alt`; null when never.
    pub first_seen: Option<Point3>,
    pub first_seen_dist_from_site: Option<f64>,
    pub first_seen_dist_along_route: Option<f64>,
    pub masked_segments: Vec<RouteSpan>,
    pub visible_fraction: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geojson: Option<serde_json::Value>,
}

/// A stretch of a route, as distances along it from its first point.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RouteSpan {
    pub from: f64,
    pub to: f64,
}

// exposure

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExposureRequest {
    /// The sites looking, each out to its own radius.
    pub sites: Vec<ExposureSite>,
    /// The area answered over; the box about every site's radius when
    /// omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<Region>,
    /// The height above ground at which each cell's sites are counted.
    pub target_alt_agl: f64,
    /// The heights above ground the unseen areas are drawn at.
    #[serde(default = "default_floor_bands")]
    pub bands_agl: Vec<f64>,
    /// Raster cell size; the file's cell size when omitted, and may be
    /// coarser.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<f64>,
    /// Also return the unseen areas as GeoJSON.
    #[serde(default)]
    pub geojson: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExposureSite {
    pub observer: Observer,
    pub radius: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExposureResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    pub raster: ExposureRaster,
    pub target_alt_agl: f64,
    /// The share of the region's cells with ground that some site sees at
    /// `target_alt_agl`.
    pub seen_fraction: f64,
    /// The share unseen at each band, lowest first.
    pub bands: Vec<ExposureBand>,
    /// One `MultiPolygon` per band of the area unseen at it, lowest first,
    /// with the sites, when `geojson` was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geojson: Option<serde_json::Value>,
}

/// Laid out as `radar_floor`'s raster, over the region's box. The floors
/// are NaN off the ground or outside the region, and infinite where no
/// site's radius reaches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExposureRaster {
    pub origin_x: f64,
    pub origin_z: f64,
    pub cell: f64,
    pub width: u32,
    pub height: u32,
    /// The lowest height above the cell's ground at which any site sees a
    /// target: the least of the sites' floors.
    pub floor_agl: String,
    /// The same as an altitude above sea level.
    pub floor_msl: String,
    /// How many sites see a target `target_alt_agl` above the cell's
    /// ground, one byte per cell, base64.
    pub seen_count: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExposureBand {
    pub alt_agl: f64,
    /// The share of the region's cells with ground that no site sees at
    /// `alt_agl`.
    pub unseen_fraction: f64,
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
    /// No route longer than this times the straight line between the
    /// snapped ends, or 5 km longer when that is more, is searched for.
    #[serde(default = "default_max_detour_factor")]
    pub max_detour_factor: f64,
    /// Also return the answer as GeoJSON.
    #[serde(default)]
    pub geojson: bool,
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
    /// The route, each leg and the ends, when `geojson` was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geojson: Option<serde_json::Value>,
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
    /// The graph edge the leg runs along, all of it or, at an end of the
    /// route, part.
    pub edge_id: i64,
    pub class: String,
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
    /// The least-cost route first, which with no exposure weight is the
    /// shortest, then the alternatives in the order found.
    pub routes: Vec<Alternative>,
    /// Each route, the edges they all share and the ends, when `geojson`
    /// was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geojson: Option<serde_json::Value>,
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
    /// Edges along which every alternative runs: the pair's true
    /// chokepoints.
    pub shared_edges_with_all: Vec<i64>,
    /// Edges of this route in the top decile of their network's
    /// betweenness; null while betweenness is still being built.
    pub chokepoints: Option<Vec<i64>>,
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
    /// For `kind` `scenery`, only objects of this class; `effect` objects
    /// are found only when named here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
}

fn default_nearest_n() -> u32 {
    1
}

/// `node` matches both sides' nodes; each result's `kind` says which. A
/// `place` is measured to its center, its `class` its size class.
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
    Place,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NearestResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    /// Nearest first.
    pub results: Vec<Nearby>,
}

/// For road and rail, the closest point on each of the nearest edges, `id`
/// being the edge's index in the graph and `class` its road class. A node
/// result's `kind` is `node_red` or `node_blue`, the side whose position it
/// gives. A scenery object's `class` is its scenery class and `name` its
/// model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Nearby {
    pub kind: String,
    pub id: String,
    pub name: Option<String>,
    pub class: Option<String>,
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
    /// Only objects of these classes; every class but `effect` when
    /// omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classes: Option<Vec<String>>,
    /// More objects than this is an error, never a truncated answer.
    #[serde(default = "default_scenery_max_rows")]
    pub max_rows: u32,
}

fn default_scenery_max_rows() -> u32 {
    5_000
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FootprintAt {
    pub x: f64,
    pub z: f64,
    pub footprint: Footprint,
    pub orientation_deg: f64,
}

/// The objects in id order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SceneryInResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    pub objects: Vec<SceneryObject>,
    pub counts_by_class: BTreeMap<String, u64>,
}

/// An object's box is `obb_w` across its local x axis, which points
/// `rotation_deg` from north towards east, and `obb_d` across its local z;
/// both are null for a model with no footprint. `alt` is its base and
/// `height` its model's top above that.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SceneryObject {
    pub id: u64,
    pub model: String,
    pub class: String,
    pub x: f64,
    pub z: f64,
    pub alt: f64,
    pub rotation_deg: f64,
    pub obb_w: Option<f64>,
    pub obb_d: Option<f64>,
    pub height: f64,
}

// chokepoints

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ChokepointsRequest {
    /// Edges any point of whose path lies in it.
    pub region: Region,
    pub network: Network,
    /// At most 500.
    #[serde(default = "default_chokepoints_limit")]
    pub limit: u32,
    /// Also return the answer as GeoJSON.
    #[serde(default)]
    pub geojson: bool,
}

fn default_chokepoints_limit() -> u32 {
    20
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ChokepointsResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    /// Highest betweenness first, ties in edge order.
    pub edges: Vec<Chokepoint>,
    /// Each edge, when `geojson` was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geojson: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Chokepoint {
    pub edge_id: i64,
    /// 0 to 1, of the network's most.
    pub betweenness: f64,
    pub class: String,
    pub crosses_water: bool,
    /// Mean `tpi_300` along the edge's geometry, null where no sample has
    /// ground.
    pub tpi_300_mean: Option<f64>,
    pub midpoint: Point,
}

// trafficability

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TrafficabilityRequest {
    pub x: f64,
    pub z: f64,
    /// Steeper ground than this blocks; at most 60.
    pub max_slope_deg: f64,
    pub network: Network,
    /// Also return the answer as GeoJSON.
    #[serde(default)]
    pub geojson: bool,
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
    /// The line's length weighted by how hard its ground is to cross, in
    /// meters of open flat ground; null when it is not reachable.
    pub cost: Option<f64>,
    /// The first sample that fails, when one does.
    pub blocking: Option<Point>,
    /// Why the line is or is not passable, for a reader.
    pub reason: String,
    /// The line, its ends and the blocking point, when `geojson` was asked
    /// for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geojson: Option<serde_json::Value>,
}

// point_info

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PointInfoRequest {
    /// At most 10 000.
    pub points: Vec<Point>,
    /// Also return the answer as GeoJSON.
    #[serde(default)]
    pub geojson: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PointInfoResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    /// In the order of the request's.
    pub points: Vec<PointInfo>,
    /// Whether the places were built, so a null `place` means outside
    /// every one; `dcsterrain index` builds them.
    pub places_built: bool,
    /// A point per request point with its fields, when `geojson` was asked
    /// for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geojson: Option<serde_json::Value>,
}

/// What stands at one point. The ground fields are the mesh triangle's own
/// and null where no mesh covers the point; `slope` and `aspect` are the
/// cell's holding it. Every distance is in DCS meters, along the flat
/// x–z plane the game measures in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PointInfo {
    pub x: f64,
    pub z: f64,
    /// The water surface over water, the ground elsewhere.
    pub height: Option<f64>,
    pub water: Option<WaterClass>,
    /// Meters of water over the bed; null on land.
    pub depth: Option<f64>,
    pub cover: Option<CoverClass>,
    /// Degrees from level; null where a neighboring cell has no ground.
    pub slope: Option<f64>,
    /// The downslope direction in degrees clockwise from grid north, DCS
    /// `x`; null on ground flatter than half a degree.
    pub aspect: Option<f64>,
    /// Null when the terrain lists no towns.
    pub town: Option<TownFix>,
    /// The place whose outline holds the point; null outside every place,
    /// and while the places are still being built.
    pub place: Option<PlaceRef>,
    /// The nearest point of the nearest road; null when the terrain has
    /// none.
    pub road: Option<Nearby>,
    /// Null when no airdrome has a position.
    pub airfield: Option<Nearby>,
    pub lat: f64,
    pub lon: f64,
    /// Null outside 80° S to 84° N, where MGRS uses polar grids.
    pub mgrs: Option<String>,
}

/// The point placed from its nearest town, as a reader would say it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TownFix {
    pub name: String,
    pub x: f64,
    pub z: f64,
    pub distance: f64,
    /// From the town to the point, clockwise from true north at the town.
    pub bearing_true_deg: f64,
    /// From the town to the point, clockwise from grid north, DCS `x`.
    pub bearing_grid_deg: f64,
    /// The true bearing's point of the eight-point compass.
    pub compass: String,
    /// Such as "6 km NE of Zugdidi"; "at Zugdidi" within 50 m of it.
    pub description: String,
}

// towns

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TownsRequest {
    /// Places whose center lies in it; every place of the terrain when
    /// omitted.
    #[serde(default)]
    pub region: Option<Region>,
    /// Only places of these size classes; any class when empty.
    #[serde(default)]
    pub size: Vec<SizeClass>,
    /// Only places of these forms; any form when empty.
    #[serde(default)]
    pub form: Vec<Form>,
    #[serde(default)]
    pub min_buildings: u32,
    #[serde(default)]
    pub sort: TownsSort,
    /// At most 1 000.
    #[serde(default = "default_towns_limit")]
    pub limit: u32,
    /// Each place's outline: its buildings buffered by 100 m, rasterized at
    /// 50 m, traced and simplified to within 25 m.
    #[serde(default)]
    pub outlines: bool,
    /// Also return the answer as GeoJSON.
    #[serde(default)]
    pub geojson: bool,
}

fn default_towns_limit() -> u32 {
    50
}

/// What places are ranked by, most first, ties by id.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TownsSort {
    #[default]
    Buildings,
    ResidentialFloor,
    ApartmentShare,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TownsResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    /// How many places pass the region and the filters, before the limit.
    pub matched: u32,
    pub places: Vec<PlaceInfo>,
    /// A point at each place's center, and its outline when `outlines`
    /// was asked for, when `geojson` was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geojson: Option<serde_json::Value>,
}

/// A place: settlement buildings each within 200 m of another, three or
/// more.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlaceInfo {
    /// The least scenery object id among its buildings, stable while the
    /// scene file is.
    pub id: u64,
    /// The town of the terrain's list that names it; null when none does.
    pub name: Option<String>,
    /// Other towns of the list it holds, largest first.
    pub other_names: Vec<String>,
    /// The mean of its buildings' positions.
    pub x: f64,
    pub z: f64,
    /// Its buildings' positions' box.
    pub bbox: BoundingBox,
    pub size: SizeClass,
    pub form: Form,
    pub buildings: u32,
    pub houses: u32,
    pub apartments: u32,
    pub shops: u32,
    pub civic: u32,
    pub industrial: u32,
    /// Square meters of house and apartment floor.
    pub residential_floor: f64,
    /// Apartment blocks over its buildings.
    pub apartment_share: f64,
    /// The 100 m cells holding one of its buildings.
    pub hectares: u32,
    /// Polygons, each its exterior ring then its holes, as `[x, z]`
    /// vertices; when `outlines` was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outline: Option<Vec<Vec<Vec<[f64; 2]>>>>,
}

/// The place a point stands in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlaceRef {
    pub id: u64,
    pub name: Option<String>,
    pub size: SizeClass,
    pub form: Form,
}

// airfield_info

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AirfieldInfoRequest {
    /// A number is the airdrome's id; a string is its `name_id` or, where
    /// none matches, its id written in digits.
    pub airdrome: AirdromeRef,
    /// An airframe's size: each stand then says whether it fits.
    #[serde(default)]
    pub fits: Option<Airframe>,
    /// Also return the answer as GeoJSON.
    #[serde(default)]
    pub geojson: bool,
}

/// An airframe's size in meters, which the caller supplies.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Airframe {
    /// Span, rotor disc included.
    pub width: f64,
    pub length: f64,
    pub height: f64,
    /// A helicopter needs a stand helicopters may use; an airplane one
    /// airplanes may.
    #[serde(default)]
    pub helicopter: bool,
}

/// One airdrome. Courses are true, from true north at the threshold they
/// leave, and grid, from DCS `x`; there is no magnetic model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AirfieldInfoResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    /// Null where no file gives it one; the airdrome is then named by its
    /// `name_id`.
    pub id: Option<i64>,
    pub name_id: String,
    /// Its first beacon's name, or its `name_id` where it has none.
    pub name: String,
    /// The mean of its runway ends; null with no runway.
    pub x: Option<f64>,
    pub z: Option<f64>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    /// The highest ground along any runway's centerline, over the mesh
    /// triangles; with no runway on the mesh, the highest ground at a stand.
    /// Null with neither on the mesh.
    pub elevation: Option<f64>,
    pub runways: Vec<RunwayInfo>,
    /// Each beacon of the airdrome, in the order the files list them.
    pub navaids: Vec<Navaid>,
    pub radio: Vec<RadioInfo>,
    pub stands: Vec<StandInfo>,
    /// How many stands there are of each size, widest first.
    pub stand_sizes: Vec<StandSize>,
    /// How many stands the airframe `fits` names fits; null without it.
    pub stands_fitting: Option<u32>,
    /// Each runway as a line between its thresholds and each stand as a
    /// point, when `geojson` was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geojson: Option<serde_json::Value>,
}

/// A runway between its two thresholds, the ends the taxi graph names.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RunwayInfo {
    /// Both ends' designators, the first threshold's first: "22-04".
    pub designator: String,
    /// From the first threshold to the second, clockwise from true north
    /// at the first.
    pub course_true_deg: f64,
    /// The same, from grid north, DCS `x`.
    pub course_grid_deg: f64,
    /// Threshold to threshold, in the x–z plane.
    pub length: f64,
    /// Across the runway surface the mesh paints, the median of its width
    /// at nine stations from a tenth to nine tenths of the way along; null
    /// where the mesh paints none there.
    pub width: Option<f64>,
    /// The first threshold, then the second.
    pub thresholds: Vec<Threshold>,
}

/// One end of a runway, and the course a landing on it flies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Threshold {
    pub designator: String,
    pub x: f64,
    pub z: f64,
    pub lat: f64,
    pub lon: f64,
    /// The ground at the threshold; null off the mesh.
    pub height: Option<f64>,
    /// Towards the other threshold, clockwise from true north here.
    pub course_true_deg: f64,
    pub course_grid_deg: f64,
}

/// A radio navigation beacon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Navaid {
    pub id: Option<String>,
    /// Such as `tacan`, `vor_dme`, `ils_localizer` or `airport_homer`, the
    /// game's beacon type without its `BEACON_TYPE_` prefix; null for a
    /// type the game does not name.
    pub kind: Option<String>,
    /// The game's code for the type.
    pub kind_code: Option<i64>,
    pub callsign: Option<String>,
    pub frequency_hz: Option<f64>,
    /// The TACAN channel.
    pub channel: Option<i64>,
    pub x: Option<f64>,
    pub z: Option<f64>,
    pub alt: Option<f64>,
}

/// A radio station, such as the tower.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RadioInfo {
    pub id: Option<String>,
    /// Its English callsign, or the first language's where it has none.
    pub callsign: Option<String>,
    /// HF, the low VHF band (FM), the high VHF band and UHF.
    pub hf_hz: Option<f64>,
    pub fm_hz: Option<f64>,
    pub vhf_hz: Option<f64>,
    pub uhf_hz: Option<f64>,
}

/// A parking stand. Its size fields are null where the files give it no
/// class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StandInfo {
    pub name: String,
    pub x: f64,
    pub z: f64,
    pub class: Option<String>,
    pub width: Option<f64>,
    pub length: Option<f64>,
    /// The tallest airframe it takes; null where the class gives none.
    pub height: Option<f64>,
    pub helicopters: Option<bool>,
    pub airplanes: Option<bool>,
    /// Always null: no file says whether a stand is a shelter.
    pub shelter: Option<bool>,
    /// Whether the airframe `fits` names fits: false where any known size
    /// or use fails it; null without `fits`, or where none fails it but one
    /// is unknown.
    pub fits: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StandSize {
    pub width: Option<f64>,
    pub length: Option<f64>,
    pub count: u32,
}

// validate_placements

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ValidatePlacementsRequest {
    /// At most 10 000.
    pub placements: Vec<Placement>,
    /// Also return the answer as GeoJSON.
    #[serde(default)]
    pub geojson: bool,
}

/// Something to be placed: its shape at `x, z`, which is also its label
/// point, and what it needs of the ground under it. The tool knows nothing
/// of units: the caller gives the size and the needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Placement {
    pub id: String,
    pub x: f64,
    pub z: f64,
    /// Degrees from north towards east; turns only a `Rect`.
    #[serde(default)]
    pub heading: f64,
    #[serde(default)]
    pub shape: PlacementShape,
    #[serde(default)]
    pub on: Underfoot,
    /// Degrees, 0 to 90: the steepest slope allowed under the shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_slope_deg: Option<f64>,
    /// Meters: the shallowest water allowed under the shape, for `on:
    /// water` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_depth: Option<f64>,
}

/// Lengths are meters, at most 10 km.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum PlacementShape {
    /// The position alone.
    #[default]
    Point,
    /// Centered on the position, `depth` along `heading` and `width`
    /// across it.
    Rect { width: f64, depth: f64 },
    /// Centered on the position.
    Circle { radius: f64 },
    /// Absolute DCS `[x, z]` vertices, 3 to 1 000, closed by the first,
    /// inside by the even-odd rule.
    Polygon { points: Vec<[f64; 2]> },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Underfoot {
    #[default]
    Land,
    Water,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ValidatePlacementsResponse {
    #[serde(flatten)]
    pub provenance: Provenance,
    /// In the order of the request's.
    pub placements: Vec<PlacementCheck>,
    /// A point per placement with its issues, its shape unless it is a
    /// point, and each object it meets, when `geojson` was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geojson: Option<serde_json::Value>,
}

/// One placement checked. The ground fields read every cell the shape
/// covers part of, each at its center, or a point's own ground; null where
/// none of them has any.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlacementCheck {
    pub id: String,
    /// No issues.
    pub ok: bool,
    /// The cells read: 1 for a point.
    pub cells: u32,
    /// Degrees: the steepest cell.
    pub max_slope_deg: Option<f64>,
    /// Meters: the shallowest water, null where none is water.
    pub min_depth: Option<f64>,
    /// Each cover class under the shape, in the order the classes are
    /// listed: a placement in forest says so here.
    pub cover: Vec<CoverClass>,
    /// The scenery objects the shape meets; the issues list the nearest
    /// 100 to the position.
    pub objects_met: u32,
    pub issues: Vec<PlacementIssue>,
}

/// One thing wrong with a placement and what it is. Where an issue names a
/// place, `x, z` is the offending cell's center, or the point's own
/// position, nearest the placement's; for `slope` the steepest cell and
/// for `shallow` the shallowest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlacementIssue {
    /// Under the shape, ground no mesh covers.
    NoGround { cells: u32, x: f64, z: f64 },
    /// For `on: land`, water of this class under the shape.
    Water {
        water: WaterClass,
        cells: u32,
        x: f64,
        z: f64,
    },
    /// For `on: water`, land under the shape.
    Land { cells: u32, x: f64, z: f64 },
    /// For `on: water`, water shallower than `min_depth`; `depth` is the
    /// shallowest.
    Shallow {
        depth: f64,
        min_depth: f64,
        cells: u32,
        x: f64,
        z: f64,
    },
    /// The steepest cell, over `max_slope_deg`.
    Slope {
        slope_deg: f64,
        max_slope_deg: f64,
        x: f64,
        z: f64,
    },
    /// A scenery object the shape meets: a building, wall or other
    /// structure of the objects pool by its box, a power-line pole by its
    /// box, a tree by the circle inscribed in its box.
    Scenery {
        pool: ScenePool,
        object: SceneryObject,
    },
    /// The airfield's paved surface. The terrain's files mark runway,
    /// taxiway and apron alike, so the issue names the runway whose
    /// centerline is nearest the shape, and how near.
    Pavement {
        airdrome_id: Option<i64>,
        /// Its `name_id`; null with the runway when the terrain lists none.
        airdrome: Option<String>,
        runway: Option<String>,
        centerline_m: Option<f64>,
    },
}

/// A scenery pool, by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScenePool {
    Objects,
    Lines,
    Trees,
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
            geojson: None,
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
