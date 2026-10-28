//! The synthetic terrain as closed-form functions of position.
//!
//! Positions here are offsets from the grid origin, `north` and `east` in
//! metres, as the constants are written; `Theatre::abs` turns one into DCS
//! `x` and `z`. Every function is the value at a point, so a test can ask for
//! the expected value of any cell.

use std::fmt;

use super::*;
use crate::decode::rn4::{KIND_RAIL, KIND_ROAD};
use crate::roads;
use crate::source::Cover;

/// A setting `Theatre::new` refuses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthError(pub String);

impl fmt::Display for SynthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SynthError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theatre {
    pub seed: u32,
    pub size_km: u32,
    pub cell_size: u32,
}

/// One cell of every layer, nodata where the cell is outside the mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub height: i16,
    pub water: u8,
    pub surface: u8,
    pub cover: u8,
}

const OUTSIDE: Cell = Cell {
    height: HEIGHT_NODATA,
    water: WATER_NODATA,
    surface: SURFACE_NODATA,
    cover: 0,
};

/// One edge of a road or rail network, in DCS coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub id: u32,
    pub class: &'static str,
    pub a: u32,
    pub b: u32,
    /// `[x, z, alt]`.
    pub points: Vec<[f64; 3]>,
}

/// One scenery object, in DCS coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    pub id: u64,
    /// As the constants spell it; the writer lowercases it.
    pub model: &'static str,
    pub x: f64,
    pub alt: f64,
    pub z: f64,
    pub rotation: f64,
}

impl Theatre {
    /// The features sit on the 50 m lattice's cell centres, so any other
    /// cell size is refused rather than answered with roads that miss every
    /// cell.
    pub fn new(seed: u32, size_km: u32, cell_size: u32) -> Result<Self, SynthError> {
        if cell_size != CELL_SIZE_M {
            return Err(SynthError(format!(
                "the synthetic terrain's cell is always {CELL_SIZE_M} m, not {cell_size}"
            )));
        }
        if seed == 0 || seed >= PRNG_MODULUS {
            return Err(SynthError(format!(
                "the seed must be between 1 and {}, not {seed}",
                PRNG_MODULUS - 1
            )));
        }
        // Smaller would cut features off the north-east while the model
        // counts and polygons still describe all of them.
        if size_km < DEFAULT_SIZE_KM {
            return Err(SynthError(format!(
                "the theatre needs at least {DEFAULT_SIZE_KM} km to hold every feature, not {size_km}"
            )));
        }
        Ok(Theatre {
            seed,
            size_km,
            cell_size,
        })
    }

    pub fn size_m(&self) -> f64 {
        f64::from(self.size_km) * 1000.0
    }

    /// Cells along each side; the grid is square.
    pub fn cells(&self) -> u32 {
        self.size_km * 1000 / self.cell_size
    }

    /// DCS `x` and `z` of an offset from the grid origin.
    pub fn abs(&self, north: f64, east: f64) -> (f64, f64) {
        (f64::from(ORIGIN_X_M) + north, f64::from(ORIGIN_Z_M) + east)
    }

    /// The offset of a cell's centre.
    pub fn centre(&self, row: u32, col: u32) -> (f64, f64) {
        let c = f64::from(self.cell_size);
        ((f64::from(row) + 0.5) * c, (f64::from(col) + 0.5) * c)
    }

    pub fn covered(&self, north: f64, east: f64) -> bool {
        let hi = self.size_m() - MESH_MARGIN_M;
        (MESH_MARGIN_M..hi).contains(&north) && (MESH_MARGIN_M..hi).contains(&east)
    }

    /// The land surface: the plane and both hills, before any water.
    pub fn ground(&self, north: f64, east: f64) -> f64 {
        let hill = |h: f64, n0: f64, e0: f64| {
            let d2 = (north - n0).powi(2) + (east - e0).powi(2);
            h * (-d2 / (2.0 * HILL_SIGMA_M * HILL_SIGMA_M)).exp()
        };
        PLANE_BASE_M
            + PLANE_SLOPE * north
            + hill(HILL_A_HEIGHT_M, HILL_A_NORTH_M, HILL_A_EAST_M)
            + hill(HILL_B_HEIGHT_M, HILL_B_NORTH_M, HILL_B_EAST_M)
    }

    fn in_sea(&self, north: f64, east: f64) -> bool {
        north + east < COAST_M
    }

