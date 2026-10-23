//! What an install says about itself: the DCS build, a map's theatre id, and
//! the map's files with the class and version each one states.

use std::fmt;
use std::fs;
use std::path::Path;

use serde_json::Value;

use super::container;
use super::{Decoder, dispatch};
use crate::extract::FileEntry;

pub const BUILD_FILE: &str = "autoupdate.cfg";
pub const ENTRY_FILE: &str = "entry.lua";

/// Why the install cannot be read for this run. The message names the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal(pub String);

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<super::Unknown> for Refusal {
    fn from(u: super::Unknown) -> Self {
        Refusal(u.to_string())
    }
}

/// The DCS core build, as `autoupdate.cfg` at the install root states it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    pub version: String,
    pub timestamp: String,
}

/// `version` and `timestamp` from `autoupdate.cfg`, which is strict JSON.
pub fn parse_build(text: &str) -> Result<Build, Refusal> {
    let doc: Value = serde_json::from_str(text)
        .map_err(|e| Refusal(format!("{BUILD_FILE} is not JSON: {e}")))?;
    let field = |key: &str| match doc.get(key).and_then(Value::as_str) {
        Some(s) if !s.is_empty() => Ok(s.to_string()),
        _ => Err(Refusal(format!("{BUILD_FILE} has no {key}"))),
    };
    Ok(Build {
        version: field("version")?,
        timestamp: field("timestamp")?,
    })
}

pub fn read_build(install: &Path) -> Result<Build, Refusal> {
    let text = fs::read_to_string(install.join(BUILD_FILE))
        .map_err(|e| Refusal(format!("{BUILD_FILE} cannot be read: {e}")))?;
    parse_build(&text)
}

/// The theatre id an `entry.lua` declares, read as text and never run.
///
/// ED writes `['id'] = "SinaiMap";` inside the theatre table, one brace deep.
/// Of every assignment to an `id` key, the shallowest wins, so a skin or node
/// table carrying its own `id` further in cannot be taken for the theatre's.
/// At equal depth a bracketed key beats a bare `id = "..."`, and `update_id`
/// is never a bare `id`. Braces inside strings and comments count toward the
/// depth, which is wrong for Lua in general and right for these files, whose
/// strings are names and paths.
///
/// Ties go as the hook's reader broke them, so the two agree on any file: by
/// form, in the order `['id'] = "…"`, `["id"] = "…"`, `['id'] = '…'`,
/// `["id"] = '…'`, `id = "…"`, `id = '…'`, then the first in the file.
pub fn entry_id(text: &str) -> Option<String> {
    let b = text.as_bytes();
    // (depth, form, id): the least wins, and the first of equals.
    let mut best: Option<(i64, u8, String)> = None;
    let mut depth = 0i64;
    let mut counted = 0;
    for i in 0..b.len() {
        if !b[i..].starts_with(b"id") {
            continue;
        }
        let Some((at, form, id)) = bracketed(b, i).or_else(|| bare(b, i)) else {
            continue;
        };
        if id.is_empty() {
            continue;
        }
        // Positions only grow, so the depth is carried forward.
        for &c in &b[counted..at] {
            match c {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
        }
        counted = at;
        if best.as_ref().is_none_or(|b| (depth, form) < (b.0, b.1)) {
            best = Some((depth, form, id));
        }
    }
    best.map(|(.., id)| id)
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// `= "value"` or `= 'value'` from `i`, after optional space: the value and
/// whether it was double-quoted.
fn assigned(b: &[u8], i: usize) -> Option<(String, bool)> {
    let i = skip_ws(b, i);
    if b.get(i) != Some(&b'=') {
        return None;
    }
    let i = skip_ws(b, i + 1);
    let q = *b.get(i)?;
    if q != b'"' && q != b'\'' {
        return None;
    }
    let len = b[i + 1..].iter().position(|&c| c == q)?;
    let value = String::from_utf8_lossy(&b[i + 1..i + 1 + len]).into_owned();
    Some((value, q == b'"'))
}

/// `['id'] = "..."` with `id` at `i`: the `[` position, the form's rank and
/// the value.
fn bracketed(b: &[u8], i: usize) -> Option<(usize, u8, String)> {
    let q = *b.get(i.checked_sub(1)?)?;
    if (q != b'"' && q != b'\'') || b.get(i + 2) != Some(&q) {
        return None;
    }
    let mut open = i - 1;
    while open > 0 && b[open - 1].is_ascii_whitespace() {
        open -= 1;
    }
    let open = open.checked_sub(1)?;
    if b[open] != b'[' {
        return None;
    }
    let close = skip_ws(b, i + 3);
    if b.get(close) != Some(&b']') {
        return None;
    }
    let (id, double) = assigned(b, close + 1)?;
    let form = match (q == b'"', double) {
        (false, true) => 0,
        (true, true) => 1,
        (false, false) => 2,
        (true, false) => 3,
    };
    Some((open, form, id))
}

/// `id = "..."` with `id` at `i` and no word character before it.
fn bare(b: &[u8], i: usize) -> Option<(usize, u8, String)> {
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    if i > 0 && word(b[i - 1]) {
        return None;
    }
    let (id, double) = assigned(b, i + 2)?;
    Some((i, if double { 4 } else { 5 }, id))
}

pub const TERRAINS_DIR: &str = "Mods/terrains";

/// A map as the install holds it: what it is, which build, and every file
/// the decoders will read, each classified from its own header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Map {
    /// The directory under `Mods/terrains`, spelled as the disk spells it.
    pub dir: String,
    pub theatre: String,
    pub build: Build,
    pub files: Vec<FileEntry>,
}

/// Where a role's files live under the map directory, and how many.
enum Find {
    /// Exactly one file with this extension in this directory (ADR 0022).
    One(&'static str, &'static str),
    /// Every file with this extension in this directory, perhaps none.
    All(&'static str, &'static str),
    /// This path, if it is there.
    Named(&'static [&'static str]),
}

