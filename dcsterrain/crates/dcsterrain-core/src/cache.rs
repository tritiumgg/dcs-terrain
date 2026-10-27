//! The cache beside the install: one directory per terrain and DCS build,
//! holding what is too slow to derive per query. Deleting it loses nothing
//! but time; the next query rebuilds what it needs.
//!
//! An entry is a raw file that opens with a header: magic, the entry's
//! format version, and its key, the build and a stat of each install file it
//! was derived from. A missing entry, or one whose key or version does not
//! match, is rebuilt, and only that entry. An entry is written under a
//! temporary name and renamed into place, so two builders at once leave one
//! valid copy.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::UNIX_EPOCH;

use crate::decode::container;
use crate::decode::install::Map;
use crate::source::Refusal;

/// The live validation sortie writes this; nothing can rebuild it, so
/// clearing the cache asks before deleting it.
pub const VALIDATION_FILE: &str = "validation.json";

/// Marks a directory as this tool's cache, so a mistyped `--cache` cannot
/// have `clear` or `prune` delete somebody's files. Backup tools that honor
/// the convention skip the directory too.
pub const TAG_FILE: &str = "CACHEDIR.TAG";
const TAG: &str = "Signature: 8a477f597d28d172789f06886806bc55\n\
                   # This directory is the dcs-terrain cache; it can be rebuilt.\n";

const MAGIC: &[u8; 8] = b"DCSTRCH\0";
const MAX_KEY: u64 = 1 << 20;

/// One kind of entry: what it is called, which install files it reads, and
/// how it is built from them.
pub trait Entry: Sync {
    /// The file name in the build's directory, such as `mesh.toc`.
    fn name(&self) -> &str;
    /// Raised whenever the entry's layout changes, which rebuilds it.
    fn version(&self) -> u32;
    /// The roles of the install files it is derived from, as the terrain's
    /// files name them; its key covers these files only.
    fn roles(&self) -> &[&str];
    fn build(&self, install: &Path, map: &Map) -> Result<Vec<u8>, Refusal>;
}

/// Every kind of entry `index` builds.
pub fn entries() -> Vec<&'static dyn Entry> {
    vec![&crate::source::install::MESH_TOC]
}

/// Where the cache lives when `--cache` does not say.
pub fn default_root() -> Result<PathBuf, Refusal> {
    platform_root(std::env::consts::OS, |k| std::env::var_os(k)).ok_or_else(|| {
        Refusal(
            "no cache directory: the user's cache location is not set; pass --cache <dir>".into(),
        )
    })
}

/// The cache directory on `os`, given the environment: the platform's cache
/// location with `dcs-terrain` under it.
fn platform_root(os: &str, var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    // A relative value is ignored, as the XDG specification says.
    let abs = |k: &str| var(k).map(PathBuf::from).filter(|p| p.is_absolute());
    match os {
        "windows" => abs("LOCALAPPDATA").map(|p| p.join("dcs-terrain").join("cache")),
        "macos" => abs("HOME").map(|p| p.join("Library/Caches/dcs-terrain")),
        _ => abs("XDG_CACHE_HOME")
            .or_else(|| abs("HOME").map(|p| p.join(".cache")))
            .map(|p| p.join("dcs-terrain")),
    }
}

/// A terrain id or build as a directory name: anything but ASCII letters,
/// digits, `.`, `-` and `_` becomes `_`, and a name of dots is not a path
/// step.
pub fn dir_name(s: &str) -> String {
    let n: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if n.chars().all(|c| c == '.') {
        n.replace('.', "_") + "_"
    } else {
        n
    }
}

/// The cache directory and what it holds.
#[derive(Debug, Clone)]
pub struct Cache {
    root: PathBuf,
}

/// One terrain and build's directory, and what is in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub terrain: String,
    pub build: String,
    pub path: PathBuf,
    /// Each entry, by name, and its size in bytes; a directory's is the sum
    /// of the files under it.
    pub entries: Vec<(String, u64)>,
}

impl Stored {
    pub fn size(&self) -> u64 {
        self.entries.iter().map(|(_, s)| s).sum()
    }
}