    fn in_lake(&self, north: f64, east: f64) -> bool {
        (north - LAKE_NORTH_M).hypot(east - LAKE_EAST_M) <= LAKE_RADIUS_M
    }

    fn in_river(&self, east: f64) -> bool {
        (east - RIVER_EAST_M).abs() <= RIVER_WIDTH_M / 2.0
    }

    /// The height of the winning surface: water level over the sea and the
    /// lake, the ground everywhere else, roads and the river included.
    pub fn height(&self, north: f64, east: f64) -> f64 {
        if self.in_sea(north, east) && !self.on_road(north, east) {
            SEA_LEVEL_M
        } else if self.in_lake(north, east) && !self.on_road(north, east) {
            LAKE_LEVEL_M
        } else {
            self.ground(north, east)
        }
    }

    /// A road strip wins over the water under it, as the mesh's road
    /// triangles do, so a cell of the bridge reads as land.
    pub fn water(&self, north: f64, east: f64) -> u8 {
        if self.on_road(north, east) {
            WATER_LAND
        } else if self.in_sea(north, east) {
            WATER_SEA
        } else if self.in_lake(north, east) {
            WATER_LAKE
        } else if self.in_river(east) {
            WATER_RIVER
        } else {
            WATER_LAND
        }
    }

    /// Within half a road's width of any road's centre line, between its
    /// ends. The railway is not a road strip.
    pub fn on_road(&self, north: f64, east: f64) -> bool {
        let half = ROAD_WIDTH_M / 2.0;
        let ew = (north - ROAD_EW_NORTH_M).abs() <= half
            && (ROAD_EW_WEST_EAST_M..=ROAD_EW_EAST_EAST_M).contains(&east);
        let ns = (east - ROAD_NS_EAST_M).abs() <= half
            && (ROAD_NS_SOUTH_NORTH_M..=ROAD_NS_NORTH_NORTH_M).contains(&north);
        let east_road = (east - ROAD_EAST_EAST_M).abs() <= half
            && (ROAD_EAST_SOUTH_NORTH_M..=ROAD_EAST_NORTH_NORTH_M).contains(&north);
        ew || ns || east_road
    }

    pub fn on_runway(&self, north: f64, east: f64) -> bool {
        (north - RUNWAY_NORTH_M).abs() <= RUNWAY_WIDTH_M / 2.0
            && (east - RUNWAY_EAST_M).abs() <= RUNWAY_LENGTH_M / 2.0
    }

    pub fn surface(&self, north: f64, east: f64) -> u8 {
        if self.on_runway(north, east) {
            SURFACE_RUNWAY
        } else if self.on_road(north, east) {
            SURFACE_ROAD
        } else if self.water(north, east) != WATER_LAND {
            SURFACE_WATER
        } else {
            SURFACE_LAND
        }
    }

    /// The ground cover, first match winning: water, the runway, then the
    /// town, forest, field and beach patches, else plain. A road keeps the
    /// cover of the ground under it.
    pub fn cover(&self, north: f64, east: f64) -> Cover {
        let in_box =
            |s: f64, n: f64, w: f64, e: f64| (s..n).contains(&north) && (w..e).contains(&east);
        let town = in_box(
            BUILT_UP_SOUTH_M,
            BUILT_UP_NORTH_M,
            BUILT_UP_WEST_M,
            BUILT_UP_EAST_M,
        );
        if self.water(north, east) != WATER_LAND {
            Cover::Water
        } else if self.on_runway(north, east) {
            Cover::Runway
        } else if town {
            Cover::Town
        } else if self.trees(north, east) > 0 {
            Cover::Forest
        } else if in_box(FIELD_SOUTH_M, FIELD_NORTH_M, FIELD_WEST_M, FIELD_EAST_M) {
            Cover::Field
        } else if in_box(BEACH_SOUTH_M, BEACH_NORTH_M, BEACH_WEST_M, BEACH_EAST_M) {
            Cover::Beach
        } else {
            Cover::Plain
        }
    }

    /// Trees in the cell whose centre this is.
    pub fn trees(&self, north: f64, east: f64) -> u16 {
        let in_box =
            |s: f64, n: f64, w: f64, e: f64| (s..n).contains(&north) && (w..e).contains(&east);
        let forest = in_box(FOREST_SOUTH_M, FOREST_NORTH_M, FOREST_WEST_M, FOREST_EAST_M);
        let hole = in_box(
            FOREST_HOLE_SOUTH_M,
            FOREST_HOLE_NORTH_M,
            FOREST_HOLE_WEST_M,
            FOREST_HOLE_EAST_M,
        );
        if forest && !hole {
            TREES_PER_CELL as u16
        } else {
            0
        }
    }