/// Each role, where its files are, and the decoder its container must name.
/// A role without a decoder is plain: Lua source, or `assets.assets`, which
/// has no container header.
const ROLES: [(&str, Find, Option<Decoder>); 10] = [
    (
        "surface",
        Find::One("Surface", ".surface5"),
        Some(Decoder::Surface5),
    ),
    (
        "roads",
        Find::One("roads", ".rn4"),
        Some(Decoder::RoadNetwork),
    ),
    ("scene", Find::One("Scenes", ".scn5"), Some(Decoder::Scene5)),
    (
        "airfield_rn4",
        Find::All("AirfieldsTaxiways", ".rn4"),
        Some(Decoder::RoadNetwork),
    ),
    (
        "airfield_rn5",
        Find::All("AirfieldsTaxiways", ".rn5"),
        Some(Decoder::Rn5),
    ),
    ("assets", Find::Named(&["misc", "assets.assets"]), None),
    ("lua", Find::Named(&["Beacons.lua"]), None),
    ("lua", Find::Named(&["Radio.lua"]), None),
    ("lua", Find::Named(&["Map", "towns.lua"]), None),
    ("lua", Find::Named(&["MissionGenerator", "nodes.lua"]), None),
];

/// The entry of `parent` named `name` without regard to case, spelled as the
/// disk spells it: `Surface` on one map is `surface` on another.
fn child(parent: &Path, name: &str) -> Option<String> {
    fs::read_dir(parent)
        .ok()?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| n.eq_ignore_ascii_case(name))
}

/// Every file in `dir` whose name ends in `ext`, without regard to case,
/// sorted.
fn with_ext(dir: &Path, ext: &str) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.len() > ext.len() && n.to_ascii_lowercase().ends_with(ext))
        .collect();
    names.sort();
    names
}

/// Reads `map`, the directory name under `Mods/terrains`, from the install
/// at `install`. Opens only the files a decoder reads, and those only to
/// read a header: never an encrypted `.crypt` file.
pub fn read_map(install: &Path, map: &str) -> Result<Map, Refusal> {
    let build = read_build(install)?;
    let terrains = install.join(TERRAINS_DIR);
    let dir = child(&terrains, map)
        .ok_or_else(|| Refusal(format!("no map {map} under {TERRAINS_DIR}")))?;
    let root = terrains.join(&dir);
    let rel_root = format!("{TERRAINS_DIR}/{dir}");

    let entry = child(&root, ENTRY_FILE)
        .ok_or_else(|| Refusal(format!("{rel_root} has no {ENTRY_FILE}: not a map")))?;
    // Bytes, not UTF-8: a localized name in another encoding must not hide
    // the id beside it.
    let bytes = fs::read(root.join(&entry))
        .map_err(|e| Refusal(format!("{rel_root}/{entry} cannot be read: {e}")))?;
    let text = String::from_utf8_lossy(&bytes);
    let theatre =
        entry_id(&text).ok_or_else(|| Refusal(format!("{rel_root}/{entry} declares no id")))?;

    let mut files = Vec::new();
    for (role, find, decoder) in ROLES {
        for rel in locate(&root, &rel_root, &find)? {
            files.push(classify(install, role, &rel, decoder)?);
        }
    }
    Ok(Map {
        dir,
        theatre,
        build,
        files,
    })
}

