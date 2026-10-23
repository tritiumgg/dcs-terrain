//! Reads a map's files from a DCS install. The install is only ever read.
//!
//! A decoder is chosen by the class and format version a file's header
//! states, never by the DCS build or by probing the data (ADR 0035). A pair no
//! decoder declares stops the run: a decoder written for one version can read
//! another without error and return the wrong values.

use std::fmt;

pub mod container;

use container::Header;

/// The decoders, each reading one class at the versions it declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decoder {
    Surface5,
    RoadNetwork,
    Rn5,
    Scene5,
}

impl Decoder {
    pub const ALL: [Decoder; 4] = [
        Decoder::Surface5,
        Decoder::RoadNetwork,
        Decoder::Rn5,
        Decoder::Scene5,
    ];

    pub fn class(self) -> &'static str {
        match self {
            Decoder::Surface5 => "landscape5::Surface5File",
            Decoder::RoadNetwork => "landscape4::lRoadNetwork",
            Decoder::Rn5 => "t5::rn5file",
            Decoder::Scene5 => "landscape5::Scene5File",
        }
    }

    pub fn versions(self) -> &'static [u32] {
        match self {
            Decoder::Surface5 => &[5],
            Decoder::RoadNetwork => &[5],
            Decoder::Rn5 => &[0],
            // Nevada's scene file is version 8 and every other map's 9; the
            // two differ in layout, not just in number.
            Decoder::Scene5 => &[8, 9],
        }
    }

    /// Whether the header's u64 at 0x08 is the base every data offset is
    /// relative to, which the manifest records as `payload_size`. The other
    /// classes leave it unused.
    pub fn has_data_base(self) -> bool {
        matches!(self, Decoder::Surface5 | Decoder::Scene5)
    }
}

/// A file whose class and version no decoder declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unknown {
    pub path: String,
    pub class: String,
    pub version: u32,
}

impl fmt::Display for Unknown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (path, class, version) = (&self.path, &self.class, self.version);
        match Decoder::ALL.iter().find(|d| d.class() == class) {
            Some(d) => {
                let read: Vec<String> = d.versions().iter().map(u32::to_string).collect();
                write!(
                    f,
                    "{path}: {class} version {version} has no decoder; this build reads version {}",
                    read.join(", ")
                )
            }
            None => write!(f, "{path}: no decoder reads {class} (version {version})"),
        }
    }
}

/// The decoder for the file at `path`, whose header is `h`.
pub fn dispatch(path: &str, h: &Header) -> Result<Decoder, Unknown> {
    Decoder::ALL
        .into_iter()
        .find(|d| d.class() == h.class && d.versions().contains(&h.version))
        .ok_or_else(|| Unknown {
            path: path.to_string(),
            class: h.class.clone(),
            version: h.version,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(class: &str, version: u32) -> Header {
        container::parse(&container::build(class, version, 0)).unwrap()
    }

    #[test]
    fn every_declared_pair_dispatches_to_its_decoder() {
        for d in Decoder::ALL {
            for &v in d.versions() {
                assert_eq!(dispatch("f", &header(d.class(), v)), Ok(d));
            }
        }
        let nevada = header("landscape5::Scene5File", 8);
        assert_eq!(dispatch("Nevada.scn5", &nevada), Ok(Decoder::Scene5));
    }

    #[test]
    fn refuses_a_version_no_decoder_declares() {
        let e = dispatch("Scenes/X.scn5", &header("landscape5::Scene5File", 10)).unwrap_err();
        assert_eq!(e.version, 10);
        let msg = e.to_string();
        assert!(
            msg.starts_with("Scenes/X.scn5: landscape5::Scene5File version 10"),
            "{msg}"
        );
        assert!(msg.ends_with("reads version 8, 9"), "{msg}");
    }

    #[test]
    fn refuses_a_class_no_decoder_reads() {
        let e = dispatch("roads/X.routes", &header("landscape4::lRoutesFile", 3)).unwrap_err();
        assert_eq!(
            e.to_string(),
            "roads/X.routes: no decoder reads landscape4::lRoutesFile (version 3)"
        );
    }
}