    /// Power-line poles in the cell whose centre this is.
    pub fn lines(&self, north: f64, east: f64) -> u16 {
        let half = f64::from(self.cell_size) / 2.0;
        if !(east - half..east + half).contains(&LINE_EAST_M) {
            return 0;
        }
        self.pole_norths()
            .filter(|n| (north - half..north + half).contains(n))
            .count() as u16
    }

    fn pole_norths(&self) -> impl Iterator<Item = f64> {
        let count = ((LINE_NORTH_M - LINE_SOUTH_M) / LINE_POLE_SPACING_M).floor() as u32 + 1;
        (0..count).map(|k| LINE_SOUTH_M + f64::from(k) * LINE_POLE_SPACING_M)
    }

    pub fn pole_count(&self) -> u32 {
        self.pole_norths().count() as u32
    }

    pub fn tree_count(&self) -> u32 {
        let cells = |a: f64, b: f64| ((b - a) / f64::from(self.cell_size)) as u32;
        let forest = cells(FOREST_SOUTH_M, FOREST_NORTH_M) * cells(FOREST_WEST_M, FOREST_EAST_M);
        let hole = cells(FOREST_HOLE_SOUTH_M, FOREST_HOLE_NORTH_M)
            * cells(FOREST_HOLE_WEST_M, FOREST_HOLE_EAST_M);
        (forest - hole) * TREES_PER_CELL
    }

    pub fn cell(&self, row: u32, col: u32) -> Cell {
        let (n, e) = self.centre(row, col);
        if row >= self.cells() || col >= self.cells() || !self.covered(n, e) {
            return OUTSIDE;
        }
        let h = (self.height(n, e) + 0.5).floor();
        Cell {
            height: h.clamp(f64::from(HEIGHT_MIN), f64::from(HEIGHT_MAX)) as i16,
            water: self.water(n, e),
            surface: self.surface(n, e),
            cover: self.cover(n, e) as u8,
        }
    }

    /// A polyline from `from` to `to` along one axis, a vertex at each end
    /// and at every multiple of the vertex spacing between, at ground height.
    fn polyline(&self, fixed: f64, from: f64, to: f64, along_north: bool) -> Vec<[f64; 3]> {
        let mut ts = vec![from];
        let mut t = (from / ROAD_VERTEX_SPACING_M).floor() * ROAD_VERTEX_SPACING_M;
        loop {
            t += ROAD_VERTEX_SPACING_M;
            if t >= to {
                break;
            }
            ts.push(t);
        }
        ts.push(to);
        ts.into_iter()
            .map(|t| {
                let (n, e) = if along_north { (t, fixed) } else { (fixed, t) };
                let (x, z) = self.abs(n, e);
                [x, z, self.ground(n, e)]
            })
            .collect()
    }

