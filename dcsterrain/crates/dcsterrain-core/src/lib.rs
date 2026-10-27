//! The file decoders, the grid, the extract reader kept as a test oracle,
//! and the types of the terrain operations.
//!
//! The modules this crate exposes arrive with the tasks that fill them.

pub mod crs;
pub mod decode;
pub mod extract;
pub mod grid;
// Constants only for now; the generator lands beside them. Tests always see
// the module, and a consumer that wants `dcsterrain synth` in a release build
// asks for the feature.
#[cfg(any(test, feature = "synth"))]
pub mod synth;
pub mod types;

#[cfg(test)]
mod scratch;