/// The paths, relative to the install, that `find` names under the map.
fn locate(root: &Path, rel_root: &str, find: &Find) -> Result<Vec<String>, Refusal> {
    match *find {
        Find::One(sub, ext) | Find::All(sub, ext) => {
            let Some(sub) = child(root, sub) else {
                return match find {
                    Find::One(..) => Err(Refusal(format!("{rel_root} has no {sub} directory"))),
                    _ => Ok(Vec::new()),
                };
            };
            let names = with_ext(&root.join(&sub), ext);
            if matches!(find, Find::One(..)) && names.len() != 1 {
                let found = if names.is_empty() {
                    "none".to_string()
                } else {
                    names.join(", ")
                };
                return Err(Refusal(format!(
                    "{rel_root}/{sub} must hold one {ext} file, and holds {found}"
                )));
            }
            Ok(names
                .into_iter()
                .map(|n| format!("{rel_root}/{sub}/{n}"))
                .collect())
        }
        Find::Named(parts) => {
            let mut at = root.to_path_buf();
            let mut rel = rel_root.to_string();
            for part in parts {
                let Some(name) = child(&at, part) else {
                    return Ok(Vec::new());
                };
                at.push(&name);
                rel = format!("{rel}/{name}");
            }
            Ok(if at.is_file() { vec![rel] } else { Vec::new() })
        }
    }
}