impl Cache {
    pub fn new(root: impl Into<PathBuf>) -> Cache {
        Cache { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory of `terrain` at `build`, whether or not it exists.
    pub fn dir(&self, terrain: &str, build: &str) -> PathBuf {
        self.root.join(dir_name(terrain)).join(dir_name(build))
    }

    /// The entries of one opened terrain.
    pub fn terrain(&self, install: &Path, map: &Map) -> TerrainCache {
        TerrainCache {
            cache: self.clone(),
            dir: self.dir(&map.theatre, &map.build.version),
            install: install.to_path_buf(),
            map: map.clone(),
        }
    }

    /// What is stored for `terrain` at `build`, `None` when nothing is.
    pub fn stored(&self, terrain: &str, build: &str) -> Option<Stored> {
        let path = self.dir(terrain, build);
        is_real_dir(&path).then(|| Stored {
            terrain: dir_name(terrain),
            build: dir_name(build),
            entries: entries_in(&path),
            path,
        })
    }

    /// Every terrain and build stored, sorted. No cache directory is an empty
    /// list.
    pub fn list(&self) -> Result<Vec<Stored>, Refusal> {
        let mut out = Vec::new();
        for terrain in subdirs(&self.root)? {
            for build in subdirs(&self.root.join(&terrain))? {
                let path = self.root.join(&terrain).join(&build);
                out.push(Stored {
                    entries: entries_in(&path),
                    terrain: terrain.clone(),
                    build,
                    path,
                });
            }
        }
        Ok(out)
    }

    /// Deletes every build but `keep`, of every terrain, and returns what it
    /// deleted. An old build's `validation.json` goes with it: its figures
    /// are for terrain files no longer installed.
    pub fn prune(&self, keep: &str) -> Result<Vec<Stored>, Refusal> {
        self.check_tag()?;
        let keep = dir_name(keep);
        let mut gone = Vec::new();
        for s in self.list()? {
            if s.build != keep {
                fs::remove_dir_all(&s.path).map_err(|e| cannot("delete", &s.path, e))?;
                gone.push(s);
            }
        }
        self.drop_empty_terrains()?;
        Ok(gone)
    }

    /// Each `validation.json` that clearing `terrain`, or every terrain, would
    /// delete.
    pub fn validation_files(&self, terrain: Option<&str>) -> Result<Vec<PathBuf>, Refusal> {
        Ok(self
            .chosen(terrain)?
            .into_iter()
            .map(|s| s.path.join(VALIDATION_FILE))
            .filter(|p| p.is_file())
            .collect())
    }

    /// Deletes everything stored for `terrain`, or for every terrain, keeping
    /// each `validation.json` unless `validation` is set. Returns what it
    /// cleared.
    pub fn clear(&self, terrain: Option<&str>, validation: bool) -> Result<Vec<Stored>, Refusal> {
        self.check_tag()?;
        let chosen = self.chosen(terrain)?;
        for s in &chosen {
            for e in fs::read_dir(&s.path).map_err(|e| cannot("read", &s.path, e))? {
                let e = e.map_err(|e| cannot("read", &s.path, e))?;
                let path = e.path();
                if !validation && e.file_name() == VALIDATION_FILE {
                    continue;
                }
                // A link is deleted as a link, never followed.
                let done = if e.file_type().is_ok_and(|t| t.is_dir()) {
                    fs::remove_dir_all(&path)
                } else {
                    fs::remove_file(&path)
                };
                done.map_err(|e| cannot("delete", &path, e))?;
            }
            // Only an empty directory goes; one left holding its figures stays.
            let _ = fs::remove_dir(&s.path);
        }
        self.drop_empty_terrains()?;
        Ok(chosen)
    }

    fn chosen(&self, terrain: Option<&str>) -> Result<Vec<Stored>, Refusal> {
        let all = self.list()?;
        Ok(match terrain {
            None => all,
            Some(t) => all
                .into_iter()
                .filter(|s| s.terrain == dir_name(t))
                .collect(),
        })
    }

    fn drop_empty_terrains(&self) -> Result<(), Refusal> {
        for t in subdirs(&self.root)? {
            // Fails, harmlessly, on one that still holds a build.
            let _ = fs::remove_dir(self.root.join(t));
        }
        Ok(())
    }

    /// Refuses to delete under a directory this tool did not make. A root
    /// that does not exist holds nothing to delete.
    pub fn check_tag(&self) -> Result<(), Refusal> {
        if !self.root.exists() || self.root.join(TAG_FILE).is_file() {
            return Ok(());
        }
        Err(Refusal(format!(
            "{} has no {TAG_FILE}, so it is not a dcs-terrain cache; nothing was deleted",
            self.root.display()
        )))
    }

    /// Makes the root a cache: tags it, if it is new or empty. A directory
    /// that already holds something else is refused, since `clear` would
    /// then delete under it.
    fn claim(&self) -> Result<(), Refusal> {
        let tag = self.root.join(TAG_FILE);
        let empty = match fs::read_dir(&self.root) {
            Ok(mut r) => r.next().is_none(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => true,
            Err(e) => return Err(cannot("read", &self.root, e)),
        };
        // The tag is looked for after the listing: a concurrent builder tags
        // the root before it puts anything else in it, so a root found full
        // is either tagged by now or not a cache.
        if tag.is_file() {
            return Ok(());
        }
        if !empty {
            return Err(Refusal(format!(
                "{} holds other files and no {TAG_FILE}, so it is not a dcs-terrain cache; \
                 name a new or empty directory",
                self.root.display()
            )));
        }
        fs::create_dir_all(&self.root).map_err(|e| cannot("create", &self.root, e))?;
        match File::create_new(&tag) {
            Ok(mut f) => f
                .write_all(TAG.as_bytes())
                .map_err(|e| cannot("write", &tag, e)),
            // Another builder tagged it first.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            Err(e) => Err(cannot("create", &tag, e)),
        }
    }
}

/// A directory itself, not a link to one.
fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

/// Each directory directly under `dir`, sorted; none when `dir` is missing.
/// A link to a directory is not one, so nothing is deleted through it.
fn subdirs(dir: &Path) -> Result<Vec<String>, Refusal> {
    let read = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(cannot("read", dir, e)),
    };
    let mut out: Vec<String> = read
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    Ok(out)
}

/// The entries in a build's directory, sorted, without a builder's
/// temporary files.
fn entries_in(dir: &Path) -> Vec<(String, u64)> {
    let mut out: Vec<(String, u64)> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .filter(|(n, _)| !is_temporary(n))
        .map(|(n, p)| (n, size_of(&p)))
        .collect();
    out.sort();
    out
}

fn size_of(path: &Path) -> u64 {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| size_of(&e.path()))
            .sum(),
        Ok(m) => m.len(),
        Err(_) => 0,
    }
}

