//! What an install says about itself: the DCS build, the terrains it holds
//! and the id each declares, and a terrain's files with the class and version
//! each one states.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::container;
use super::{Decoder, dispatch};
use crate::extract::FileEntry;

pub const BUILD_FILE: &str = "autoupdate.cfg";
pub const ENTRY_FILE: &str = "entry.lua";

pub use crate::source::Refusal;

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
/// Ties go by form, in the order `['id'] = "…"`, `["id"] = "…"`,
/// `['id'] = '…'`, `["id"] = '…'`, `id = "…"`, `id = '…'`, then the first in
/// the file.
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
/// the decoders will read. Only the surface file is classified from its
/// header when the map is read, since nothing answers without it; the
/// others are classified when something first reads their role, so a
/// height never waits on 450 taxiway headers (ADR 0001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Map {
    /// The directory under `Mods/terrains`, spelled as the disk spells it.
    pub dir: String,
    pub theatre: String,
    pub build: Build,
    /// Every file of every role, found by listing, none of them read.
    pub files: Vec<Located>,
    pub surface: FileEntry,
}

/// One of a map's files as a listing or a stat gives it, without opening
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    pub role: String,
    pub path: String,
    pub size: u64,
    /// Nanoseconds since the Unix epoch.
    pub modified: u128,
}

impl Map {
    /// The files of `roles` as the disk holds them now, read from metadata
    /// alone: a stat of each file of a role that takes one file or a named
    /// one, and one listing of the directory of a role that takes every
    /// file in it, so a file added there is seen.
    pub fn stat(&self, install: &Path, roles: &[&str]) -> Result<Vec<Located>, Refusal> {
        let root = install.join(TERRAINS_DIR).join(&self.dir);
        let rel_root = format!("{TERRAINS_DIR}/{}", self.dir);
        let mut ls = Listings::default();
        let mut out = Vec::new();
        for (i, (role, find, _)) in ROLES.iter().enumerate() {
            if !roles.contains(role) {
                continue;
            }
            if let Find::All(..) = find {
                out.extend(locate(&mut ls, &root, &rel_root, role, find)?);
                continue;
            }
            // A role named by several rows is stated once, at its first.
            if ROLES[..i].iter().any(|(r, ..)| r == role) {
                continue;
            }
            for f in self.files.iter().filter(|f| f.role == *role) {
                let meta = fs::metadata(install.join(&f.path))
                    .map_err(|e| Refusal(format!("{} cannot be read: {e}", f.path)))?;
                out.push(Located {
                    size: meta.len(),
                    modified: since_epoch(&meta).ok_or_else(|| no_time(&f.path))?,
                    ..f.clone()
                });
            }
        }
        Ok(out)
    }
}

fn since_epoch(meta: &fs::Metadata) -> Option<u128> {
    let t = meta.modified().ok()?;
    Some(t.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos())
}

fn no_time(rel: &str) -> Refusal {
    Refusal(format!("{rel} has no modification time"))
}

/// Where a role's files live under the map directory, and how many.
enum Find {
    /// Exactly one file with this extension in this directory, found by its
    /// extension because its base name follows neither the id nor the folder.
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

/// `rel`, a `/`-separated path under `root`, spelled as the disk spells it,
/// if a file is there. Each part is matched without regard to case.
pub fn resolve(root: &Path, rel: &str) -> Option<String> {
    let mut at = root.to_path_buf();
    let mut parts = Vec::new();
    for part in rel.split('/') {
        let name = child(&at, part)?;
        at.push(&name);
        parts.push(name);
    }
    at.is_file().then(|| parts.join("/"))
}

/// One entry of a directory as its listing gives it.
#[derive(Debug, Clone)]
struct Seen {
    name: String,
    file: bool,
    size: u64,
    modified: Option<u128>,
}

/// Directory listings, each read once: the map's directory is looked in
/// for every role, and `AirfieldsTaxiways` holds two.
#[derive(Default)]
struct Listings(HashMap<PathBuf, Vec<Seen>>);

impl Listings {
    fn of(&mut self, dir: &Path) -> &[Seen] {
        self.0.entry(dir.to_path_buf()).or_insert_with(|| list(dir))
    }