/// The manifest's entry for one file: its size, and for a container its
/// class and version, refused unless they name the decoder its role needs.
fn classify(
    install: &Path,
    role: &str,
    rel: &str,
    decoder: Option<Decoder>,
) -> Result<FileEntry, Refusal> {
    let path = install.join(rel);
    let size = fs::metadata(&path)
        .map_err(|e| Refusal(format!("{rel} cannot be read: {e}")))?
        .len();
    let Some(want) = decoder else {
        return Ok(FileEntry {
            role: role.to_string(),
            path: rel.to_string(),
            class: None,
            version: None,
            size,
            payload_size: None,
        });
    };
    let header = container::read(&path)
        .map_err(|e| Refusal(format!("{rel} cannot be read: {e}")))?
        .map_err(|why| Refusal(format!("{rel} is not a map file: {why}")))?;
    let got = dispatch(rel, &header)?;
    if got != want {
        return Err(Refusal(format!(
            "{rel} is a {}, where a {role} file is a {}",
            header.class,
            want.class()
        )));
    }
    Ok(FileEntry {
        role: role.to_string(),
        path: rel.to_string(),
        class: Some(header.class),
        version: Some(header.version),
        size,
        payload_size: got.has_data_base().then_some(header.data_base),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of ED's entry.lua around the one line that matters.
    /// `update_id` comes first on purpose: a reader that took the first `id`
    /// it saw would take it.
    fn entry(id: &str) -> String {
        format!(
            "if not USE_TERRAIN4 then\n\treturn\nend\n\ntheatre =\n{{\n\
             \t['developerName'] = \"x\";\n\
             \t['update_id'] = \"{}_terrain\";\n\
             \t['id'] = \"{id}\";\n\
             \t['localizedName'] = \"x\";\n\
             }} -- end of theatre\n",
            id.to_uppercase()
        )
    }

    #[test]
    fn reads_the_id_entry_lua_declares() {
        assert_eq!(entry_id(&entry("SinaiMap")).as_deref(), Some("SinaiMap"));
        assert_eq!(entry_id(&entry("GermanyCW")).as_deref(), Some("GermanyCW"));
    }

    #[test]
    fn the_shallowest_id_wins() {
        let text = "theatre = {\n skins = { { id = \"deep\" } };\n ['id'] = \"top\";\n}\n";
        assert_eq!(entry_id(text).as_deref(), Some("top"));
    }

    #[test]
    fn a_bracketed_key_beats_a_bare_one_at_the_same_depth() {
        let text = "t = {\n id = 'bare';\n [\"id\"] = 'bracketed';\n}\n";
        assert_eq!(entry_id(text).as_deref(), Some("bracketed"));
    }

    #[test]
    fn ties_break_by_form_then_position_as_the_hook_breaks_them() {
        assert_eq!(
            entry_id("t = { [\"id\"] = \"A\"; ['id'] = \"B\" }").as_deref(),
            Some("B")
        );
        assert_eq!(
            entry_id("t = { id = 'x'; id = \"y\" }").as_deref(),
            Some("y")
        );
        assert_eq!(
            entry_id("t = { ['id'] = 'p'; ['id'] = 'q' }").as_deref(),
            Some("p")
        );
        assert_eq!(
            entry_id("t = { [ 'id' ] = \"spaced\" }").as_deref(),
            Some("spaced")
        );
    }

    #[test]
    fn an_entry_file_in_another_encoding_still_gives_its_id() {
        let m = "Mods/terrains/Synth";
        let (_i, r) = install();
        let mut bytes = b"theatre = {\n\t['localizedName'] = \"Caf\xe9\";\n".to_vec();
        bytes.extend_from_slice(b"\t['id'] = \"Synth\";\n}\n");
        put(&r, &format!("{m}/entry.lua"), &bytes);
        assert_eq!(read_map(&r, "Synth").unwrap().theatre, "Synth");
    }

    #[test]
    fn update_id_alone_is_no_id() {
        assert_eq!(
            entry_id("t = { update_id = \"X\"; ['update_id'] = 'Y' }"),
            None
        );
        assert_eq!(entry_id("t = { ['id'] = \"\" }"), None);
    }

    #[test]
    fn reads_the_build() {
        let text = "{\n \"WARNING\": \"DO NOT EDIT this file.\",\n \
                    \"version\": \"0.0.0.0\",\n \"timestamp\": \"00000000-000000\",\n \
                    \"arch\": \"x86_64\",\n \"modules\": [\n  \"WORLD\"\n ]\n}\n";
        let b = parse_build(text).unwrap();
        assert_eq!(
            (b.version.as_str(), b.timestamp.as_str()),
            ("0.0.0.0", "00000000-000000")
        );
    }

    #[test]
    fn refuses_a_build_file_without_its_fields() {
        let e = parse_build("{\"version\": \"\"}").unwrap_err();
        assert_eq!(e.0, "autoupdate.cfg has no version");
        let e = parse_build("{\"version\": \"1\"}").unwrap_err();
        assert_eq!(e.0, "autoupdate.cfg has no timestamp");
        assert!(parse_build("version=1").unwrap_err().0.contains("not JSON"));
    }

    use crate::scratch::Scratch;
    use std::path::PathBuf;

    const BUILD: &str = "{\"version\": \"0.0.0.0\", \"timestamp\": \"00000000-000000\"}\n";

    /// Writes `bytes` at `rel` under `root`, making its directories.
    fn put(root: &Path, rel: &str, bytes: &[u8]) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }

    fn head(class: &str, version: u32, base: u64, pad: usize) -> Vec<u8> {
        let mut b = container::build(class, version, base);
        b.resize(b.len() + pad, 0);
        b
    }

    /// An install holding one map, `Synth`, with its folders spelled as the
    /// installed maps vary them.
    fn install() -> (Scratch, PathBuf) {
        let s = Scratch::new();
        let r = s.0.clone();
        let m = "Mods/terrains/Synth";
        put(&r, "autoupdate.cfg", BUILD.as_bytes());
        put(&r, &format!("{m}/entry.lua"), entry("Synth").as_bytes());
        put(
            &r,
            &format!("{m}/surface/Synth.surface5"),
            &head("landscape5::Surface5File", 5, 4096, 100),
        );
        put(
            &r,
            &format!("{m}/roads/Synth.rn4"),
            &head("landscape4::lRoadNetwork", 5, 77, 10),
        );
        put(
            &r,
            &format!("{m}/roads/Synth.routes"),
            &head("landscape4::lRoutesFile", 3, 0, 0),
        );
        put(
            &r,
            &format!("{m}/Scenes/Synth.scn5"),
            &head("landscape5::Scene5File", 9, 512, 0),
        );
        put(
            &r,
            &format!("{m}/AirfieldsTaxiways/B.rn4"),
            &head("landscape4::lRoadNetwork", 5, 0, 0),
        );
        put(
            &r,
            &format!("{m}/AirfieldsTaxiways/A.rn4"),
            &head("landscape4::lRoadNetwork", 5, 0, 0),
        );
        put(
            &r,
            &format!("{m}/AirfieldsTaxiways/A.rn5"),
            &head("t5::rn5file", 0, 0, 0),
        );
        put(&r, &format!("{m}/misc/assets.assets"), b"stand");
        put(&r, &format!("{m}/Beacons.lua"), b"beacons = {}\n");
        put(&r, &format!("{m}/map/towns.lua"), b"towns = {}\n");
        put(
            &r,
            &format!("{m}/terrain.cfg.lua.pak.crypt"),
            b"\x02\x00\x00\x00",
        );
        (s, r)
    }

    #[test]
    fn lists_every_file_a_decoder_reads_with_its_header() {
        let (_s, r) = install();
        let map = read_map(&r, "synth").unwrap();
        assert_eq!((map.dir.as_str(), map.theatre.as_str()), ("Synth", "Synth"));
        assert_eq!(map.build.version, "0.0.0.0");
        let rows: Vec<(&str, &str)> = map
            .files
            .iter()
            .map(|f| (f.role.as_str(), f.path.as_str()))
            .collect();
        let m = "Mods/terrains/Synth";
        assert_eq!(
            rows,
            [
                ("surface", format!("{m}/surface/Synth.surface5").as_str()),
                ("roads", format!("{m}/roads/Synth.rn4").as_str()),
                ("scene", format!("{m}/Scenes/Synth.scn5").as_str()),
                (
                    "airfield_rn4",
                    format!("{m}/AirfieldsTaxiways/A.rn4").as_str()
                ),
                (
                    "airfield_rn4",
                    format!("{m}/AirfieldsTaxiways/B.rn4").as_str()
                ),
                (
                    "airfield_rn5",
                    format!("{m}/AirfieldsTaxiways/A.rn5").as_str()
                ),
                ("assets", format!("{m}/misc/assets.assets").as_str()),
                ("lua", format!("{m}/Beacons.lua").as_str()),
                ("lua", format!("{m}/map/towns.lua").as_str()),
            ]
        );
        let surface = &map.files[0];
        assert_eq!(surface.class.as_deref(), Some("landscape5::Surface5File"));
        assert_eq!(
            (surface.version, surface.payload_size),
            (Some(5), Some(4096))
        );
        // The road network's u64 at 0x08 is not a data base.
        assert_eq!(map.files[1].payload_size, None);
        assert_eq!(map.files[6].size, 5);
        assert_eq!(map.files[6].class, None);
    }

    #[test]
    fn refuses_a_version_no_decoder_reads_naming_file_class_and_version() {
        let (_s, r) = install();
        put(
            &r,
            "Mods/terrains/Synth/Scenes/Synth.scn5",
            &head("landscape5::Scene5File", 10, 0, 0),
        );
        let e = read_map(&r, "Synth").unwrap_err();
        assert!(
            e.0.starts_with(
                "Mods/terrains/Synth/Scenes/Synth.scn5: landscape5::Scene5File version 10"
            ),
            "{e}"
        );
    }

    #[test]
    fn refuses_a_role_holding_another_class() {
        let (_s, r) = install();
        put(
            &r,
            "Mods/terrains/Synth/roads/Synth.rn4",
            &head("t5::rn5file", 0, 0, 0),
        );
        let e = read_map(&r, "Synth").unwrap_err();
        assert!(e.0.contains("is a t5::rn5file, where a roads file"), "{e}");
    }

    #[test]
    fn refuses_two_files_where_a_role_takes_one() {
        let (_s, r) = install();
        put(
            &r,
            "Mods/terrains/Synth/surface/Old.surface5",
            &head("landscape5::Surface5File", 5, 0, 0),
        );
        let e = read_map(&r, "Synth").unwrap_err();
        assert!(e.0.contains("holds Old.surface5, Synth.surface5"), "{e}");
    }

    #[test]
    fn refuses_a_map_without_its_required_files() {
        let (_s, r) = install();
        fs::remove_file(r.join("Mods/terrains/Synth/roads/Synth.rn4")).unwrap();
        let e = read_map(&r, "Synth").unwrap_err();
        assert!(e.0.contains("one .rn4 file, and holds none"), "{e}");

        let (_s, r) = install();
        fs::remove_file(r.join("Mods/terrains/Synth/entry.lua")).unwrap();
        assert!(read_map(&r, "Synth").unwrap_err().0.contains("not a map"));

        let (_s, r) = install();
        assert!(read_map(&r, "Kola").unwrap_err().0.contains("no map Kola"));
    }

    #[test]
    fn refuses_a_file_that_is_not_a_container() {
        let (_s, r) = install();
        put(&r, "Mods/terrains/Synth/Scenes/Synth.scn5", b"-- lua\n");
        let e = read_map(&r, "Synth").unwrap_err();
        assert!(e.0.contains("Synth.scn5 is not a map file"), "{e}");
    }
}