    /// The road network's edges, cut at the junctions and at the bridge ends,
    /// then the railway's, numbered in that order as one scan of the file.
    fn edges(&self) -> (Vec<Edge>, Vec<Edge>) {
        let node = |k: u32| ROAD_FIRST_NODE_ID + k;
        let (w, j, bw, be, e) = (node(0), node(1), node(2), node(3), node(4));
        let (s, n) = (node(5), node(6));
        let (rw, re) = (node(7), node(8));
        let (j2, s2, n2) = (node(9), node(10), node(11));
        let bridge_w = RIVER_EAST_M - BRIDGE_LENGTH_M / 2.0;
        let bridge_e = RIVER_EAST_M + BRIDGE_LENGTH_M / 2.0;
        let ew = |from, to| self.polyline(ROAD_EW_NORTH_M, from, to, false);
        let ns = |from, to| self.polyline(ROAD_NS_EAST_M, from, to, true);
        let east = |from, to| self.polyline(ROAD_EAST_EAST_M, from, to, true);
        let rail_north = ROAD_EW_NORTH_M - RAIL_SOUTH_OFFSET_M;
        let parts = [
            (ROAD_EW_CLASS, w, j, ew(ROAD_EW_WEST_EAST_M, ROAD_NS_EAST_M)),
            (ROAD_EW_CLASS, j, bw, ew(ROAD_NS_EAST_M, bridge_w)),
            (ROAD_EW_CLASS, bw, be, ew(bridge_w, bridge_e)),
            (ROAD_EW_CLASS, be, j2, ew(bridge_e, ROAD_EAST_EAST_M)),
            (
                ROAD_EW_CLASS,
                j2,
                e,
                ew(ROAD_EAST_EAST_M, ROAD_EW_EAST_EAST_M),
            ),
            (
                ROAD_NS_CLASS,
                s,
                j,
                ns(ROAD_NS_SOUTH_NORTH_M, ROAD_EW_NORTH_M),
            ),
            (
                ROAD_NS_CLASS,
                j,
                n,
                ns(ROAD_EW_NORTH_M, ROAD_NS_NORTH_NORTH_M),
            ),
            (
                ROAD_NS_CLASS,
                s2,
                j2,
                east(ROAD_EAST_SOUTH_NORTH_M, ROAD_EW_NORTH_M),
            ),
            (
                ROAD_NS_CLASS,
                j2,
                n2,
                east(ROAD_EW_NORTH_M, ROAD_EAST_NORTH_NORTH_M),
            ),
        ];
        let mut roads: Vec<Edge> = Vec::new();
        for (id, (class, a, b, points)) in parts.into_iter().enumerate() {
            roads.push(Edge {
                id: id as u32,
                class,
                a,
                b,
                points,
            });
        }
        let rail = vec![Edge {
            id: roads.len() as u32,
            class: RAIL_CLASS,
            a: rw,
            b: re,
            points: self.polyline(rail_north, ROAD_EW_WEST_EAST_M, ROAD_EW_EAST_EAST_M, false),
        }];
        (roads, rail)
    }

    pub fn roads(&self) -> Vec<Edge> {
        self.edges().0
    }

    pub fn railroads(&self) -> Vec<Edge> {
        self.edges().1
    }

    /// The road and rail graph, as the road file's edges would build it.
    pub fn road_graph(&self) -> roads::Graph {
        let classes = [ROAD_EW_CLASS, ROAD_NS_CLASS, RAIL_CLASS];
        let stretch = |kind: u8, e: Edge| roads::Stretch {
            kind,
            class: classes.iter().position(|c| *c == e.class).unwrap() as u8,
            a: Some(e.a),
            b: Some(e.b),
            points: e.points.iter().map(|p| [p[0], p[1]]).collect(),
        };
        let (road, rail) = self.edges();
        let stretches = road
            .into_iter()
            .map(|e| stretch(KIND_ROAD, e))
            .chain(rail.into_iter().map(|e| stretch(KIND_RAIL, e)))
            .collect();
        let mut water = |x: f64, z: f64| {
            let (north, east) = (x - f64::from(ORIGIN_X_M), z - f64::from(ORIGIN_Z_M));
            // The water under a road strip, which the bridge's covers.
            self.in_sea(north, east) || self.in_lake(north, east) || self.in_river(east)
        };
        let classes = classes.iter().map(|c| c.to_string()).collect();
        roads::build(classes, stretches, &mut water).expect("the water test answers every sample")
    }

