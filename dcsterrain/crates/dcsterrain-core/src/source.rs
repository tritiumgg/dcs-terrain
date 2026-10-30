//! Where the engine reads a terrain from.
//!
//! Every operation is written once against `Source`. An install's decoders
//! implement it, and so does the synthetic terrain held in memory, which is
//! what almost every test is stated against.

use std::fmt;

use crate::classes::{Class, Family};
use crate::decode::scn5::Pool;
pub use crate::decode::surface5::ground::Piece;
use crate::grid::Grid;
use crate::places::Places;
use crate::roads::Graph;

pub mod install;

pub use install::Tables;

/// What `Source::tiles` hands each tile to: its index and its cells.
pub type EachTile<'a> = dyn FnMut(usize, &[i32]) -> Result<(), Refusal> + 'a;

/// Why a terrain cannot answer. The message names the file or the setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(pub String);

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refusal {}

/// What a terrain is, as every response reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// The id `entry.lua` declares, or `synth`.
    pub terrain: String,
    pub dcs_build: String,
    /// The sizes of the surface, road and scene files joined by `-`: a
    /// record of what answered, which needs no hash.
    pub fingerprint: String,
}

pub trait Source: Sync {
    fn identity(&self) -> &Identity;

    /// The cells the tiles cover.
    fn grid(&self) -> Grid;

    /// One tile of a layer, `tile_size` squared samples, row-major with
    /// columns fastest. A cell past the grid, or one no mesh triangle covers,
    /// is the layer's `nodata`.
    fn tile(&self, layer: Layer, tx: u32, tz: u32) -> Result<Vec<i32>, Refusal>;

    /// `tile` of each of `tiles`, `(tx, tz)`, calling `f` with its index in
    /// `tiles` and its cells, in order. An install rasterizes the tiles it
    /// does not hold together, on every core, a band of them at a time.
    fn tiles(&self, layer: Layer, tiles: &[(u32, u32)], f: &mut EachTile) -> Result<(), Refusal> {
        for (i, &(tx, tz)) in tiles.iter().enumerate() {
            f(i, &self.tile(layer, tx, tz)?)?;
        }
        Ok(())
    }

    /// The ground at DCS `(x, z)` itself, not at a cell center: the winning
    /// surface's own height, which is what the game reports. `None` where
    /// nothing covers the point.
    fn point(&self, x: f64, z: f64) -> Result<Option<Ground>, Refusal>;

    /// `point` at each of `points`, `[x, z]`, in order. An install answers
    /// them block by block, each block read once however many points lie on
    /// it, so the points need not be near one another.
    fn points(&self, points: &[[f64; 2]]) -> Result<Vec<Option<Ground>>, Refusal> {
        points.iter().map(|p| self.point(p[0], p[1])).collect()
    }

    /// Every mesh triangle under the segment from `a` to `b`, each `[x, z]`,
    /// clipped to the part of the segment it covers, in no order. Where
    /// pieces overlap, the one of higher rank is the ground, as at a point.
    fn along(&self, a: [f64; 2], b: [f64; 2]) -> Result<Vec<Piece>, Refusal> {
        let _ = (a, b);
        Err(lacks(self, "mesh triangles"))
    }

    /// `along` for each of `segments`, `[a, b]`, in order. An install
    /// answers them block by block, each block read once however many
    /// segments cross it.
    fn alongs(&self, segments: &[[[f64; 2]; 2]]) -> Result<Vec<Vec<Piece>>, Refusal> {
        segments.iter().map(|&[a, b]| self.along(a, b)).collect()
    }

    /// Every triangle of the runway surface whose box meets `b`, min x,
    /// min z, max x, max z in DCS meters, as its corners' `[x, z]`, in no
    /// order; the same triangle may come twice.
    fn runway_triangles(&self, b: [f64; 4]) -> Result<Vec<[[f64; 2]; 3]>, Refusal> {
        let _ = b;
        Err(lacks(self, "runway triangles"))
    }

    /// `runway_triangles` for each of `boxes`, in order. An install answers
    /// them block by block, each block read once however many boxes lie on
    /// it.
    fn runway_triangles_each(
        &self,
        boxes: &[[f64; 4]],
    ) -> Result<Vec<Vec<[[f64; 2]; 3]>>, Refusal> {
        boxes.iter().map(|&b| self.runway_triangles(b)).collect()
    }

    /// The airdromes, runways, stands, beacons, radio, towns, nodes and the
    /// projection.
    fn tables(&self) -> Result<&Tables, Refusal> {
        Err(lacks(self, "tables"))
    }

    /// Calls `f` with every instance of `pool` within `b`, min x, min z,
    /// max x, max z in DCS meters, as an object with its model and box:
    /// for the objects pool every scenery object, `effect` objects too.
    /// It may also call `f` with a few just outside `b`.
    fn objects_in(
        &self,
        pool: Pool,
        b: [f64; 4],
        f: &mut dyn FnMut(&Object),
    ) -> Result<(), Refusal> {
        let _ = (pool, b, f);
        Err(lacks(self, "scenery"))
    }

