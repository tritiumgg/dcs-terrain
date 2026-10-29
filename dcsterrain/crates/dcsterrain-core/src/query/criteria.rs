//! The siting criteria: each criterion's term at a site, and which sites
//! pass every hard test.
//!
//! A site is a candidate position with one of the footprint's orientations.
//! The work runs cheapest first, and a site leaves at the first hard test
//! it fails:
//!
//! 1. The layers a criterion reads at the site's own cell are derived over
//!    the cells of every candidate at once, as map algebra does. Each grid
//!    tile then gets a zone map: its least and greatest value of each such
//!    layer, or the set of codes it holds. A tile where no cell can pass a
//!    criterion is skipped whole, before its horizon is computed, its
//!    scenery read or a sightline drawn from it. The skip is exact, never a
//!    guess: such a criterion's value at a site is its layer's value at the
//!    site's cell, or for a footprint test the worst over cells that include
//!    the site's own, so a tile skipped held no site that passes.
//! 2. The horizon criteria, from the horizon tiles of the tiles left, each
//!    tile zone-mapped again.
//! 3. The rest, per site, in order of cost over the share of sites each
//!    fails, measured on the first sites; the order changes how soon a site
//!    leaves, never whether it passes.
//! 4. The sightlines, last, for the sites that pass everything else.
//!
//! `Mode::Every` skips nothing and leaves no term out: the oracle the
//! pruned evaluation is checked against.

use std::collections::{BTreeMap, HashSet};

use super::refused;
use super::region::airdromes;
use super::scenery::{OBJECT_REACH_M, Rect, footprint as object_rect};
use super::sightline::exact;
use super::visible::altitude;
use crate::classes::Class;
use crate::grid::{Grid, Window, box_distance};
use crate::horizon;
use crate::layers::{Cells, DISTANCE_CAP_M, Derived, Raster, TPI_300, TPI_2000, Target, derive};
use crate::source::{Layer, Refusal, Source};
use crate::types::{Criterion, Footprint, Issue, Point3, Term, TpiClass, WaterClass};

/// Sites evaluated in full before the order of the per-site tests is fixed
/// from the share of them each test failed.
const SAMPLE: usize = 256;
/// The mean margin over a masked sector's floor that scores 1.
pub const MASK_MARGIN_FULL_DEG: f64 = 10.0;
/// The longest side a footprint may have.
pub const FOOTPRINT_MAX_M: f64 = 10_000.0;
/// The side of a bucket of the objects' index.
const BUCKET_M: f64 = 250.0;

/// Whether to skip what cannot pass, or evaluate everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Only sites that pass every hard test are returned, each with every
    /// term.
    Prune,
    /// Every site with every term, nothing skipped.
    Every,
}

/// One candidate at one orientation, with a term per criterion in the
/// order asked, `valid` first when it was not asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct Site {
    pub x: f64,
    pub z: f64,
    pub orientation_deg: f64,
    pub terms: Vec<Term>,
    pub pass: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    /// Candidate positions.
    pub candidates: u64,
    /// Candidates with an orientation that passes.
    pub candidates_passed: u64,
    /// Grid tiles holding a candidate.
    pub tiles: u64,
    /// Tiles skipped whole.
    pub tiles_skipped: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Evaluation {
    /// In candidate order, then orientation order.
    pub sites: Vec<Site>,
    pub stats: Stats,
}

/// What a criterion tests, compiled against the terrain.
#[derive(Debug, Clone)]
enum Kind {
    /// Every footprint cell valid.
    Valid,
    /// The steepest footprint cell.
    SlopeMax(f64),
    /// Over the footprint's corners and center.
    SpanMax(f64),
    /// Every footprint cell's code of `layer` among the bits of `allowed`.
    Codes {
        layer: Layer,
        allowed: u64,
    },
    /// The value of center raster `raster` at the site's cell.
    Center {
        raster: usize,
        test: Test,
    },
    /// The landform class at the site's cell, among the bits of `allowed`.
    Tpi {
        allowed: u8,
        t: f64,
    },
    /// To the nearest of `at`.
    Airdrome {
        at: Vec<[f64; 2]>,
        min: f64,
        max: f64,
    },
    /// Structures of `class`, or of any class, within `radius` of the site.
    Count {
        class: Option<Class>,
        radius: f64,
        max: u32,
    },
    /// Structures whose box meets the footprint.
    InFootprint {
        max: u32,
    },
    Enclosure(f64),
    /// Every bin of `bins` at least `min_elev`.
    Masked {
        bins: Vec<usize>,
        min_elev: f64,
    },
    /// The share of `ends` in sight of the site raised `own`. Each end is
    /// x, z and its altitude, or its height above the ground when `agl`.
    Sight {
        ends: Vec<[f64; 3]>,
        own: f64,
        limit: f64,
        /// `not_visible_from`: the share at most `limit`; else at least.
        at_most: bool,
    },
}

/// A test of one value at the site's cell, and its soft score.
#[derive(Debug, Clone, Copy)]
enum Test {
    /// At least `min`, scoring the value: a share.
    Share { min: f64 },
    /// From `min` to `max`, scoring 1 at `min` and 0 at `max`.
    Band { min: f64, max: f64 },
    /// At least `m`, scoring `min(1, v / 2m)`.
    Beyond { m: f64 },
}

impl Test {
    /// The values that pass.
    fn range(self) -> (f64, f64) {
        match self {
            Test::Share { min } => (min, f64::INFINITY),
            Test::Band { min, max } => (min, max),
            Test::Beyond { m } => (m, f64::INFINITY),
        }
    }

    fn score(self, v: f64) -> f64 {
        let s = match self {
            Test::Share { .. } => v,
            Test::Band { min, max } if max > min => (max - v) / (max - min),
            Test::Band { .. } => 1.0,
            Test::Beyond { m } if m > 0.0 => v / (2.0 * m),
            Test::Beyond { .. } => 1.0,
        };
        unit(s)
    }
}

/// `s` held to 0..1, and 0 where it is undefined.
fn unit(s: f64) -> f64 {
    if s.is_nan() { 0.0 } else { s.clamp(0.0, 1.0) }
}

/// `1 - v / limit`, held to 0..1; at a limit of 0, 1 when `v` is 0.
fn under(v: f64, limit: f64) -> f64 {
    if limit > 0.0 {
        unit(1.0 - v / limit)
    } else {
        unit(f64::from(u8::from(v <= limit)))
    }
}

#[derive(Debug, Clone)]
struct Check {
    name: String,
    kind: Kind,
}