fn is_temporary(name: &str) -> bool {
    name.starts_with('.') && name.ends_with(".tmp")
}

fn cannot(what: &str, path: &Path, e: io::Error) -> Refusal {
    Refusal(format!("cannot {what} {}: {e}", path.display()))
}

/// The entries of one terrain at its installed build.
#[derive(Debug, Clone)]
pub struct TerrainCache {
    cache: Cache,
    dir: PathBuf,
    install: PathBuf,
    map: Map,
}

/// Whether an entry on disk can be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Current,
    Missing,
    /// It is there and must be rebuilt; the reason says what differs.
    Stale(String),
}

/// An entry ready to read: its file, held open since its header was
/// checked so a builder replacing it cannot change what is read, and where
/// its payload starts in it.
#[derive(Debug)]
pub struct Opened {
    pub path: PathBuf,
    pub file: File,
    pub offset: u64,
    pub len: u64,
    /// Whether this call built it.
    pub built: bool,
}

impl Opened {
    pub fn payload(&self) -> io::Result<Vec<u8>> {
        let mut f = &self.file;
        f.seek(SeekFrom::Start(self.offset))?;
        let mut out = Vec::new();
        f.take(self.len).read_to_end(&mut out)?;
        Ok(out)
    }
}

impl TerrainCache {
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The key an entry built now would carry: the build, then for each
    /// install file of the entry's roles its path, size, payload size,
    /// modification time, class and version, each read from the disk now.
    pub fn key(&self, entry: &dyn Entry) -> Result<String, Refusal> {
        let b = &self.map.build;
        let mut key = format!("build {} {}\n", b.version, b.timestamp);
        for f in self
            .map
            .files
            .iter()
            .filter(|f| entry.roles().contains(&f.role.as_str()))
        {
            let path = self.install.join(&f.path);
            let meta = fs::metadata(&path).map_err(|e| cannot("read", &path, e))?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .ok_or_else(|| Refusal(format!("{} has no modification time", path.display())))?
                .as_nanos();
            let (class, version, payload) = match f.class {
                None => ("-".to_string(), "-".to_string(), "-".to_string()),
                Some(_) => {
                    let h = container::read(&path)
                        .map_err(|e| cannot("read", &path, e))?
                        .map_err(|why| Refusal(format!("{}: {why}", f.path)))?;
                    let payload = f
                        .payload_size
                        .map_or("-".to_string(), |_| h.data_base.to_string());
                    (h.class, h.version.to_string(), payload)
                }
            };
            key.push_str(&format!(
                "{}\t{}\t{payload}\t{modified}\t{class}\t{version}\n",
                f.path,
                meta.len()
            ));
        }
        Ok(key)
    }