    /// The horizon tiles `tiles`, in order, as `horizon::compute` makes
    /// them. An install keeps each in its cache once computed.
    fn horizon(&self, tiles: &[(u32, u32)]) -> Result<Vec<Vec<u8>>, Refusal> {
        crate::horizon::compute(self, tiles)
    }

    /// The road and rail graph.
    fn road_graph(&self) -> Result<&Graph, Refusal> {
        Err(lacks(self, "roads"))
    }

    /// Each road and rail edge's betweenness, 0 to 1 of its network's most,
    /// in the graph's edge order; `None` while it is still being built,
    /// which on the largest terrains takes minutes.
    fn road_betweenness(&self) -> Result<Option<&[f32]>, Refusal> {
        Err(lacks(self, "road betweenness"))
    }

    /// Every place, in id order; `None` while the places are still being
    /// built, which walks every object of the terrain.
    fn places(&self) -> Result<Option<&Places>, Refusal> {
        Err(lacks(self, "places"))
    }

    /// Calls `f` with the `x`, `z` and class of every scenery object that
    /// stands on the ground within `b`, min x, min z, max x, max z in DCS
    /// meters. It may also call `f` with objects just outside `b`.
    fn structures_in(
        &self,
        b: [f64; 4],
        f: &mut dyn FnMut(f64, f64, Class),
    ) -> Result<(), Refusal> {
        let _ = (b, f);
        Err(lacks(self, "scenery"))
    }

    /// Calls `f` with the `x` and `z` of every instance of the trees or
    /// lines pool within `b`, and perhaps a few just outside it.
    fn instances_in(
        &self,
        pool: Pool,
        b: [f64; 4],
        f: &mut dyn FnMut(f64, f64),
    ) -> Result<(), Refusal> {
        let _ = (pool, b, f);
        Err(lacks(self, "scenery"))
    }
}

/// Why a terrain without `what` cannot answer.
fn lacks<S: Source + ?Sized>(s: &S, what: &str) -> Refusal {
    Refusal(format!("{} has no {what}", s.identity().terrain))
}

/// One scenery object as placed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Object<'a> {
    pub id: u64,
    /// As the scene file spells it, lowercase.
    pub model: &'a str,
    pub class: Class,
    /// Its settlement family; none for a model that is not a building.
    pub family: Option<Family>,
    /// World x, altitude, z.
    pub pos: [f64; 3],
    /// Of the model's local x axis, radians from north towards east.
    pub rotation: f64,
    /// The model's box, min x, y, z then max x, y, z, model-local.
    pub bbox: [f32; 6],
}

/// The ground at one point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ground {
    /// Meters: the water surface over water, the ground elsewhere.
    pub height: f64,
    /// Meters of water over the bed: zero on land, and wherever the mesh
    /// carries no depth.
    pub depth: f64,
    /// As the `water` layer codes it.
    pub water: u8,
    /// As the `surface` layer codes it.
    pub surface: u8,
    pub cover: Cover,
}

/// Cells along each side of a tile.
pub const TILE_SIZE: u32 = 64;
/// Meters along each side of a cell.
pub const CELL_SIZE: u32 = 50;

/// A layer of the cell grid, each cell sampled at its centre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    /// Metres, `floor(h + 0.5)` of the winning surface.
    Height,
    /// 0 land, 1 lake, 2 sea, 3 river.
    Water,
    /// 1 land, 3 water, 4 road, 5 runway.
    Surface,
    /// A `Cover` code.
    Cover,
}

impl Layer {
    pub const ALL: [Layer; 4] = [Layer::Height, Layer::Water, Layer::Surface, Layer::Cover];

    pub fn name(self) -> &'static str {
        match self {
            Layer::Height => "height",
            Layer::Water => "water",
            Layer::Surface => "surface",
            Layer::Cover => "cover",
        }
    }

    pub fn named(name: &str) -> Option<Layer> {
        Layer::ALL.into_iter().find(|l| l.name() == name)
    }

    /// What a cell no mesh triangle covers holds.
    pub fn nodata(self) -> i32 {
        match self {
            Layer::Height => -32768,
            Layer::Water => 255,
            Layer::Surface | Layer::Cover => 0,
        }
    }
}

/// Ground cover, as the `cover` layer codes it. Where several apply the
/// first in this order wins: water, runway, town, forest, field, beach, and
/// plain where none does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cover {
    Plain = 1,
    Field = 2,
    Forest = 3,
    Town = 4,
    Beach = 5,
    Runway = 6,
    Water = 7,
}

impl Cover {
    pub const ALL: [Cover; 7] = [
        Cover::Plain,
        Cover::Field,
        Cover::Forest,
        Cover::Town,
        Cover::Beach,
        Cover::Runway,
        Cover::Water,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Cover::Plain => "plain",
            Cover::Field => "field",
            Cover::Forest => "forest",
            Cover::Town => "town",
            Cover::Beach => "beach",
            Cover::Runway => "runway",
            Cover::Water => "water",
        }
    }

    pub fn code(self) -> i32 {
        self as i32
    }
}
