//! Members of a response held compactly until the answer is written, and
//! encoded as it is, so a large answer is never a second time in memory as
//! text or as a tree of values.

use std::borrow::Cow;
use std::fmt;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A raster of `f32`, little-endian and base64 encoded on the wire: the
/// numbers until written, since at the limit the text is 21 MiB, or the
/// text when read back.
#[derive(Debug, Clone, PartialEq)]
pub enum Floats {
    Text(String),
    Values(Vec<f32>),
}

/// Floats written a few at a time.
struct Runs<'a>(&'a [f32]);

impl fmt::Display for Runs<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Whole groups of three bytes in each run, so runs join seamlessly.
        for run in self.0.chunks(3 << 10) {
            f.write_str(&crate::query::floats32(run))?;
        }
        Ok(())
    }
}

impl Serialize for Floats {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Floats::Text(t) => s.serialize_str(t),
            Floats::Values(v) => s.collect_str(&Runs(v)),
        }
    }
}

impl<'de> Deserialize<'de> for Floats {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Floats, D::Error> {
        String::deserialize(d).map(Floats::Text)
    }
}

/// On the wire it is a string, and its schema says so.
impl JsonSchema for Floats {
    fn inline_schema() -> bool {
        String::inline_schema()
    }

    fn schema_name() -> Cow<'static, str> {
        String::schema_name()
    }

    fn schema_id() -> Cow<'static, str> {
        String::schema_id()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        String::json_schema(generator)
    }
}
