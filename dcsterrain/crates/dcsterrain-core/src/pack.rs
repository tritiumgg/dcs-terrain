//! Builds a packed file from an extract: one SQLite file holding `meta`, the
//! grids and the tables a query reads.

pub mod meta;

/// The packed file's own format, independent of the extract's.
pub const FORMAT_VERSION: u32 = 1;
/// The base cell sizes a pack may ask for (ADR 0036).
pub const CELL_SIZES: [u32; 2] = [50, 100];
pub const DEFAULT_CELL_SIZE: u32 = 50;
/// Every grid is re-tiled to this; 64 keeps a 10 km sightline to a handful of
/// 8 KiB blobs.
pub const TILE_SIZE: u32 = 64;
