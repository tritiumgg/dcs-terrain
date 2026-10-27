//! The file decoders, the grid, the extract reader kept as a test oracle,
//! and the types of the terrain operations.
//!
//! The modules this crate exposes arrive with the tasks that fill them.

pub mod crs;
pub mod decode;
pub mod extract;
pub mod grid;
// Tests only, for now: the in-memory synthetic terrain that queries will
// answer from does not exist yet, and its extract writer builds test fixtures.
#[cfg(test)]
pub mod synth;
pub mod types;

#[cfg(test)]
mod scratch;
