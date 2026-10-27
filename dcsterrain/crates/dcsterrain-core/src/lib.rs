//! The file decoders, the grid and the terrains it reads through, the
//! extract reader kept as a test oracle, and the types of the terrain
//! operations.
//!
//! The modules this crate exposes arrive with the tasks that fill them.

pub mod crs;
pub mod decode;
pub mod extract;
pub mod grid;
pub mod source;
pub mod synth;
pub mod types;

#[cfg(test)]
mod scratch;
