//! Extract reader, packer, packed-file reader and terrain operations.
//!
//! The modules this crate exposes arrive with the tasks that fill them.

// Constants only for now; the generator lands beside them. Tests always see
// the module, and a consumer that wants `dcsterrain synth` in a release build
// asks for the feature.
#[cfg(any(test, feature = "synth"))]
pub mod synth;
