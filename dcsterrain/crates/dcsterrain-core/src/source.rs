//! Where the engine reads a terrain from.
//!
//! Every operation is written once against `Source`. An install's decoders
//! implement it, and so does the synthetic terrain held in memory, which is
//! what almost every test is stated against.

use std::fmt;

use crate::grid::Grid;

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

pub trait Source {
    fn identity(&self) -> &Identity;

    /// The cells the tiles cover.
    fn grid(&self) -> Grid;

    /// One tile of a layer, `tile_size` squared samples, row-major with
    /// columns fastest. A cell past the grid, or one no mesh triangle covers,
    /// is the layer's `nodata`.
    fn tile(&self, layer: Layer, tx: u32, tz: u32) -> Result<Vec<i32>, Refusal>;

    /// The ground at DCS `(x, z)` itself, not at a cell centre: the winning
    /// surface's own height, which is what the game reports. `None` where
    /// nothing covers the point.
    fn point(&self, x: f64, z: f64) -> Result<Option<Ground>, Refusal>;
}

/// The ground at one point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ground {
    /// Metres: the water surface over water, the ground elsewhere.
    pub height: f64,
    /// As the `water` layer codes it.
    pub water: u8,
    /// As the `surface` layer codes it.
    pub surface: u8,
    pub cover: Cover,
}

/// Cells along each side of a tile.
pub const TILE_SIZE: u32 = 64;

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