    /// Every scenery object: the town, the industrial pair, the poles and
    /// the box-less pair, ids counting up in that order.
    pub fn scenery(&self) -> Vec<Object> {
        let mut placed: Vec<(&'static str, f64, f64)> = Vec::new();
        let mut state = u64::from(self.seed);
        let mut unit = || {
            state = state * u64::from(PRNG_MULTIPLIER) % u64::from(PRNG_MODULUS);
            state as f64 / f64::from(PRNG_MODULUS)
        };
        for _ in 0..SCENERY_TOWN_COUNT {
            let n = SCENERY_TOWN_NORTH_M + (2.0 * unit() - 1.0) * SCENERY_TOWN_SPREAD_M;
            let e = SCENERY_TOWN_EAST_M + (2.0 * unit() - 1.0) * SCENERY_TOWN_SPREAD_M;
            placed.push((SCENERY_TOWN_MODEL, n, e));
        }
        let (n, e) = (SCENERY_INDUSTRIAL_NORTH_M, SCENERY_INDUSTRIAL_EAST_M);
        placed.push((SCENERY_INDUSTRIAL_MODEL_A, n, e));
        placed.push((
            SCENERY_INDUSTRIAL_MODEL_B,
            n,
            e + SCENERY_INDUSTRIAL_SPACING_M,
        ));
        for k in 0..SCENERY_POLE_COUNT {
            let e = SCENERY_POLE_FIRST_EAST_M + f64::from(k) * SCENERY_POLE_SPACING_M;
            placed.push((
                SCENERY_POLE_MODEL,
                ROAD_EW_NORTH_M + SCENERY_POLE_NORTH_OFFSET_M,
                e,
            ));
        }
        for k in 0..SCENERY_NOBOX_COUNT {
            let e = SCENERY_NOBOX_EAST_M + f64::from(k) * SCENERY_NOBOX_SPACING_M;
            placed.push((SCENERY_NOBOX_MODEL, SCENERY_NOBOX_NORTH_M, e));
        }
        placed
            .into_iter()
            .enumerate()
            .map(|(i, (model, n, e))| {
                let (x, z) = self.abs(n, e);
                Object {
                    id: SCENERY_FIRST_ID + i as u64,
                    model,
                    x,
                    alt: self.ground(n, e),
                    z,
                    rotation: 0.0,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t() -> Theatre {
        Theatre::new(DEFAULT_SEED, DEFAULT_SIZE_KM, CELL_SIZE_M).unwrap()
    }

    /// The cell whose centre is nearest an offset.
    fn at(north: f64, east: f64) -> Cell {
        let c = f64::from(CELL_SIZE_M);
        t().cell((north / c) as u32, (east / c) as u32)
    }

    #[test]
    fn refuses_what_it_cannot_hold() {
        assert!(Theatre::new(1, 70, 100).is_err());
        assert!(Theatre::new(0, 70, 50).is_err());
        assert!(Theatre::new(1, 69, 50).is_err());
        assert!(Theatre::new(1, 100, 50).is_ok());
    }

    #[test]
    fn the_margin_is_outside_the_mesh() {
        assert_eq!(at(1000.0, 30000.0), OUTSIDE);
        assert_eq!(at(30000.0, 69_990.0), OUTSIDE);
        assert_ne!(at(2025.0, 60000.0), OUTSIDE);
    }

    #[test]
    fn each_water_class_is_where_it_should_be() {
        assert_eq!(at(5000.0, 5000.0).water, WATER_SEA);
        assert_eq!(at(5000.0, 5000.0).height, 0);
        assert_eq!(at(LAKE_NORTH_M, LAKE_EAST_M).water, WATER_LAKE);
        assert_eq!(at(LAKE_NORTH_M, LAKE_EAST_M).height, 100);
        assert_eq!(at(30000.0, RIVER_EAST_M + 10.0).water, WATER_RIVER);
        assert_eq!(at(30000.0, 60000.0).water, WATER_LAND);
    }

    #[test]
    fn the_plane_rises_one_percent_north() {
        // Far from both hills, the plane alone: -30 + 0.01 × 20025.
        assert_eq!(at(20025.0, 65000.0).height, 170);
    }

    #[test]
    fn roads_and_the_runway_take_one_row_of_cells() {
        assert_eq!(at(ROAD_EW_NORTH_M, 20000.0).surface, SURFACE_ROAD);
        assert_eq!(at(ROAD_EW_NORTH_M + 50.0, 20000.0).surface, SURFACE_LAND);
        assert_eq!(at(ROAD_EW_NORTH_M - 50.0, 20000.0).surface, SURFACE_LAND);
        assert_eq!(at(50000.0, ROAD_NS_EAST_M).surface, SURFACE_ROAD);
        assert_eq!(at(RUNWAY_NORTH_M, RUNWAY_EAST_M).surface, SURFACE_RUNWAY);
        assert_eq!(
            at(RUNWAY_NORTH_M + 50.0, RUNWAY_EAST_M).surface,
            SURFACE_LAND
        );
        let runway: u32 = (0..t().cells())
            .filter(|&c| t().cell((RUNWAY_NORTH_M / 50.0) as u32, c).surface == SURFACE_RUNWAY)
            .count() as u32;
        assert_eq!(runway, (RUNWAY_LENGTH_M / 50.0) as u32);
    }

    #[test]
    fn the_bridge_is_one_edge_between_the_junctions_and_carries_the_most() {
        let g = t().road_graph();
        let road: Vec<usize> = (0..g.edges.len())
            .filter(|&i| g.edges[i].kind == KIND_ROAD)
            .collect();
        // Contracted, the four arms of each junction and the link between.
        assert_eq!(road.len(), 7);
        let wet: Vec<usize> = road
            .iter()
            .copied()
            .filter(|&i| g.edges[i].crosses_water)
            .collect();
        assert_eq!(wet.len(), 1);
        let bridge = &g.edges[wet[0]];
        let mut ends = [bridge.a, bridge.b].map(|n| g.nodes[n as usize]);
        ends.sort_by(|p, q| p[1].total_cmp(&q[1]));
        let junctions = [ROAD_NS_EAST_M, ROAD_EAST_EAST_M].map(|east| {
            let (x, z) = t().abs(ROAD_EW_NORTH_M, east);
            [x, z]
        });
        assert_eq!(ends, junctions);
        assert_eq!(
            bridge.length,
            ROAD_EAST_EAST_M - ROAD_NS_EAST_M,
            "the bridge's whole length and the road either side"
        );

        let b = roads::betweenness(&g, roads::SOURCES, 2);
        assert_eq!(b[wet[0]], 1.0);
        for &i in &road {
            if i != wet[0] {
                assert_eq!(b[i], 7.0 / 16.0, "edge {i}");
            }
        }
    }

    #[test]
    fn the_bridge_is_road_over_the_river() {
        let c = at(ROAD_EW_NORTH_M, RIVER_EAST_M + 10.0);
        assert_eq!((c.surface, c.water), (SURFACE_ROAD, WATER_LAND));
        assert_eq!(
            at(ROAD_EW_NORTH_M + 50.0, RIVER_EAST_M + 10.0).water,
            WATER_RIVER
        );
    }

    #[test]
    fn the_forest_has_trees_and_its_hole_has_none() {
        let trees = |n: f64, e: f64| {
            let (n, e) = t().centre((n / 50.0) as u32, (e / 50.0) as u32);
            t().trees(n, e)
        };
        assert_eq!(trees(40025.0, 55025.0), 3);
        assert_eq!(trees(42000.0, 57000.0), 0);
        assert_eq!(trees(39975.0, 55025.0), 0);
        let sum: u32 = (0..t().cells())
            .flat_map(|r| (0..t().cells()).map(move |c| t().centre(r, c)))
            .map(|(n, e)| u32::from(t().trees(n, e)))
            .sum();
        assert_eq!(sum, t().tree_count());
    }

    #[test]
    fn every_pole_is_counted_once() {
        let col = (LINE_EAST_M / 50.0) as u32;
        let lines = |r: u32| {
            let (n, e) = t().centre(r, col);
            t().lines(n, e)
        };
        let sum: u32 = (0..t().cells()).map(|r| u32::from(lines(r))).sum();
        assert_eq!(sum, t().pole_count());
        assert_eq!(t().pole_count(), 201);
        assert_eq!(lines(400), 1);
        assert_eq!(lines(401), 0);
    }

    #[test]
    fn edges_that_share_a_node_meet_there() {
        let mut ends = std::collections::BTreeMap::<u32, Vec<[f64; 2]>>::new();
        for e in t().roads().iter().chain(t().railroads().iter()) {
            let (first, last) = (e.points[0], e.points[e.points.len() - 1]);
            ends.entry(e.a).or_default().push([first[0], first[1]]);
            ends.entry(e.b).or_default().push([last[0], last[1]]);
        }
        for (node, points) in &ends {
            assert!(
                points.iter().all(|p| p == &points[0]),
                "node {node}: {points:?}"
            );
        }
        // Each junction joins four edges, every other node one or two.
        assert_eq!(ends[&(ROAD_FIRST_NODE_ID + 1)].len(), 4);
        assert_eq!(ends[&(ROAD_FIRST_NODE_ID + 9)].len(), 4);
        let ids: Vec<u32> = t()
            .roads()
            .iter()
            .chain(t().railroads().iter())
            .map(|e| e.id)
            .collect();
        assert_eq!(ids, (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn the_scenery_is_what_the_constants_say() {
        let objects = t().scenery();
        assert_eq!(objects.len() as u32, SCENERY_COUNT);
        let first = objects.first().unwrap().id;
        assert_eq!(first, SCENERY_FIRST_ID);
        assert_eq!(objects, t().scenery(), "the same seed places the same town");
        let other = Theatre::new(2, DEFAULT_SIZE_KM, CELL_SIZE_M)
            .unwrap()
            .scenery();
        assert_ne!(objects[0], other[0]);
    }
}