impl Check {
    /// Whether it differs between a site's orientations.
    fn per_orientation(&self) -> bool {
        matches!(
            self.kind,
            Kind::Valid
                | Kind::SlopeMax(_)
                | Kind::SpanMax(_)
                | Kind::Codes { .. }
                | Kind::InFootprint { .. }
        )
    }

    fn is_sight(&self) -> bool {
        matches!(self.kind, Kind::Sight { .. })
    }

    /// Rough cost of one evaluation, in center lookups.
    fn cost(&self, footprint_cells: f64) -> f64 {
        match &self.kind {
            Kind::Valid | Kind::SlopeMax(_) | Kind::Codes { .. } => footprint_cells,
            Kind::SpanMax(_) => 20.0,
            Kind::Center { .. } | Kind::Tpi { .. } | Kind::Enclosure(_) => 1.0,
            Kind::Airdrome { at, .. } => at.len() as f64,
            Kind::Masked { bins, .. } => bins.len() as f64,
            Kind::Count { .. } | Kind::InFootprint { .. } => 100.0,
            Kind::Sight { ends, .. } => 10_000.0 * ends.len() as f64,
        }
    }
}

fn bad(path: String, message: &str) -> Issue {
    Issue::new("invalid_criterion", message, &path)
}

/// The criteria compiled, `valid` first unless asked for, with the center
/// rasters they read.
fn compile(
    src: &dyn Source,
    criteria: &[Criterion],
) -> Result<(Vec<Check>, Vec<Derived>), Vec<Issue>> {
    let mut checks = Vec::new();
    let mut wants: Vec<Derived> = Vec::new();
    let mut issues = Vec::new();
    let mut want = |d: Derived| match wants.iter().position(|w| *w == d) {
        Some(i) => i,
        None => {
            wants.push(d);
            wants.len() - 1
        }
    };
    if !criteria.contains(&Criterion::Valid) {
        checks.push(Check {
            name: "valid".to_string(),
            kind: Kind::Valid,
        });
    }
    for (k, c) in criteria.iter().enumerate() {
        let at = |field: &str| format!("criteria.{k}.params.{field}");
        let length = |v: f64, field: &str, issues: &mut Vec<Issue>| {
            if !(v.is_finite() && v >= 0.0) {
                issues.push(bad(at(field), "must be a finite length, 0 or more"));
            }
        };
        let band = |min: f64, max: f64, issues: &mut Vec<Issue>| {
            length(min, "min", issues);
            length(max, "max", issues);
            if min > max {
                issues.push(bad(at("min"), "min lies past max"));
            }
        };
        let fraction = |v: f64, field: &str, issues: &mut Vec<Issue>| {
            if !(0.0..=1.0).contains(&v) {
                issues.push(bad(at(field), "must be a fraction from 0 to 1"));
            }
        };
        let nonempty = |n: usize, field: &str, issues: &mut Vec<Issue>| {
            if n == 0 {
                issues.push(bad(at(field), "names nothing, so no site could pass"));
            }
        };
        let kind = match c {
            Criterion::Valid => Kind::Valid,
            &Criterion::SlopeMax { deg } => {
                if !(0.0..=90.0).contains(&deg) {
                    issues.push(bad(at("deg"), "must be an angle from 0 to 90 degrees"));
                }
                Kind::SlopeMax(deg)
            }
            &Criterion::SpanMax { m } => {
                length(m, "m", &mut issues);
                Kind::SpanMax(m)
            }
            Criterion::SurfaceIn { classes } => {
                nonempty(classes.len(), "classes", &mut issues);
                let code = |w: &WaterClass| match w {
                    WaterClass::Land => 0,
                    WaterClass::Lake => 1,
                    WaterClass::Sea => 2,
                    WaterClass::River => 3,
                };
                Kind::Codes {
                    layer: Layer::Water,
                    allowed: classes.iter().fold(0, |m, w| m | 1 << code(w)),
                }
            }
            Criterion::SurfaceEnumIn { values } => {
                nonempty(values.len(), "values", &mut issues);
                // 0 is the code of a cell no mesh covers, which no site
                // stands on.
                for (i, &v) in values.iter().enumerate() {
                    if !(1..64).contains(&v) {
                        issues.push(bad(
                            at(&format!("values.{i}")),
                            "must be a surface code from 1 to 63",
                        ));
                    }
                }
                Kind::Codes {
                    layer: Layer::Surface,
                    allowed: values
                        .iter()
                        .filter(|&&v| (1..64).contains(&v))
                        .fold(0, |m, &v| m | 1 << v),
                }
            }
            Criterion::CoverIn { classes } => {
                nonempty(classes.len(), "classes", &mut issues);
                Kind::Codes {
                    layer: Layer::Cover,
                    allowed: classes.iter().fold(0, |m, c| m | 1 << c.cover().code()),
                }
            }
            &Criterion::CoverFractionMin { class, radius, min } => {
                length(radius, "radius", &mut issues);
                fraction(min, "min", &mut issues);
                Kind::Center {
                    raster: want(Derived::CoverFraction {
                        cover: class.cover(),
                        radius,
                    }),
                    test: Test::Share { min },
                }
            }
            &Criterion::DistCoverBand { class, min, max } => {
                band(min, max, &mut issues);
                distance(&mut want, Target::Cover(class.cover()), min, max)
            }
            &Criterion::DistRoadBand { min, max } => {
                band(min, max, &mut issues);
                distance(&mut want, Target::Road, min, max)
            }
            &Criterion::DistRailBand { min, max } => {
                band(min, max, &mut issues);
                distance(&mut want, Target::Rail, min, max)
            }
            &Criterion::DistWaterMin { m } => {
                length(m, "m", &mut issues);
                // Searched to where the score saturates, so no site's
                // answer reads the cap; 5 km at the least, as a distance
                // with no maximum is.
                let cap = (2.0 * m).max(DISTANCE_CAP_M);
                Kind::Center {
                    raster: want(Derived::Distance {
                        to: Target::Water,
                        cap,
                    }),
                    test: Test::Beyond { m },
                }
            }
            Criterion::DistAirdromeBand { min, max, airdrome } => {
                let (min, max) = (*min, *max);
                band(min, max, &mut issues);
                match airdromes(src, airdrome.as_ref(), &at("airdrome")) {
                    Ok(at) => Kind::Airdrome { at, min, max },
                    Err(mut i) => {
                        issues.append(&mut i);
                        continue;
                    }
                }
            }
            Criterion::SceneryMax { class, radius, max } => {
                length(*radius, "radius", &mut issues);
                let found = Class::ALL
                    .into_iter()
                    .find(|c| c.name() == class && c.is_structure());
                if found.is_none() {
                    let all: Vec<_> = Class::ALL
                        .iter()
                        .filter(|c| c.is_structure())
                        .map(|c| c.name())
                        .collect();
                    issues.push(Issue::new(
                        "unknown_class",
                        &format!("no class `{class}`; the classes are {}", all.join(", ")),
                        &at("class"),
                    ));
                }
                Kind::Count {
                    class: found,
                    radius: *radius,
                    max: *max,
                }
            }
            &Criterion::SceneryInFootprintMax { max } => Kind::InFootprint { max },
            &Criterion::ClearingMax { radius, max } => {
                length(radius, "radius", &mut issues);
                Kind::Count {
                    class: None,
                    radius,
                    max,
                }
            }
            &Criterion::EnclosureMin { deg } => {
                if !deg.is_finite() {
                    issues.push(bad(at("deg"), "must be a finite angle"));
                }
                Kind::Enclosure(deg)
            }
            &Criterion::MaskedFrom {
                bearing_deg,
                half_width_deg,
                min_elev_deg,
            } => {
                for (v, field) in [(bearing_deg, "bearing_deg"), (min_elev_deg, "min_elev_deg")] {
                    if !v.is_finite() {
                        issues.push(bad(at(field), "must be a finite angle"));
                    }
                }
                if !(0.0..=180.0).contains(&half_width_deg) {
                    issues.push(bad(
                        at("half_width_deg"),
                        "must be an angle from 0 to 180 degrees",
                    ));
                }
                Kind::Masked {
                    bins: sector(bearing_deg, half_width_deg),
                    min_elev: min_elev_deg,
                }
            }
            Criterion::NotVisibleFrom {
                observers,
                max_fraction,
                own_alt_agl,
            } => {
                fraction(*max_fraction, "max_fraction", &mut issues);
                length(*own_alt_agl, "own_alt_agl", &mut issues);
                nonempty(observers.len(), "observers", &mut issues);
                let points: Vec<Point3> = observers
                    .iter()
                    .map(|o| Point3 {
                        x: o.x,
                        z: o.z,
                        alt: None,
                        alt_agl: Some(o.alt_agl),
                    })
                    .collect();
                match ends(src, &points, &at("observers")) {
                    Ok(ends) => Kind::Sight {
                        ends,
                        own: *own_alt_agl,
                        limit: *max_fraction,
                        at_most: true,
                    },
                    Err(mut i) => {
                        issues.append(&mut i);
                        continue;
                    }
                }
            }
            Criterion::VisibleTo {
                targets,
                min_fraction,
                own_alt_agl,
            } => {
                fraction(*min_fraction, "min_fraction", &mut issues);
                length(*own_alt_agl, "own_alt_agl", &mut issues);
                nonempty(targets.len(), "targets", &mut issues);
                match ends(src, targets, &at("targets")) {
                    Ok(ends) => Kind::Sight {
                        ends,
                        own: *own_alt_agl,
                        limit: *min_fraction,
                        at_most: false,
                    },
                    Err(mut i) => {
                        issues.append(&mut i);
                        continue;
                    }
                }
            }
            Criterion::TpiClassIn {
                classes,
                threshold_m,
            } => {
                nonempty(classes.len(), "classes", &mut issues);
                length(*threshold_m, "threshold_m", &mut issues);
                want(TPI_300);
                want(TPI_2000);
                Kind::Tpi {
                    allowed: classes.iter().fold(0, |m, &c| m | 1 << tpi_code(c)),
                    t: *threshold_m,
                }
            }
        };
        checks.push(Check {
            name: name(c),
            kind,
        });
    }
    if issues.is_empty() {
        Ok((checks, wants))
    } else {
        Err(issues)
    }
}