    /// The entry of `dir` named `name` without regard to case.
    fn child(&mut self, dir: &Path, name: &str) -> Option<Seen> {
        self.of(dir)
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
            .cloned()
    }
}

/// Every entry of `dir`, none when it cannot be read. A listing carries
/// each entry's type, size and time without opening it, on Windows from
/// the directory itself; only a link is followed, so a linked map file is
/// still the map's file.
fn list(dir: &Path) -> Vec<Seen> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| {
            let meta = match e.file_type() {
                Ok(t) if t.is_symlink() => fs::metadata(e.path()),
                _ => e.metadata(),
            };
            let name = e.file_name().to_string_lossy().into_owned();
            match meta {
                Ok(m) => Seen {
                    name,
                    file: m.is_file(),
                    size: m.len(),
                    modified: since_epoch(&m),
                },
                Err(_) => Seen {
                    name,
                    file: false,
                    size: 0,
                    modified: None,
                },
            }
        })
        .collect()
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
    read_files(install, build, dir, theatre)
}

/// The map in `dir`, spelled as the disk spells it, whose `entry.lua`
/// declares `theatre`: its files located and its surface classified.
fn read_files(install: &Path, build: Build, dir: String, theatre: String) -> Result<Map, Refusal> {
    let root = install.join(TERRAINS_DIR).join(&dir);
    let rel_root = format!("{TERRAINS_DIR}/{dir}");
    let mut ls = Listings::default();
    let mut files = Vec::new();
    for (role, find, _) in &ROLES {
        files.extend(locate(&mut ls, &root, &rel_root, role, find)?);
    }
    let surface = files
        .iter()
        .find(|f| f.role == "surface")
        .expect("the surface role finds its one file or refuses");
    let surface = classify_one(install, surface)?;
    Ok(Map {
        dir,
        theatre,
        build,
        files,
        surface,
    })
}

/// A directory under `Mods/terrains` holding an `entry.lua`, and the id that
/// declares, `None` when it declares none or cannot be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub dir: String,
    pub id: Option<String>,
}

/// Every terrain the install holds, sorted by folder. A directory without an
/// `entry.lua` is not a terrain and is passed over.
pub fn terrains(install: &Path) -> Result<Vec<Listed>, Refusal> {
    let root = install.join(TERRAINS_DIR);
    let entries = fs::read_dir(&root).map_err(|e| {
        Refusal(format!(
            "{} has no {TERRAINS_DIR} to read: {e}",
            install.display()
        ))
    })?;
    let mut out = Vec::new();
    for e in entries.flatten() {
        // The listing gives an entry's type without a stat; only a link is
        // followed to see what it names.
        let dir = match e.file_type() {
            Ok(t) if !t.is_symlink() => t.is_dir(),
            _ => e.path().is_dir(),
        };
        if dir {
            out.extend(listed(
                e.file_name().to_string_lossy().into_owned(),
                &e.path(),
            ));
        }
    }
    out.sort_by(|a, b| a.dir.cmp(&b.dir));
    Ok(out)
}

/// The terrain in the directory `path`, named `dir`, if it holds an
/// `entry.lua`. The file is opened by ED's spelling first, which every
/// install uses, and looked for in a listing only when that is not there:
/// on Windows each open or listing in the install costs tens of
/// microseconds, and this runs for every terrain on every call.
fn listed(dir: String, path: &Path) -> Option<Listed> {
    let bytes = match fs::read(path.join(ENTRY_FILE)) {
        Ok(b) => Some(b),
        Err(_) => fs::read(path.join(child(path, ENTRY_FILE)?)).ok(),
    };
    let id = bytes.and_then(|b| entry_id(&String::from_utf8_lossy(&b)));
    Some(Listed { dir, id })
}

/// The folder of the terrain whose `entry.lua` declares `id`, which is not
/// the folder's own name: Germany's folder is `GermanyColdWar` and its id
/// `GermanyCW`. An unknown id is refused with every id the install holds.
pub fn find(install: &Path, id: &str) -> Result<String, Refusal> {
    let all = terrains(install)?;
    let dirs: Vec<&str> = all
        .iter()
        .filter(|t| t.id.as_deref() == Some(id))
        .map(|t| t.dir.as_str())
        .collect();
    match dirs[..] {
        [dir] => Ok(dir.to_string()),
        [] => {
            let ids: Vec<&str> = all.iter().filter_map(|t| t.id.as_deref()).collect();
            let held = if ids.is_empty() {
                "none".to_string()
            } else {
                ids.join(", ")
            };
            Err(Refusal(format!(
                "no terrain {id} in this install; the ids it holds are {held}"
            )))
        }
        _ => Err(Refusal(format!(
            "{} each declare the id {id}; remove one",
            dirs.join(" and ")
        ))),
    }
}