    pub fn state(&self, entry: &dyn Entry) -> Result<State, Refusal> {
        let key = self.key(entry)?;
        Ok(match read_head(&self.dir.join(entry.name())) {
            Ok(head) => judge(&head, entry.version(), &key),
            Err(e) if e.kind() == io::ErrorKind::NotFound => State::Missing,
            Err(e) => State::Stale(format!("its header cannot be read: {e}")),
        })
    }

    /// The entry, built first if it is missing or stale.
    pub fn get(&self, entry: &dyn Entry) -> Result<Opened, Refusal> {
        let path = self.dir.join(entry.name());
        let key = self.key(entry)?;
        if let Ok(head) = read_head(&path)
            && judge(&head, entry.version(), &key) == State::Current
        {
            return Ok(head.opened(path, false));
        }
        let payload = entry.build(&self.install, &self.map)?;
        let bytes = encode(entry.version(), &key, &payload);
        self.write(entry.name(), &bytes, |dest| {
            read_head(dest).is_ok_and(|h| judge(&h, entry.version(), &key) == State::Current)
        })?;
        let head = read_head(&path).map_err(|e| cannot("read", &path, e))?;
        Ok(head.opened(path, true))
    }

    /// Builds every entry `index` knows of that is not current, and creates
    /// the directory even when there is none to build. Returns each entry's
    /// name and whether it was built.
    pub fn index(&self, entries: &[&dyn Entry]) -> Result<Vec<(String, bool)>, Refusal> {
        self.make_dir()?;
        entries
            .iter()
            .map(|e| Ok((e.name().to_string(), self.get(*e)?.built)))
            .collect()
    }

    /// Creates the directory, refusing one inside the install, which is
    /// never written to, and sweeps temporary files a crashed builder left.
    fn make_dir(&self) -> Result<(), Refusal> {
        if self.dir.starts_with(&self.install) {
            return Err(Refusal(format!(
                "the cache {} is inside the install {}, which is never written to",
                self.cache.root().display(),
                self.install.display()
            )));
        }
        self.cache.claim()?;
        fs::create_dir_all(&self.dir).map_err(|e| cannot("create", &self.dir, e))?;
        // An hour is far longer than any build takes, so a live builder's
        // file is never swept.
        let old = |e: &fs::DirEntry| {
            e.metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|t| t.elapsed().is_ok_and(|age| age.as_secs() > 3600))
        };
        for e in fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            if is_temporary(&e.file_name().to_string_lossy()) && old(&e) {
                let _ = fs::remove_file(e.path());
            }
        }
        Ok(())
    }

    /// Writes `name` under a temporary name and renames it into place. When
    /// the rename fails, which Windows does while another process has the
    /// old file open, the write still succeeds if `current` says the file
    /// already there is one another builder just put in place.
    fn write(
        &self,
        name: &str,
        bytes: &[u8],
        current: impl Fn(&Path) -> bool,
    ) -> Result<(), Refusal> {
        static N: AtomicU32 = AtomicU32::new(0);
        self.make_dir()?;
        let dest = self.dir.join(name);
        let tmp = self.dir.join(format!(
            ".{name}.{}.{}.tmp",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let written = File::create(&tmp).and_then(|mut f| {
            f.write_all(bytes)?;
            f.sync_all()
        });
        if let Err(e) = written {
            let _ = fs::remove_file(&tmp);
            return Err(cannot("write", &tmp, e));
        }
        match fs::rename(&tmp, &dest) {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                if current(&dest) {
                    Ok(())
                } else {
                    Err(cannot("replace", &dest, e))
                }
            }
        }
    }
}

/// An entry's header as read from the disk, and the file it was read from.
struct Head {
    file: File,
    version: u32,
    key: String,
    offset: u64,
    len: u64,
    file_len: u64,
}

impl Head {
    fn opened(self, path: PathBuf, built: bool) -> Opened {
        Opened {
            path,
            file: self.file,
            offset: self.offset,
            len: self.len,
            built,
        }
    }
}