/// A distance band, searched one cell past its maximum so a cell with
/// nothing nearer reads past the band rather than on its edge.
fn distance(want: &mut impl FnMut(Derived) -> usize, to: Target, min: f64, max: f64) -> Kind {
    let cap = max + f64::from(crate::source::CELL_SIZE);
    Kind::Center {
        raster: want(Derived::Distance { to, cap }),
        test: Test::Band { min, max },
    }
}

pub(super) fn name(c: &Criterion) -> String {
    let v = serde_json::to_value(c).expect("a criterion serializes");
    v["name"]
        .as_str()
        .expect("a criterion has a name")
        .to_string()
}

fn tpi_code(c: TpiClass) -> u8 {
    match c {
        TpiClass::Ridge => 0,
        TpiClass::Valley => 1,
        TpiClass::Upper => 2,
        TpiClass::Lower => 3,
        TpiClass::Mid => 4,
        TpiClass::Flat => 5,
    }
}

/// The landform class's code from the cell's relief and slope, or `None`
/// where any is undefined.
fn landform(tpi_300: f32, tpi_2000: f32, slope: f32, t: f64) -> Option<u8> {
    let (a, b, s) = (f64::from(tpi_300), f64::from(tpi_2000), f64::from(slope));
    if a.is_nan() || b.is_nan() || s.is_nan() {
        return None;
    }
    Some(if a > t {
        0
    } else if a < -t {
        1
    } else if b > t {
        2
    } else if b < -t {
        3
    } else if s > 5.0 {
        4
    } else {
        5
    })
}

/// The horizon bins whose center lines lie within `half` degrees of
/// `bearing`, or the one bin holding the bearing when none does.
fn sector(bearing: f64, half: f64) -> Vec<usize> {
    let w = 360.0 / horizon::BINS as f64;
    let off = |b: usize| {
        let d = (b as f64 * w - bearing).rem_euclid(360.0);
        d.min(360.0 - d)
    };
    let bins: Vec<usize> = (0..horizon::BINS).filter(|&b| off(b) <= half).collect();
    if bins.is_empty() && bearing.is_finite() {
        vec![(bearing / w).round().rem_euclid(horizon::BINS as f64) as usize]
    } else {
        bins
    }
}