/// Opens the terrain whose `entry.lua` declares `id`.
pub fn open(install: &Path, id: &str) -> Result<Map, Refusal> {
    let dir = find(install, id)?;
    // Finding it listed the terrains and read each `entry.lua`, so the
    // folder is spelled as the disk spells it and declares `id`: neither is
    // looked up again.
    let build = read_build(install)?;
    read_files(install, build, dir, id.to_string())
}

/// One of a terrain's files, and whether a decoder reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Surveyed {
    pub role: String,
    pub path: String,
    pub class: Option<String>,
    pub version: Option<u32>,
    pub readable: bool,
    /// Why no decoder reads it, naming the file, its class and its version.
    pub why: Option<String>,
}

/// A terrain as a listing shows it: every file a decoder would read, each
/// marked readable or not, and why the terrain cannot be opened when it
/// cannot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Survey {
    pub dir: String,
    pub id: Option<String>,
    pub files: Vec<Surveyed>,
    pub refusal: Option<String>,
}

/// Surveys one listed terrain in one pass: each file located once and each
/// header read once. Never refuses: what would refuse opening the terrain
/// is its `refusal`, and a file no decoder reads that opening does not
/// read is only marked unreadable.
pub fn survey(install: &Path, listed: &Listed) -> Survey {
    let root = install.join(TERRAINS_DIR).join(&listed.dir);
    let rel_root = format!("{TERRAINS_DIR}/{}", listed.dir);
    // In the order opening the terrain meets them.
    let mut refusal = match &listed.id {
        None => Some(format!("{rel_root}/{ENTRY_FILE} declares no id")),
        Some(_) => read_build(install).err().map(|r| r.0),
    };
    let mut surface = None;
    let mut ls = Listings::default();
    let mut files = Vec::new();
    for (role, find, decoder) in &ROLES {
        let found = locate(&mut ls, &root, &rel_root, role, find).unwrap_or_else(|e| {
            refusal.get_or_insert(e.0);
            Vec::new()
        });
        for f in found {
            let mut s = Surveyed {
                role: role.to_string(),
                path: f.path.clone(),
                class: None,
                version: None,
                readable: decoder.is_none(),
                why: None,
            };
            if let Some(want) = *decoder {
                let read = header(install, &f.path).and_then(|h| {
                    s.class = Some(h.class.clone());
                    s.version = Some(h.version);
                    expect(&f.path, role, &h, want)
                });
                s.readable = read.is_ok();
                s.why = read.as_ref().err().map(|e| e.0.clone());
                if *role == "surface" {
                    surface = read.err();
                }
            }
            files.push(s);
        }
    }
    if let (None, Some(e)) = (&refusal, surface) {
        refusal = Some(e.0);
    }
    Survey {
        dir: listed.dir.clone(),
        id: listed.id.clone(),
        files,
        refusal,
    }
}

/// The files, relative to the install, that `find` names for `role` under
/// the map, with the size and time their listing gives.
fn locate(
    ls: &mut Listings,
    root: &Path,
    rel_root: &str,
    role: &str,
    find: &Find,
) -> Result<Vec<Located>, Refusal> {
    let found = |path: String, s: &Seen| {
        Ok(Located {
            role: role.to_string(),
            size: s.size,
            modified: s.modified.ok_or_else(|| no_time(&path))?,
            path,
        })
    };
    match *find {
        Find::One(sub, ext) | Find::All(sub, ext) => {
            let Some(sub) = ls.child(root, sub).map(|s| s.name) else {
                return match find {
                    Find::One(..) => Err(Refusal(format!("{rel_root} has no {sub} directory"))),
                    _ => Ok(Vec::new()),
                };
            };
            let mut names: Vec<Seen> = ls
                .of(&root.join(&sub))
                .iter()
                .filter(|s| {
                    s.file && s.name.len() > ext.len() && s.name.to_ascii_lowercase().ends_with(ext)
                })
                .cloned()
                .collect();
            names.sort_by(|a, b| a.name.cmp(&b.name));
            if matches!(find, Find::One(..)) && names.len() != 1 {
                let found = if names.is_empty() {
                    "none".to_string()
                } else {
                    let n: Vec<&str> = names.iter().map(|s| s.name.as_str()).collect();
                    n.join(", ")
                };
                return Err(Refusal(format!(
                    "{rel_root}/{sub} must hold one {ext} file, and holds {found}"
                )));
            }
            names
                .iter()
                .map(|s| found(format!("{rel_root}/{sub}/{}", s.name), s))
                .collect()
        }
        Find::Named(parts) => {
            let mut at = root.to_path_buf();
            let mut rel = rel_root.to_string();
            let mut last = None;
            for part in parts {
                let Some(s) = ls.child(&at, part) else {
                    return Ok(Vec::new());
                };
                at.push(&s.name);
                rel = format!("{rel}/{}", s.name);
                last = Some(s);
            }
            match last {
                Some(s) if s.file => Ok(vec![found(rel, &s)?]),
                _ => Ok(Vec::new()),
            }
        }
    }
}