fn encode(version: u32, key: &str, payload: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(24 + key.len() + payload.len());
    b.extend_from_slice(MAGIC);
    b.extend_from_slice(&version.to_le_bytes());
    b.extend_from_slice(&(key.len() as u32).to_le_bytes());
    b.extend_from_slice(key.as_bytes());
    b.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    b.extend_from_slice(payload);
    b
}

fn read_head(path: &Path) -> io::Result<Head> {
    let bad = |why: &str| io::Error::new(io::ErrorKind::InvalidData, why.to_string());
    let mut f = File::open(path)?;
    let file_len = f.metadata()?.len();
    let mut fixed = [0u8; 16];
    f.read_exact(&mut fixed)?;
    if &fixed[..8] != MAGIC {
        return Err(bad("not a cache entry"));
    }
    let version = u32::from_le_bytes(fixed[8..12].try_into().expect("4 bytes"));
    let key_len = u64::from(u32::from_le_bytes(
        fixed[12..16].try_into().expect("4 bytes"),
    ));
    // A key is a few lines per install file; a longer one is not ours, and
    // must not size an allocation.
    if key_len > MAX_KEY {
        return Err(bad("its key is too long"));
    }
    if 24 + key_len > file_len {
        return Err(bad("its key runs past the end"));
    }
    let mut key = vec![0u8; key_len as usize];
    f.read_exact(&mut key)?;
    let mut len = [0u8; 8];
    f.read_exact(&mut len)?;
    Ok(Head {
        file: f,
        version,
        key: String::from_utf8(key).map_err(|_| bad("its key is not UTF-8"))?,
        offset: 24 + key_len,
        len: u64::from_le_bytes(len),
        file_len,
    })
}