/// Each point's x, z and altitude, standing on the mesh's own height, as
/// `visible` places its ends.
fn ends(src: &dyn Source, points: &[Point3], path: &str) -> Result<Vec<[f64; 3]>, Vec<Issue>> {
    // Only a finite point has ground to ask for; the rest are refused below.
    let finite: Vec<usize> = (0..points.len())
        .filter(|&i| points[i].x.is_finite() && points[i].z.is_finite())
        .collect();
    let xz: Vec<[f64; 2]> = finite.iter().map(|&i| [points[i].x, points[i].z]).collect();
    let mut ground = vec![None; points.len()];
    for (&i, g) in finite.iter().zip(src.points(&xz).map_err(refused)?) {
        ground[i] = g.map(|g| g.height);
    }
    let mut out = Vec::with_capacity(points.len());
    let mut issues = Vec::new();
    for (i, (p, g)) in points.iter().zip(ground).enumerate() {
        match altitude(p, g, &format!("{path}.{i}")) {
            Ok((alt, _)) => out.push([p.x, p.z, alt]),
            Err(e) => issues.push(e),
        }
    }
    if issues.is_empty() {
        Ok(out)
    } else {
        Err(issues)
    }
}

/// One candidate position.
#[derive(Debug, Clone, Copy)]
struct Cand {
    x: f64,
    z: f64,
    row: i64,
    col: i64,
}

/// A structure, for counting near a site and testing against a footprint.
struct Obj {
    x: f64,
    z: f64,
    class: Class,
    rect: Rect,
}

/// Structures bucketed on a square grid, for the ones near a point.
#[derive(Default)]
struct Objects {
    origin: [f64; 2],
    buckets: BTreeMap<(i64, i64), Vec<Obj>>,
}

impl Objects {
    fn read(src: &dyn Source, b: [f64; 4]) -> Result<Objects, Refusal> {
        let mut seen = HashSet::new();
        let mut o = Objects {
            origin: [b[0], b[1]],
            buckets: BTreeMap::new(),
        };
        let mut found = Vec::new();
        src.objects_in(b, &mut |obj| {
            if obj.class.is_structure() && seen.insert(obj.id) {
                found.push(Obj {
                    x: obj.pos[0],
                    z: obj.pos[2],
                    class: obj.class,
                    rect: object_rect(obj),
                });
            }
        })?;
        for obj in found {
            let k = o.key(obj.x, obj.z);
            o.buckets.entry(k).or_default().push(obj);
        }
        Ok(o)
    }

    fn key(&self, x: f64, z: f64) -> (i64, i64) {
        (
            ((x - self.origin[0]) / BUCKET_M).floor() as i64,
            ((z - self.origin[1]) / BUCKET_M).floor() as i64,
        )
    }

    /// Every object positioned in the box `b`, and perhaps a few beyond it.
    fn near(&self, b: [f64; 4]) -> impl Iterator<Item = &Obj> {
        let (r0, c0) = self.key(b[0], b[1]);
        let (r1, c1) = self.key(b[2], b[3]);
        self.buckets
            .range((r0, i64::MIN)..=(r1, i64::MAX))
            .filter(move |((_, c), _)| (c0..=c1).contains(c))
            .flat_map(|(_, v)| v.iter())
    }
}

/// What the evaluation reads, derived once.
struct Ctx<'a> {
    src: &'a dyn Source,
    g: Grid,
    footprint: Footprint,
    /// Over the footprints' cells.
    valid: Raster,
    slope: Option<Raster>,
    height: Option<Window>,
    codes: Vec<Window>,
    /// Over the candidates' cells, as `compile` lists them.
    centers: Vec<Raster>,
    tpi: Option<(usize, usize)>,
    /// By grid tile, the horizon tiles read.
    horizon: BTreeMap<(i64, i64), Vec<u8>>,
    objects: Objects,
}