/// Each of `files` with the class and version its header states, refused
/// unless they name the decoder its role needs. A file of a plain role,
/// Lua source or `assets.assets`, is not opened.
pub fn classify(install: &Path, files: &[Located]) -> Result<Vec<FileEntry>, Refusal> {
    files.iter().map(|f| classify_one(install, f)).collect()
}

fn classify_one(install: &Path, f: &Located) -> Result<FileEntry, Refusal> {
    let decoder = ROLES
        .iter()
        .find(|(r, ..)| *r == f.role)
        .and_then(|(.., d)| *d);
    let mut entry = FileEntry {
        role: f.role.clone(),
        path: f.path.clone(),
        class: None,
        version: None,
        size: f.size,
        payload_size: None,
    };
    if let Some(want) = decoder {
        let h = header(install, &f.path)?;
        let got = expect(&f.path, &f.role, &h, want)?;
        entry.payload_size = got.has_data_base().then_some(h.data_base);
        entry.class = Some(h.class);
        entry.version = Some(h.version);
    }
    Ok(entry)
}

fn header(install: &Path, rel: &str) -> Result<container::Header, Refusal> {
    container::read(&install.join(rel))
        .map_err(|e| Refusal(format!("{rel} cannot be read: {e}")))?
        .map_err(|why| Refusal(format!("{rel} is not a map file: {why}")))
}