fn judge(head: &Head, version: u32, key: &str) -> State {
    if head.version != version {
        State::Stale(format!("its version is {}, not {version}", head.version))
    } else if head.offset.checked_add(head.len) != Some(head.file_len) {
        State::Stale("its length does not match its header".into())
    } else if head.key != key {
        State::Stale("an install file or the build changed".into())
    } else {
        State::Current
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::install;
    use crate::scratch::Scratch;
    use std::sync::atomic::AtomicUsize;
    use std::time::{Duration, SystemTime};

    /// A test entry over the files of one role, counting its builds.
    struct Counted {
        name: &'static str,
        version: u32,
        roles: &'static [&'static str],
        builds: AtomicUsize,
    }

    impl Counted {
        const fn new(name: &'static str, roles: &'static [&'static str]) -> Counted {
            Counted {
                name,
                version: 1,
                roles,
                builds: AtomicUsize::new(0),
            }
        }
        fn builds(&self) -> usize {
            self.builds.load(Ordering::SeqCst)
        }
    }

    impl Entry for Counted {
        fn name(&self) -> &str {
            self.name
        }
        fn version(&self) -> u32 {
            self.version
        }
        fn roles(&self) -> &[&str] {
            self.roles
        }
        fn build(&self, _: &Path, map: &Map) -> Result<Vec<u8>, Refusal> {
            self.builds.fetch_add(1, Ordering::SeqCst);
            Ok(format!("{} of {}", self.name, map.theatre).into_bytes())
        }
    }

    const SURFACE: &str = "Mods/terrains/Caucasus/Surface/Caucasus.surface5";
    const ROADS: &str = "Mods/terrains/Caucasus/roads/Caucasus.rn4";

    fn put(root: &Path, rel: &str, bytes: &[u8]) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }

    fn build_file(root: &Path, timestamp: &str) {
        let cfg = format!(r#"{{"version": "2.9.0.1", "timestamp": "{timestamp}"}}"#);
        put(root, install::BUILD_FILE, cfg.as_bytes());
    }

    /// An install holding Caucasus with its three terrain files.
    fn install_tree() -> Scratch {
        let s = Scratch::new();
        build_file(&s.0, "20000101-000000");
        put(
            &s.0,
            "Mods/terrains/Caucasus/entry.lua",
            b"theatre = {\n\t['id'] = \"Caucasus\";\n}\n",
        );
        put(
            &s.0,
            SURFACE,
            &container::build("landscape5::Surface5File", 5, 100),
        );
        put(
            &s.0,
            ROADS,
            &container::build("landscape4::lRoadNetwork", 5, 0),
        );
        put(
            &s.0,
            "Mods/terrains/Caucasus/Scenes/Caucasus.scn5",
            &container::build("landscape5::Scene5File", 9, 0),
        );
        s
    }

    struct Fixture {
        install: Scratch,
        cache: Scratch,
    }

    impl Fixture {
        fn new() -> Fixture {
            Fixture {
                install: install_tree(),
                cache: Scratch::new(),
            }
        }
        fn terrain(&self) -> TerrainCache {
            let map = install::open(&self.install.0, "Caucasus").unwrap();
            Cache::new(&self.cache.0).terrain(&self.install.0, &map)
        }
        fn touch(&self, rel: &str, secs: u64) {
            let f = File::options()
                .write(true)
                .open(self.install.0.join(rel))
                .unwrap();
            f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
                .unwrap();
        }
    }

    #[test]
    fn a_change_rebuilds_only_the_entry_that_reads_it() {
        let f = Fixture::new();
        let mesh = Counted::new("mesh.toc", &["surface"]);
        let roads = Counted::new("roads.graph", &["roads"]);
        let both = |t: &TerrainCache| {
            t.get(&mesh).unwrap();
            t.get(&roads).unwrap();
            (mesh.builds(), roads.builds())
        };
        // Fix the times first, so each change below is the only one.
        f.touch(SURFACE, 1_000_000);
        f.touch(ROADS, 1_000_000);
        let t = f.terrain();
        assert_eq!(both(&t), (1, 1));
        assert_eq!(both(&t), (1, 1), "a current entry is not rebuilt");
        assert_eq!(
            t.get(&mesh).unwrap().payload().unwrap(),
            b"mesh.toc of Caucasus"
        );

        // The size.
        let mut grown = container::build("landscape5::Surface5File", 5, 100);
        grown.push(0);
        put(&f.install.0, SURFACE, &grown);
        f.touch(SURFACE, 1_000_000);
        assert_eq!(both(&t), (2, 1));

        // The payload size alone, the file's size and time unchanged.
        let mut moved = container::build("landscape5::Surface5File", 5, 101);
        moved.push(0);
        put(&f.install.0, SURFACE, &moved);
        f.touch(SURFACE, 1_000_000);
        assert_eq!(
            fs::metadata(f.install.0.join(SURFACE)).unwrap().len(),
            grown.len() as u64
        );
        assert_eq!(both(&t), (3, 1));

        // The time alone.
        f.touch(ROADS, 2_000_000);
        assert_eq!(both(&t), (3, 2));

        // The entry's own version.
        let mesh2 = Counted {
            version: 2,
            ..Counted::new("mesh.toc", &["surface"])
        };
        assert!(matches!(t.state(&mesh2).unwrap(), State::Stale(_)));
        assert!(t.get(&mesh2).unwrap().built);
        assert_eq!(
            both(&t),
            (4, 2),
            "version 1 is stale against version 2's entry"
        );

        // The build, which every entry's key carries.
        build_file(&f.install.0, "20000202-000000");
        let t = f.terrain();
        assert_eq!(both(&t), (5, 3));
    }

    #[test]
    fn a_missing_truncated_or_foreign_entry_is_rebuilt() {
        let f = Fixture::new();
        let mesh = Counted::new("mesh.toc", &["surface"]);
        let t = f.terrain();
        assert_eq!(t.state(&mesh).unwrap(), State::Missing);
        let got = t.get(&mesh).unwrap();
        assert!(got.built);
        assert_eq!(t.state(&mesh).unwrap(), State::Current);

        let bytes = fs::read(&got.path).unwrap();
        fs::write(&got.path, &bytes[..bytes.len() - 1]).unwrap();
        assert!(matches!(t.state(&mesh).unwrap(), State::Stale(_)));
        assert!(t.get(&mesh).unwrap().built);

        fs::write(&got.path, b"not an entry at all").unwrap();
        assert!(matches!(t.state(&mesh).unwrap(), State::Stale(_)));
        assert_eq!(
            t.get(&mesh).unwrap().payload().unwrap(),
            b"mesh.toc of Caucasus"
        );
        assert_eq!(mesh.builds(), 3);
    }

    #[test]
    fn concurrent_builders_leave_one_valid_entry() {
        let f = Fixture::new();
        let mesh = Counted::new("mesh.toc", &["surface"]);
        let t = f.terrain();
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| {
                    let got = t.get(&mesh).unwrap();
                    assert_eq!(got.payload().unwrap(), b"mesh.toc of Caucasus");
                });
            }
        });
        assert_eq!(t.state(&mesh).unwrap(), State::Current);
        let names: Vec<String> = entries_in(t.dir()).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["mesh.toc"], "no temporary file is left behind");
    }

    #[test]
    fn index_lays_out_terrain_and_build_and_tags_the_root() {
        let f = Fixture::new();
        let mesh = Counted::new("mesh.toc", &["surface"]);
        let t = f.terrain();
        assert_eq!(t.index(&[&mesh]).unwrap(), [("mesh.toc".to_string(), true)]);
        assert_eq!(
            t.index(&[&mesh]).unwrap(),
            [("mesh.toc".to_string(), false)]
        );
        assert_eq!(t.dir(), f.cache.0.join("Caucasus").join("2.9.0.1"));
        assert!(f.cache.0.join(TAG_FILE).is_file());

        let cache = Cache::new(&f.cache.0);
        let stored = cache.stored("Caucasus", "2.9.0.1").unwrap();
        assert_eq!(stored.entries.len(), 1);
        assert_eq!(
            stored.size(),
            fs::metadata(t.dir().join("mesh.toc")).unwrap().len()
        );
        assert!(cache.stored("Caucasus", "2.9.0.2").is_none());
    }

    #[test]
    fn prune_keeps_the_installed_build_and_clear_can_keep_validation() {
        let f = Fixture::new();
        let mesh = Counted::new("mesh.toc", &["surface"]);
        f.terrain().index(&[&mesh]).unwrap();
        let cache = Cache::new(&f.cache.0);
        put(&f.cache.0, "Caucasus/2.8.0.1/mesh.toc", b"old");
        put(&f.cache.0, "Nevada/2.8.0.1/validation.json", b"{}");
        put(&f.cache.0, "Caucasus/2.9.0.1/validation.json", b"{}");
        put(&f.cache.0, "Caucasus/2.9.0.1/horizon/0_0", b"tile");

        let gone = cache.prune("2.9.0.1").unwrap();
        let gone: Vec<(&str, &str)> = gone.iter().map(|s| (&*s.terrain, &*s.build)).collect();
        assert_eq!(gone, [("Caucasus", "2.8.0.1"), ("Nevada", "2.8.0.1")]);
        assert!(!f.cache.0.join("Nevada").exists());
        assert_eq!(cache.list().unwrap().len(), 1);

        assert_eq!(cache.validation_files(None).unwrap().len(), 1);
        cache.clear(Some("Caucasus"), false).unwrap();
        let left = cache.stored("Caucasus", "2.9.0.1").unwrap();
        assert_eq!(left.entries, [(VALIDATION_FILE.to_string(), 2)]);
        cache.clear(None, true).unwrap();
        assert!(cache.list().unwrap().is_empty());
        assert!(f.cache.0.join(TAG_FILE).is_file(), "the root stays a cache");
    }

    #[test]
    fn refuses_to_make_a_cache_of_a_full_directory_or_inside_the_install() {
        let f = Fixture::new();
        put(&f.cache.0, "photos/2024/cat.jpg", b"meow");
        let mesh = Counted::new("mesh.toc", &["surface"]);
        let err = f.terrain().index(&[&mesh]).unwrap_err().0;
        assert!(err.contains("holds other files"), "{err}");
        assert!(!f.cache.0.join(TAG_FILE).exists());

        let map = install::open(&f.install.0, "Caucasus").unwrap();
        let inside = Cache::new(f.install.0.join("cache")).terrain(&f.install.0, &map);
        let err = inside.index(&[&mesh]).unwrap_err().0;
        assert!(err.contains("inside the install"), "{err}");
        assert!(!f.install.0.join("cache").exists());
    }

    #[test]
    fn a_hostile_header_is_stale_and_sizes_nothing() {
        let f = Fixture::new();
        let mesh = Counted::new("mesh.toc", &["surface"]);
        let t = f.terrain();
        let path = t.get(&mesh).unwrap().path;
        let mut long_key = MAGIC.to_vec();
        long_key.extend_from_slice(&1u32.to_le_bytes());
        long_key.extend_from_slice(&u32::MAX.to_le_bytes());
        fs::write(&path, &long_key).unwrap();
        assert!(matches!(t.state(&mesh).unwrap(), State::Stale(_)));

        let mut huge = encode(1, &t.key(&mesh).unwrap(), b"");
        let at = huge.len() - 8;
        huge[at..].copy_from_slice(&u64::MAX.to_le_bytes());
        fs::write(&path, &huge).unwrap();
        assert!(matches!(t.state(&mesh).unwrap(), State::Stale(_)));
        assert_eq!(
            t.get(&mesh).unwrap().payload().unwrap(),
            b"mesh.toc of Caucasus"
        );
    }

    #[test]
    fn an_opened_entry_reads_the_file_its_header_came_from() {
        let f = Fixture::new();
        let mesh = Counted::new("mesh.toc", &["surface"]);
        let t = f.terrain();
        let held = t.get(&mesh).unwrap();
        // Another builder replaces it, with a longer key and so a later
        // payload.
        let other = encode(1, &format!("{}x\n", t.key(&mesh).unwrap()), b"other");
        let tmp = t.dir().join(".mesh.toc.test.tmp");
        fs::write(&tmp, other).unwrap();
        fs::rename(&tmp, &held.path).unwrap();
        assert_eq!(held.payload().unwrap(), b"mesh.toc of Caucasus");
    }

    #[cfg(unix)]
    #[test]
    fn clear_and_prune_never_follow_a_link_out_of_the_cache() {
        let f = Fixture::new();
        let mesh = Counted::new("mesh.toc", &["surface"]);
        f.terrain().index(&[&mesh]).unwrap();
        let outside = Scratch::new();
        put(&outside.0, "2.8.0.1/keep", b"mine");
        put(&outside.0, "keep", b"mine");
        std::os::unix::fs::symlink(&outside.0, f.cache.0.join("Linked")).unwrap();
        std::os::unix::fs::symlink(&outside.0, f.cache.0.join("Caucasus/2.9.0.1/horizon")).unwrap();
        let cache = Cache::new(&f.cache.0);
        cache.prune("2.9.0.1").unwrap();
        cache.clear(None, true).unwrap();
        assert!(outside.0.join("2.8.0.1/keep").is_file());
        assert!(outside.0.join("keep").is_file());
    }

    #[test]
    fn refuses_to_delete_under_a_directory_without_the_tag() {
        let s = Scratch::new();
        put(&s.0, "photos/2024/cat.jpg", b"meow");
        let cache = Cache::new(&s.0);
        assert!(cache.clear(None, true).unwrap_err().0.contains(TAG_FILE));
        assert!(cache.prune("2.9.0.1").is_err());
        assert!(s.0.join("photos/2024/cat.jpg").is_file());
        // Nothing there is nothing to refuse.
        assert!(
            Cache::new(s.0.join("none"))
                .clear(None, true)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn finds_the_platform_cache_directory() {
        // Absolute on the platform running the test, which is what `is_absolute`
        // asks of a path.
        let abs = |p: &str| {
            if cfg!(windows) {
                format!(r"C:\{p}")
            } else {
                format!("/{p}")
            }
        };
        let (home, local, xdg) = (abs("h"), abs("l"), abs("x"));
        let root = |os: &str, pairs: &[(&str, &str)]| {
            platform_root(os, |k| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| OsString::from(v))
            })
        };
        let usual = [("HOME", home.as_str()), ("LOCALAPPDATA", local.as_str())];
        assert_eq!(
            root("windows", &usual).unwrap(),
            Path::new(&local).join("dcs-terrain").join("cache")
        );
        assert_eq!(
            root("macos", &usual).unwrap(),
            Path::new(&home).join("Library/Caches/dcs-terrain")
        );
        assert_eq!(
            root("linux", &usual).unwrap(),
            Path::new(&home).join(".cache").join("dcs-terrain")
        );
        assert_eq!(
            root("linux", &[("HOME", &home), ("XDG_CACHE_HOME", &xdg)]).unwrap(),
            Path::new(&xdg).join("dcs-terrain")
        );
        assert_eq!(
            root("linux", &[("HOME", &home), ("XDG_CACHE_HOME", "rel")]).unwrap(),
            Path::new(&home).join(".cache").join("dcs-terrain")
        );
        assert_eq!(root("windows", &[]), None);
    }

    #[test]
    fn names_directories_safely() {
        assert_eq!(dir_name("GermanyCW"), "GermanyCW");
        assert_eq!(dir_name("2.9.30.28738"), "2.9.30.28738");
        assert_eq!(dir_name("a/b\\c:d"), "a_b_c_d");
        assert_eq!(dir_name(".."), "___");
        assert_eq!(dir_name(""), "_");
    }
}