impl Ctx<'_> {
    /// The footprint at `deg` on the candidate.
    fn rect(&self, c: &Cand, deg: f64) -> Rect {
        Rect::new(
            [c.x, c.z],
            deg.to_radians(),
            self.footprint.depth,
            self.footprint.width,
        )
    }

    /// The cells whose centers lie in the footprint, and the candidate's
    /// own cell however small the footprint.
    fn cells(&self, c: &Cand, deg: f64) -> Vec<(i64, i64)> {
        let r = self.rect(c, deg);
        let cs = f64::from(self.g.cell_size);
        let b = r.bbox();
        let row = |x: f64| ((x - self.g.origin_x) / cs).floor() as i64;
        let col = |z: f64| ((z - self.g.origin_z) / cs).floor() as i64;
        let mut out = vec![(c.row, c.col)];
        for i in row(b[0])..=row(b[2]) {
            for j in col(b[1])..=col(b[3]) {
                let p = [
                    self.g.origin_x + (i as f64 + 0.5) * cs,
                    self.g.origin_z + (j as f64 + 0.5) * cs,
                ];
                if (i, j) != (c.row, c.col) && r.contains(p) {
                    out.push((i, j));
                }
            }
        }
        out
    }

    fn center(&self, raster: usize, c: &Cand) -> f64 {
        self.centers[raster]
            .get(c.row, c.col)
            .map_or(f64::NAN, f64::from)
    }

    /// The horizon bins of the horizon cell holding the candidate's cell,
    /// `None` where its center is outside the mesh or off the grid.
    fn bins(&self, c: &Cand) -> Option<&[u8]> {
        let ts = i64::from(self.g.tile_size);
        let t = self
            .horizon
            .get(&(c.row.div_euclid(ts), c.col.div_euclid(ts)))?;
        let n = i64::from(horizon::tile_cells(&self.g));
        let span = i64::from(horizon::SPAN);
        let k = ((c.row % ts) / span * n + (c.col % ts) / span) as usize * horizon::BINS;
        let bins = &t[k..k + horizon::BINS];
        (bins[0] != horizon::UNDEFINED).then_some(bins)
    }

    /// The term of check `k` at the candidate, at orientation `deg` where it
    /// depends on one.
    fn term(&self, k: &Check, c: &Cand, deg: f64, cells: &mut Option<Vec<(i64, i64)>>) -> Term {
        let mut fp = || -> Vec<(i64, i64)> {
            if cells.is_none() {
                *cells = Some(self.cells(c, deg));
            }
            cells.clone().expect("just made")
        };
        let (value, pass, score) = match &k.kind {
            Kind::Valid => {
                let cells = fp();
                let ok = cells
                    .iter()
                    .filter(|&&(r, k)| self.valid.get(r, k) == Some(1.0))
                    .count();
                let share = ok as f64 / cells.len() as f64;
                (share, ok == cells.len(), share)
            }
            &Kind::SlopeMax(deg) => {
                let slope = self.slope.as_ref().expect("derived for slope_max");
                let worst = fp().iter().fold(0.0f64, |m, &(r, k)| {
                    let s = slope.get(r, k).map_or(f64::NAN, f64::from);
                    if s.is_nan() || m.is_nan() {
                        f64::NAN
                    } else {
                        m.max(s)
                    }
                });
                (worst, worst <= deg, under(worst, deg))
            }
            &Kind::SpanMax(m) => {
                let w = self.height.as_ref().expect("read for span_max");
                let r = self.rect(c, deg);
                let mut pts = r.corners().to_vec();
                pts.push([c.x, c.z]);
                let hs: Option<Vec<f64>> = pts
                    .iter()
                    .map(|p| w.value_at(&self.g, p[0], p[1]))
                    .collect();
                let span = hs.map_or(f64::NAN, |hs| {
                    let (lo, hi) = hs
                        .iter()
                        .fold((f64::MAX, f64::MIN), |(lo, hi), &h| (lo.min(h), hi.max(h)));
                    hi - lo
                });
                (span, span <= m, under(span, m))
            }
            &Kind::Codes { layer, allowed } => {
                let w = self
                    .codes
                    .iter()
                    .find(|w| w.layer == layer)
                    .expect("read for its criterion");
                let cells = fp();
                let ok = cells
                    .iter()
                    .filter(|&&(r, k)| w.get(r, k).is_some_and(|v| in_mask(v, allowed)))
                    .count();
                let share = ok as f64 / cells.len() as f64;
                (share, ok == cells.len(), share)
            }
            &Kind::Center { raster, test } => {
                let v = self.center(raster, c);
                let (lo, hi) = test.range();
                (v, v >= lo && v <= hi, test.score(v))
            }
            &Kind::Tpi { allowed, t } => {
                let (a, b) = self.tpi.expect("derived for tpi_class_in");
                let s = self
                    .slope
                    .as_ref()
                    .and_then(|s| s.get(c.row, c.col))
                    .unwrap_or(f32::NAN);
                let at = |i: usize| self.centers[i].get(c.row, c.col).unwrap_or(f32::NAN);
                match landform(at(a), at(b), s, t) {
                    Some(code) => {
                        let ok = allowed & 1 << code != 0;
                        (f64::from(code), ok, f64::from(u8::from(ok)))
                    }
                    None => (f64::NAN, false, 0.0),
                }
            }
            Kind::Airdrome { at, min, max } => {
                let d = at
                    .iter()
                    .map(|p| (p[0] - c.x).hypot(p[1] - c.z))
                    .fold(f64::INFINITY, f64::min);
                let t = Test::Band {
                    min: *min,
                    max: *max,
                };
                (d, d >= *min && d <= *max, t.score(d))
            }
            &Kind::Count { class, radius, max } => {
                let b = [c.x - radius, c.z - radius, c.x + radius, c.z + radius];
                let n = self
                    .objects
                    .near(b)
                    .filter(|o| class.is_none_or(|k| o.class == k))
                    .filter(|o| (o.x - c.x).hypot(o.z - c.z) <= radius)
                    .count() as f64;
                (n, n <= f64::from(max), under(n, f64::from(max)))
            }
            &Kind::InFootprint { max } => {
                let r = self.rect(c, deg);
                let b = r.bbox();
                let b = [
                    b[0] - OBJECT_REACH_M,
                    b[1] - OBJECT_REACH_M,
                    b[2] + OBJECT_REACH_M,
                    b[3] + OBJECT_REACH_M,
                ];
                let n = self.objects.near(b).filter(|o| r.meets(&o.rect)).count() as f64;
                (n, n <= f64::from(max), under(n, f64::from(max)))
            }
            &Kind::Enclosure(min) => {
                let e = self.bins(c).map_or(f64::NAN, |b| {
                    let sum: f64 = b.iter().filter_map(|&v| horizon::degrees(v)).sum();
                    sum / horizon::BINS as f64
                });
                let score = if min > 0.0 {
                    unit(e / (2.0 * min))
                } else {
                    f64::from(u8::from(e >= min))
                };
                (e, e >= min, score)
            }
            Kind::Masked { bins, min_elev } => {
                let (low, margin) = self
                    .bins(c)
                    .map_or((f64::NAN, f64::NAN), |b| masked(b, bins, *min_elev));
                (low, low >= *min_elev, unit(margin / MASK_MARGIN_FULL_DEG))
            }
            Kind::Sight { .. } => unreachable!("a sightline is drawn by `sight`"),
        };
        Term {
            name: k.name.clone(),
            value,
            pass,
            score,
        }
    }

    /// The share of a sightline check's ends seen from the candidate, its
    /// ground `ground`. Under `Mode::Prune` it stops at the first end that
    /// settles a fail, and the share is then a bound, not the answer.
    fn sight(&self, k: &Check, ground: Option<f64>, c: &Cand, stop: bool) -> Result<Term, Refusal> {
        let Kind::Sight {
            ends,
            own,
            limit,
            at_most,
        } = &k.kind
        else {
            unreachable!("only a sightline check is drawn")
        };
        let fail = |t: Term| Term { pass: false, ..t };
        let Some(ground) = ground else {
            return Ok(fail(Term {
                name: k.name.clone(),
                value: f64::NAN,
                pass: false,
                score: 0.0,
            }));
        };
        let site = [c.x, c.z, ground + own];
        let n = ends.len() as f64;
        let mut seen = 0usize;
        for (i, e) in ends.iter().enumerate() {
            let pieces = self.src.along([e[0], e[1]], [c.x, c.z])?;
            if exact(&pieces, *e, site).blocking.is_none() {
                seen += 1;
            }
            // Whether the rest can no longer bring the share inside its limit.
            let left = (ends.len() - i - 1) as f64;
            let lost = if *at_most {
                seen as f64 / n > *limit
            } else {
                (seen as f64 + left) / n < *limit
            };
            if stop && lost {
                return Ok(fail(Term {
                    name: k.name.clone(),
                    value: seen as f64 / n,
                    pass: false,
                    score: 0.0,
                }));
            }
        }
        let share = seen as f64 / n;
        let pass = if *at_most {
            share <= *limit
        } else {
            share >= *limit
        };
        let score = if *at_most { 1.0 - share } else { share };
        Ok(Term {
            name: k.name.clone(),
            value: share,
            pass,
            score,
        })
    }
}

/// The least bin of `sector` in degrees, and the mean margin of its bins
/// over `floor`.
fn masked(b: &[u8], sector: &[usize], floor: f64) -> (f64, f64) {
    let deg: Vec<f64> = sector
        .iter()
        .map(|&i| horizon::degrees(b[i]).unwrap_or(f64::NAN))
        .collect();
    let low = deg.iter().copied().fold(f64::INFINITY, f64::min);
    let mean = deg.iter().map(|d| d - floor).sum::<f64>() / deg.len() as f64;
    (low, mean)
}