/// The decoder `h` names, refused unless it is the one `role` needs.
fn expect(rel: &str, role: &str, h: &container::Header, want: Decoder) -> Result<Decoder, Refusal> {
    let got = dispatch(rel, h)?;
    if got != want {
        return Err(Refusal(format!(
            "{rel} is a {} version {}, where a {role} file is a {}",
            h.class,
            h.version,
            want.class()
        )));
    }
    Ok(got)
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
    fn ties_break_by_form_then_position() {
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
        let surface = &map.surface;
        assert_eq!(surface.path, map.files[0].path);
        assert_eq!(surface.class.as_deref(), Some("landscape5::Surface5File"));
        assert_eq!(
            (surface.version, surface.payload_size),
            (Some(5), Some(4096))
        );
        assert_eq!(map.files[6].size, 5);

        let all = classify(&r, &map.files).unwrap();
        assert_eq!(all[0], map.surface);
        // The road network's u64 at 0x08 is not a data base.
        assert_eq!(all[1].class.as_deref(), Some("landscape4::lRoadNetwork"));
        assert_eq!(all[1].payload_size, None);
        assert_eq!((all[6].class.as_deref(), all[6].size), (None, 5));
    }

    #[test]
    fn opening_reads_the_surface_header_and_no_other() {
        let (_s, r) = install();
        container::READS.set(0);
        read_map(&r, "Synth").unwrap();
        assert_eq!(container::READS.get(), 1);
    }

    #[test]
    fn refuses_a_version_no_decoder_reads_naming_file_class_and_version() {
        let (_s, r) = install();
        put(
            &r,
            "Mods/terrains/Synth/surface/Synth.surface5",
            &head("landscape5::Surface5File", 6, 0, 0),
        );
        let e = read_map(&r, "Synth").unwrap_err();
        assert!(
            e.0.starts_with(
                "Mods/terrains/Synth/surface/Synth.surface5: landscape5::Surface5File version 6"
            ),
            "{e}"
        );
    }

    #[test]
    fn an_unknown_version_refuses_only_what_reads_its_role() {
        let (_s, r) = install();
        let rel = "Mods/terrains/Synth/AirfieldsTaxiways/A.rn5";
        put(&r, rel, &head("t5::rn5file", 99, 0, 0));
        let map = read_map(&r, "Synth").unwrap();
        let roles = |want: &[&str]| -> Vec<Located> {
            map.files
                .iter()
                .filter(|f| want.contains(&f.role.as_str()))
                .cloned()
                .collect()
        };
        let e = classify(&r, &roles(&["airfield_rn4", "airfield_rn5"])).unwrap_err();
        assert!(
            e.0.starts_with(&format!("{rel}: t5::rn5file version 99")),
            "{e}"
        );
        classify(&r, &roles(&["roads", "scene"])).unwrap();
    }

    #[test]
    fn states_a_role_from_metadata_and_sees_a_file_added_to_its_directory() {
        let (_s, r) = install();
        let map = read_map(&r, "Synth").unwrap();
        container::READS.set(0);
        let rows = |roles: &[&str]| -> Vec<(String, String)> {
            map.stat(&r, roles)
                .unwrap()
                .into_iter()
                .map(|f| (f.role, f.path.rsplit('/').next().unwrap().to_string()))
                .collect()
        };
        let pairs = |p: &[(&str, &str)]| -> Vec<(String, String)> {
            p.iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect()
        };
        assert_eq!(
            rows(&["lua", "airfield_rn4", "surface"]),
            pairs(&[
                ("surface", "Synth.surface5"),
                ("airfield_rn4", "A.rn4"),
                ("airfield_rn4", "B.rn4"),
                ("lua", "Beacons.lua"),
                ("lua", "towns.lua"),
            ])
        );
        put(
            &r,
            "Mods/terrains/Synth/AirfieldsTaxiways/C.rn4",
            &head("landscape4::lRoadNetwork", 5, 0, 0),
        );
        assert_eq!(rows(&["airfield_rn4"]).len(), 3);
        assert_eq!(container::READS.get(), 0, "a stat reads no header");
    }

    #[test]
    fn refuses_a_role_holding_another_class() {
        let (_s, r) = install();
        put(
            &r,
            "Mods/terrains/Synth/roads/Synth.rn4",
            &head("t5::rn5file", 0, 0, 0),
        );
        let e = classify(&r, &read_map(&r, "Synth").unwrap().files).unwrap_err();
        assert!(
            e.0.contains("is a t5::rn5file version 0, where a roads file"),
            "{e}"
        );
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
    fn resolves_a_path_as_the_disk_spells_it() {
        let (_s, r) = install();
        assert_eq!(
            resolve(&r, "mods/Terrains/synth/MAP/towns.lua").as_deref(),
            Some("Mods/terrains/Synth/map/towns.lua")
        );
        assert_eq!(resolve(&r, "Mods/terrains/Synth/map"), None);
        assert_eq!(resolve(&r, "Mods/terrains/Synth/../Synth/entry.lua"), None);
        assert_eq!(resolve(&r, "/Mods/terrains/Synth/entry.lua"), None);
    }

    #[test]
    fn refuses_a_file_that_is_not_a_container() {
        let (_s, r) = install();
        put(&r, "Mods/terrains/Synth/Scenes/Synth.scn5", b"-- lua\n");
        let e = classify(&r, &read_map(&r, "Synth").unwrap().files).unwrap_err();
        assert!(e.0.contains("Synth.scn5 is not a map file"), "{e}");
    }

    /// A second terrain whose folder is not its id, as Germany's is not.
    fn add_germany(r: &Path) {
        let m = "Mods/terrains/GermanyColdWar";
        put(r, &format!("{m}/entry.lua"), entry("GermanyCW").as_bytes());
        put(
            r,
            &format!("{m}/Surface/GermanyCW.surface5"),
            &head("landscape5::Surface5File", 5, 0, 0),
        );
        put(
            r,
            &format!("{m}/roads/GermanyCW.rn4"),
            &head("landscape4::lRoadNetwork", 5, 0, 0),
        );
        put(
            r,
            &format!("{m}/Scenes/GermanyCW.scn5"),
            &head("landscape5::Scene5File", 9, 0, 0),
        );
    }

    #[test]
    fn finds_a_terrain_by_the_id_its_entry_declares() {
        let (_s, r) = install();
        add_germany(&r);
        // A directory with no entry.lua is not a terrain.
        put(&r, "Mods/terrains/Shared/readme.txt", b"x");
        let listed = terrains(&r).unwrap();
        let rows: Vec<(&str, Option<&str>)> = listed
            .iter()
            .map(|t| (t.dir.as_str(), t.id.as_deref()))
            .collect();
        assert_eq!(
            rows,
            [
                ("GermanyColdWar", Some("GermanyCW")),
                ("Synth", Some("Synth"))
            ]
        );
        assert_eq!(find(&r, "GermanyCW").unwrap(), "GermanyColdWar");
        let map = open(&r, "GermanyCW").unwrap();
        assert_eq!(
            (map.dir.as_str(), map.theatre.as_str()),
            ("GermanyColdWar", "GermanyCW")
        );
        // The folder's name is not an id.
        let e = find(&r, "GermanyColdWar").unwrap_err();
        assert_eq!(
            e.0,
            "no terrain GermanyColdWar in this install; the ids it holds are GermanyCW, Synth"
        );
    }

    #[test]
    fn refuses_an_id_two_terrains_declare() {
        let (_s, r) = install();
        add_germany(&r);
        put(
            &r,
            "Mods/terrains/Synth/entry.lua",
            entry("GermanyCW").as_bytes(),
        );
        let e = find(&r, "GermanyCW").unwrap_err();
        assert!(
            e.0.starts_with("GermanyColdWar and Synth each declare"),
            "{e}"
        );
    }

    #[test]
    fn surveys_each_file_and_marks_one_no_decoder_reads() {
        let (_s, r) = install();
        add_germany(&r);
        put(
            &r,
            "Mods/terrains/GermanyColdWar/Scenes/GermanyCW.scn5",
            &head("landscape5::Scene5File", 10, 0, 0),
        );
        let listed = terrains(&r).unwrap();
        let germany = survey(&r, &listed[0]);
        let rows: Vec<(&str, Option<u32>, bool)> = germany
            .files
            .iter()
            .map(|f| (f.role.as_str(), f.version, f.readable))
            .collect();
        assert_eq!(
            rows,
            [
                ("surface", Some(5), true),
                ("roads", Some(5), true),
                ("scene", Some(10), false)
            ]
        );
        let why = germany.files[2].why.as_deref().unwrap();
        assert!(
            why.contains("GermanyCW.scn5: landscape5::Scene5File version 10 has no decoder"),
            "{why}"
        );
        assert!(germany.files[..2].iter().all(|f| f.why.is_none()));
        // Opening reads the surface alone, so the scene file refuses only
        // what reads scenery.
        assert_eq!(germany.refusal, None);
        container::READS.set(0);
        let synth = survey(&r, &listed[1]);
        assert_eq!(container::READS.get(), 6, "each header is read once");
        assert_eq!(synth.refusal, None);
        assert!(synth.files.iter().all(|f| f.readable));
        assert_eq!(synth.files.len(), 9);

        put(
            &r,
            "Mods/terrains/GermanyColdWar/Surface/GermanyCW.surface5",
            &head("landscape5::Surface5File", 6, 0, 0),
        );
        let germany = survey(&r, &listed[0]);
        assert!(!germany.files[0].readable);
        let why = germany.refusal.unwrap();
        assert!(why.contains("Surface5File version 6"), "{why}");
        assert_eq!(Some(why), read_map(&r, "GermanyColdWar").err().map(|e| e.0));
    }

    #[test]
    fn surveys_a_terrain_that_declares_no_id_or_lacks_a_file() {
        let (_s, r) = install();
        put(&r, "Mods/terrains/Synth/entry.lua", b"theatre = {}\n");
        let listed = terrains(&r).unwrap();
        assert_eq!(listed[0].id, None);
        let s = survey(&r, &listed[0]);
        assert!(s.refusal.unwrap().ends_with("entry.lua declares no id"));

        let (_s, r) = install();
        fs::remove_file(r.join("Mods/terrains/Synth/roads/Synth.rn4")).unwrap();
        let s = survey(&r, &terrains(&r).unwrap()[0]);
        assert!(s.files.iter().all(|f| f.role != "roads"));
        assert!(s.refusal.unwrap().contains("one .rn4 file"));
    }
}