fn in_mask(v: i32, allowed: u64) -> bool {
    (0..64).contains(&v) && allowed & 1 << v != 0
}

/// A tile's zone map of one center raster: its least and greatest defined
/// value over the cells given.
fn zone(r: &Raster, cells: Cells) -> Option<(f64, f64)> {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for row in cells.row0..cells.row0 + i64::from(cells.rows) {
        for col in cells.col0..cells.col0 + i64::from(cells.cols) {
            if let Some(v) = r.get(row, col).filter(|v| !v.is_nan()) {
                lo = lo.min(f64::from(v));
                hi = hi.max(f64::from(v));
            }
        }
    }
    (lo <= hi).then_some((lo, hi))
}

/// The cells of `a` that lie in `b`.
fn meet(a: Cells, b: Cells) -> Option<Cells> {
    let r0 = a.row0.max(b.row0);
    let c0 = a.col0.max(b.col0);
    let r1 = (a.row0 + i64::from(a.rows)).min(b.row0 + i64::from(b.rows));
    let c1 = (a.col0 + i64::from(a.cols)).min(b.col0 + i64::from(b.cols));
    (r0 < r1 && c0 < c1).then(|| Cells {
        row0: r0,
        col0: c0,
        rows: (r1 - r0) as u32,
        cols: (c1 - c0) as u32,
    })
}

/// Whether any cell of the tile's `cells` can pass check `k`, from what the
/// cells hold; `None` when the check is not judged from its cells.
fn can_pass(ctx: &Ctx, k: &Check, cells: Cells) -> Option<bool> {
    Some(match &k.kind {
        Kind::Valid => zone(&ctx.valid, cells).is_some_and(|(_, hi)| hi >= 1.0),
        &Kind::SlopeMax(deg) => zone(ctx.slope.as_ref()?, cells).is_some_and(|(lo, _)| lo <= deg),
        &Kind::Codes { layer, allowed } => {
            let w = ctx.codes.iter().find(|w| w.layer == layer)?;
            let mut held = false;
            'cells: for row in cells.row0..cells.row0 + i64::from(cells.rows) {
                for col in cells.col0..cells.col0 + i64::from(cells.cols) {
                    if w.get(row, col).is_some_and(|v| in_mask(v, allowed)) {
                        held = true;
                        break 'cells;
                    }
                }
            }
            held
        }
        &Kind::Center { raster, test } => {
            let (lo, hi) = test.range();
            zone(&ctx.centers[raster], cells).is_some_and(|(a, b)| b >= lo && a <= hi)
        }
        &Kind::Tpi { allowed, t } => {
            let (a, b) = ctx.tpi?;
            let slope = ctx.slope.as_ref()?;
            let mut held = false;
            'tpi: for row in cells.row0..cells.row0 + i64::from(cells.rows) {
                for col in cells.col0..cells.col0 + i64::from(cells.cols) {
                    let at = |r: &Raster| r.get(row, col).unwrap_or(f32::NAN);
                    let code = landform(at(&ctx.centers[a]), at(&ctx.centers[b]), at(slope), t);
                    if code.is_some_and(|c| allowed & 1 << c != 0) {
                        held = true;
                        break 'tpi;
                    }
                }
            }
            held
        }
        Kind::Airdrome { at, min, max } => {
            // The nearest airdrome from somewhere in the box lies between
            // the least distance from the box to any airdrome and the least
            // farthest distance.
            let b = cell_box(&ctx.g, cells);
            let near = |p: &[f64; 2]| box_distance(b, *p);
            let far = |p: &[f64; 2]| {
                let dx = (p[0] - b[0]).abs().max((p[0] - b[2]).abs());
                let dz = (p[1] - b[1]).abs().max((p[1] - b[3]).abs());
                dx.hypot(dz)
            };
            let lo = at.iter().map(near).fold(f64::INFINITY, f64::min);
            let hi = at.iter().map(far).fold(f64::INFINITY, f64::min);
            hi >= *min && lo <= *max
        }
        &Kind::Enclosure(min) => {
            let t = tile_horizon(ctx, cells)?;
            t.chunks(horizon::BINS)
                .filter(|b| b[0] != horizon::UNDEFINED)
                .any(|b| {
                    let sum: f64 = b.iter().filter_map(|&v| horizon::degrees(v)).sum();
                    sum / horizon::BINS as f64 >= min
                })
        }
        Kind::Masked { bins, min_elev } => {
            let t = tile_horizon(ctx, cells)?;
            t.chunks(horizon::BINS)
                .filter(|b| b[0] != horizon::UNDEFINED)
                .any(|b| masked(b, bins, *min_elev).0 >= *min_elev)
        }
        _ => return None,
    })
}

/// The horizon tile holding `cells`, which lie in one grid tile.
fn tile_horizon<'c>(ctx: &'c Ctx, cells: Cells) -> Option<&'c [u8]> {
    let ts = i64::from(ctx.g.tile_size);
    ctx.horizon
        .get(&(cells.row0.div_euclid(ts), cells.col0.div_euclid(ts)))
        .map(Vec::as_slice)
}

/// The box `[min_x, min_z, max_x, max_z]` the cells cover: a candidate may
/// stand anywhere in its cell.
fn cell_box(g: &Grid, c: Cells) -> [f64; 4] {
    let cs = f64::from(g.cell_size);
    [
        g.origin_x + c.row0 as f64 * cs,
        g.origin_z + c.col0 as f64 * cs,
        g.origin_x + ((c.row0 + i64::from(c.rows)) as f64) * cs,
        g.origin_z + ((c.col0 + i64::from(c.cols)) as f64) * cs,
    ]
}

/// Each candidate's sites under `criteria`, the footprint tried at each of
/// its orientations.
pub fn evaluate(
    src: &dyn Source,
    candidates: &[[f64; 2]],
    footprint: &Footprint,
    criteria: &[Criterion],
    mode: Mode,
) -> Result<Evaluation, Vec<Issue>> {
    let fp_ok = [footprint.width, footprint.depth]
        .iter()
        .all(|v| (0.0..=FOOTPRINT_MAX_M).contains(v))
        && !footprint.orientations_deg.is_empty()
        && footprint.orientations_deg.iter().all(|d| d.is_finite());
    if !fp_ok {
        return Err(vec![Issue::new(
            "invalid_request",
            &format!(
                "a footprint needs a size from 0 to {FOOTPRINT_MAX_M} m and one or more finite \
                 orientations"
            ),
            "footprint",
        )]);
    }
    if candidates
        .iter()
        .any(|p| !(p[0].is_finite() && p[1].is_finite()))
    {
        return Err(vec![Issue::new(
            "invalid_request",
            "every candidate position must be finite",
            "",
        )]);
    }
    let (checks, wants) = compile(src, criteria)?;
    let g = src.grid();
    let cs = f64::from(g.cell_size);
    let mut stats = Stats {
        candidates: candidates.len() as u64,
        ..Stats::default()
    };
    if candidates.is_empty() {
        return Ok(Evaluation {
            sites: Vec::new(),
            stats,
        });
    }
    let cands: Vec<Cand> = candidates
        .iter()
        .map(|&[x, z]| Cand {
            x,
            z,
            row: ((x - g.origin_x) / cs).floor() as i64,
            col: ((z - g.origin_z) / cs).floor() as i64,
        })
        .collect();
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for c in &cands {
        b = [b[0].min(c.x), b[1].min(c.z), b[2].max(c.x), b[3].max(c.z)];
    }
    let at = Cells::covering(&g, b);
    // Far enough for every footprint cell; one more for the bilinear
    // neighbors of a corner.
    let reach = (footprint.width.hypot(footprint.depth) / 2.0 / cs).ceil() as i64 + 2;
    let fp = Cells {
        row0: at.row0 - reach,
        col0: at.col0 - reach,
        rows: at.rows + 2 * reach as u32,
        cols: at.cols + 2 * reach as u32,
    };
    let has = |f: &dyn Fn(&Kind) -> bool| checks.iter().any(|c| f(&c.kind));
    let r = |e: Refusal| refused(e);
    let valid = derive(src, Derived::Valid, fp).map_err(r)?;
    let slope = if has(&|k| matches!(k, Kind::SlopeMax(_) | Kind::Tpi { .. })) {
        Some(derive(src, Derived::Slope, fp).map_err(r)?)
    } else {
        None
    };
    let height = if has(&|k| matches!(k, Kind::SpanMax(_))) {
        Some(Window::read(src, Layer::Height, fp.row0, fp.col0, fp.rows, fp.cols).map_err(r)?)
    } else {
        None
    };
    let mut codes = Vec::new();
    for layer in [Layer::Water, Layer::Surface, Layer::Cover] {
        if has(&|k| matches!(k, Kind::Codes { layer: l, .. } if *l == layer)) {
            codes.push(Window::read(src, layer, fp.row0, fp.col0, fp.rows, fp.cols).map_err(r)?);
        }
    }
    let mut centers = Vec::with_capacity(wants.len());
    for w in &wants {
        centers.push(derive(src, *w, at).map_err(r)?);
    }
    let tpi = has(&|k| matches!(k, Kind::Tpi { .. })).then(|| {
        let i = |d| wants.iter().position(|w| *w == d).expect("wanted");
        (i(TPI_300), i(TPI_2000))
    });
    let mut ctx = Ctx {
        src,
        g,
        footprint: footprint.clone(),
        valid,
        slope,
        height,
        codes,
        centers,
        tpi,
        horizon: BTreeMap::new(),
        objects: Objects::default(),
    };

    // The candidates by grid tile.
    let ts = i64::from(g.tile_size);
    let mut tiles: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
    for (i, c) in cands.iter().enumerate() {
        tiles
            .entry((c.row.div_euclid(ts), c.col.div_euclid(ts)))
            .or_default()
            .push(i);
    }
    stats.tiles = tiles.len() as u64;
    let tile_cells = |&(tx, tz): &(i64, i64)| {
        meet(
            Cells {
                row0: tx * ts,
                col0: tz * ts,
                rows: g.tile_size,
                cols: g.tile_size,
            },
            at,
        )
    };
    let horizon_checks = |k: &Check| matches!(k.kind, Kind::Enclosure(_) | Kind::Masked { .. });
    let skip = |ctx: &Ctx, key: &(i64, i64), stage: &dyn Fn(&Check) -> bool| {
        tile_cells(key).is_none_or(|cells| {
            checks
                .iter()
                .filter(|k| stage(k))
                .any(|k| can_pass(ctx, k, cells) == Some(false))
        })
    };
    if mode == Mode::Prune {
        tiles.retain(|key, _| !skip(&ctx, key, &|k| !horizon_checks(k)));
    }
    if checks.iter().any(horizon_checks) {
        let (tr, tc) = g.tiles();
        let on_grid: Vec<(u32, u32)> = tiles
            .keys()
            .filter(|&&(tx, tz)| {
                (0..i64::from(tr)).contains(&tx) && (0..i64::from(tc)).contains(&tz)
            })
            .map(|&(tx, tz)| (tx as u32, tz as u32))
            .collect();
        for (t, h) in on_grid.iter().zip(src.horizon(&on_grid).map_err(r)?) {
            ctx.horizon.insert((i64::from(t.0), i64::from(t.1)), h);
        }
        if mode == Mode::Prune {
            tiles.retain(|key, _| !skip(&ctx, key, &horizon_checks));
        }
    }
    stats.tiles_skipped = stats.tiles - tiles.len() as u64;
    let mut live: Vec<usize> = tiles.into_values().flatten().collect();
    live.sort_unstable();

    if has(&|k| matches!(k, Kind::Count { .. } | Kind::InFootprint { .. })) && !live.is_empty() {
        let radius = checks
            .iter()
            .map(|k| match k.kind {
                Kind::Count { radius, .. } => radius,
                _ => 0.0,
            })
            .fold(0.0, f64::max);
        let pad =
            radius.max(footprint.width.hypot(footprint.depth) / 2.0 + OBJECT_REACH_M) + BUCKET_M;
        let mut lb = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for &i in &live {
            let c = &cands[i];
            lb = [
                lb[0].min(c.x),
                lb[1].min(c.z),
                lb[2].max(c.x),
                lb[3].max(c.z),
            ];
        }
        ctx.objects =
            Objects::read(src, [lb[0] - pad, lb[1] - pad, lb[2] + pad, lb[3] + pad]).map_err(r)?;
    }

    // The per-site tests in order of cost over the share each fails on a
    // sample, the sightlines last whatever they fail.
    let fp_cells = (footprint.width * footprint.depth / (cs * cs)).max(1.0);
    if mode == Mode::Prune {
        let sample: Vec<usize> = live.iter().copied().take(SAMPLE).collect();
        let mut failed = vec![0usize; checks.len()];
        let mut tried = vec![0usize; checks.len()];
        for &i in &sample {
            for &deg in &footprint.orientations_deg {
                let mut cells = None;
                for (j, k) in checks.iter().enumerate() {
                    if k.is_sight()
                        || (!k.per_orientation() && deg != footprint.orientations_deg[0])
                    {
                        continue;
                    }
                    tried[j] += 1;
                    if !ctx.term(k, &cands[i], deg, &mut cells).pass {
                        failed[j] += 1;
                    }
                }
            }
        }
        let rank: Vec<f64> = checks
            .iter()
            .enumerate()
            .map(|(j, k)| {
                let fails = (failed[j] as f64 + 1.0) / (tried[j] as f64 + 2.0);
                k.cost(fp_cells) / fails
            })
            .collect();
        let mut order: Vec<usize> = (0..checks.len()).collect();
        order.sort_by(|&a, &b| {
            (checks[a].is_sight(), rank[a])
                .partial_cmp(&(checks[b].is_sight(), rank[b]))
                .expect("ranks are finite")
        });
        let ordered: Vec<Check> = order.iter().map(|&j| checks[j].clone()).collect();
        let back: Vec<usize> = {
            let mut back = vec![0; order.len()];
            for (pos, &j) in order.iter().enumerate() {
                back[j] = pos;
            }
            back
        };
        let sites = prune(&ctx, &cands, &live, &ordered, &mut stats)?;
        // Terms back in the order asked.
        let sites = sites
            .into_iter()
            .map(|mut s| {
                let terms = std::mem::take(&mut s.terms);
                s.terms = (0..terms.len()).map(|j| terms[back[j]].clone()).collect();
                s
            })
            .collect();
        return Ok(Evaluation { sites, stats });
    }
    let sites = every(&ctx, &cands, &checks, &mut stats)?;
    Ok(Evaluation { sites, stats })
}

/// An orientation that passed, with the terms found so far.
type Partial = (f64, Vec<Option<Term>>);

/// The sites of the candidates `live` that pass every check, `checks` in
/// the order to try them, their terms in that order.
fn prune(
    ctx: &Ctx,
    cands: &[Cand],
    live: &[usize],
    checks: &[Check],
    stats: &mut Stats,
) -> Result<Vec<Site>, Vec<Issue>> {
    let degs = &ctx.footprint.orientations_deg;
    // Candidates that pass all but the sightlines, with the terms of each
    // passing orientation.
    let mut through: Vec<(usize, Vec<Partial>)> = Vec::new();
    'cand: for &i in live {
        let c = &cands[i];
        let mut shared: Vec<Option<Term>> = vec![None; checks.len()];
        for (j, k) in checks.iter().enumerate() {
            if k.per_orientation() || k.is_sight() {
                continue;
            }
            let t = ctx.term(k, c, degs[0], &mut None);
            if !t.pass {
                continue 'cand;
            }
            shared[j] = Some(t);
        }
        let mut passing = Vec::new();
        'deg: for &deg in degs {
            let mut terms = shared.clone();
            let mut cells = None;
            for (j, k) in checks.iter().enumerate() {
                if !k.per_orientation() {
                    continue;
                }
                let t = ctx.term(k, c, deg, &mut cells);
                if !t.pass {
                    continue 'deg;
                }
                terms[j] = Some(t);
            }
            passing.push((deg, terms));
        }
        if !passing.is_empty() {
            through.push((i, passing));
        }
    }
    let sights: Vec<usize> = (0..checks.len())
        .filter(|&j| checks[j].is_sight())
        .collect();
    let ground = if sights.is_empty() || through.is_empty() {
        vec![None; through.len()]
    } else {
        let xz: Vec<[f64; 2]> = through
            .iter()
            .map(|(i, _)| [cands[*i].x, cands[*i].z])
            .collect();
        ctx.src
            .points(&xz)
            .map_err(refused)?
            .into_iter()
            .map(|g| g.map(|g| g.height))
            .collect()
    };
    let mut sites = Vec::new();
    'site: for ((i, passing), ground) in through.into_iter().zip(ground) {
        let c = &cands[i];
        let mut seen = Vec::new();
        for &j in &sights {
            let t = ctx.sight(&checks[j], ground, c, true).map_err(refused)?;
            if !t.pass {
                continue 'site;
            }
            seen.push((j, t));
        }
        stats.candidates_passed += 1;
        for (deg, mut terms) in passing {
            for (j, t) in &seen {
                terms[*j] = Some(t.clone());
            }
            sites.push(Site {
                x: c.x,
                z: c.z,
                orientation_deg: deg,
                terms: terms.into_iter().map(|t| t.expect("every term")).collect(),
                pass: true,
            });
        }
    }
    Ok(sites)
}

/// Every candidate's every site, with every term.
fn every(
    ctx: &Ctx,
    cands: &[Cand],
    checks: &[Check],
    stats: &mut Stats,
) -> Result<Vec<Site>, Vec<Issue>> {
    let xz: Vec<[f64; 2]> = cands.iter().map(|c| [c.x, c.z]).collect();
    let ground: Vec<Option<f64>> = if checks.iter().any(Check::is_sight) {
        ctx.src
            .points(&xz)
            .map_err(refused)?
            .into_iter()
            .map(|g| g.map(|g| g.height))
            .collect()
    } else {
        vec![None; cands.len()]
    };
    let mut sites = Vec::new();
    for (c, ground) in cands.iter().zip(ground) {
        let mut any = false;
        let mut shared: Vec<Option<Term>> = vec![None; checks.len()];
        for (j, k) in checks.iter().enumerate() {
            if k.is_sight() {
                shared[j] = Some(ctx.sight(k, ground, c, false).map_err(refused)?);
            } else if !k.per_orientation() {
                shared[j] = Some(ctx.term(k, c, ctx.footprint.orientations_deg[0], &mut None));
            }
        }
        for &deg in &ctx.footprint.orientations_deg {
            let mut cells = None;
            let terms: Vec<Term> = checks
                .iter()
                .enumerate()
                .map(|(j, k)| match &shared[j] {
                    Some(t) => t.clone(),
                    None => ctx.term(k, c, deg, &mut cells),
                })
                .collect();
            let pass = terms.iter().all(|t| t.pass);
            any |= pass;
            sites.push(Site {
                x: c.x,
                z: c.z,
                orientation_deg: deg,
                terms,
                pass,
            });
        }
        stats.candidates_passed += u64::from(any);
    }
    Ok(sites)
}

#[cfg(test)]
mod tests;
