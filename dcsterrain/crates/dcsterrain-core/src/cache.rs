//! The cache beside the install: one directory per terrain and DCS build,
//! holding what is too slow to derive per query. Deleting it loses nothing
//! but time; the next query rebuilds what it needs.
//!
//! An entry is a raw file that opens with a header: magic, the entry's
//! format version, and its key, the build and a stat of each install file it
//! was derived from; a JSON entry carries the same as its first members. A
//! missing entry, or one whose key or version does not
//! match, is rebuilt, and only that entry. An entry is written under a
//! temporary name and renamed into place, so two builders at once leave one
//! valid copy.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::decode::install::{self, Located, Map};
use crate::extract::FileEntry;
use crate::source::Refusal;

/// The live validation sortie writes this; nothing can rebuild it, so
/// clearing the cache asks before deleting it.
pub const VALIDATION_FILE: &str = "validation.json";

/// Marks a directory as this tool's cache, so a mistyped `--cache` cannot
/// have `clear` delete somebody's files. Backup tools that honor
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
    /// Builds the payload from `files`, the install files of its roles with
    /// the class and version each header states, read for this build.
    /// `cache` is the terrain's, whose directory may still hold the entry
    /// being replaced, and through which an entry built from another gets it.
    fn build(&self, cache: &TerrainCache, files: &[FileEntry]) -> Result<Vec<u8>, Refusal>;
    /// Whether the entry is a JSON document, whose payload is a JSON value.
    /// A JSON entry cannot open with the binary header and stay JSON, so it
    /// is one object carrying the version, the key and the payload as its
    /// first three members, in that order, and a check reads only up to the
    /// payload.
    fn json(&self) -> bool {
        false
    }
    /// Whether the entry is kept across a DCS update that leaves its files
    /// alone, re-stamped with the new build rather than rebuilt (ADR 0002).
    fn survives_build(&self) -> bool {
        true
    }
}

/// Every kind of entry `index` builds.
pub fn entries() -> Vec<&'static dyn Entry> {
    use crate::source::install::{
        MESH_TOC, ROADS_BETWEENNESS, ROADS_GRAPH, SCENE_NODES, TABLES_JSON,
    };
    vec![
        &MESH_TOC,
        &SCENE_NODES,
        &TABLES_JSON,
        &ROADS_GRAPH,
        &ROADS_BETWEENNESS,
    ]
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

/// One terrain's directory, and what is in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub terrain: String,
    pub path: PathBuf,
    /// Each entry, by name, and its size in bytes; a directory's is the sum
    /// of the files under it.
    pub entries: Vec<(String, u64)>,
    /// The DCS builds the entries were made on, as their keys record them,
    /// sorted: one, unless some are still to be checked since an update.
    pub builds: Vec<String>,
}

impl Stored {
    pub fn size(&self) -> u64 {
        self.entries.iter().map(|(_, s)| s).sum()
    }

    fn at(terrain: String, path: PathBuf) -> Stored {
        let entries = entries_in(&path);
        let mut builds: Vec<String> = entries
            .iter()
            .filter_map(|(name, _)| read_head(&path.join(name)).ok())
            .filter_map(|h| build_of(&h.key).map(str::to_string))
            .collect();
        builds.sort();
        builds.dedup();
        Stored {
            terrain,
            path,
            entries,
            builds,
        }
    }
}

impl Cache {
    pub fn new(root: impl Into<PathBuf>) -> Cache {
        Cache { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory of `terrain`, whether or not it exists. It holds the
    /// entries of whichever build each was last checked on (ADR 0002).
    pub fn dir(&self, terrain: &str) -> PathBuf {
        self.root.join(dir_name(terrain))
    }

    /// The entries of one opened terrain.
    pub fn terrain(&self, install: &Path, map: &Map) -> TerrainCache {
        TerrainCache {
            cache: self.clone(),
            dir: self.dir(&map.theatre),
            install: install.to_path_buf(),
            map: map.clone(),
        }
    }

    /// What is stored for `terrain`, `None` when nothing is.
    pub fn stored(&self, terrain: &str) -> Option<Stored> {
        let path = self.dir(terrain);
        is_real_dir(&path).then(|| Stored::at(dir_name(terrain), path))
    }

    /// Every terrain stored, sorted. No cache directory is an empty list.
    pub fn list(&self) -> Result<Vec<Stored>, Refusal> {
        Ok(subdirs(&self.root)?
            .into_iter()
            .map(|t| {
                let path = self.root.join(&t);
                Stored::at(t, path)
            })
            .collect())
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

/// Whether `dir`, relative to `cwd` if it is relative, is or would be
/// created inside `install`. The install is DCS's own and is only ever read,
/// so a path that names it another way, by case, by a short name, through a
/// link or junction, or from the working directory, must still be caught:
/// both are resolved as the file system resolves them before they are
/// compared.
fn is_inside(dir: &Path, install: &Path, cwd: &Path) -> io::Result<bool> {
    let install = fs::canonicalize(install)?;
    let dir = resolve(&std::path::absolute(cwd.join(dir))?)?;
    // The same directory by its file system identity, which no spelling of
    // a path can disguise: a share that loops back to the install's drive,
    // a mapped drive, a case-folding volume, a bind mount of the install.
    // A bind mount of a directory inside the install is not caught: nothing
    // short of reading the mount table tells where its root came from.
    let target = file_id(&install)?;
    if dir
        .ancestors()
        .any(|a| file_id(a).is_ok_and(|i| i == target))
    {
        return Ok(true);
    }
    // Windows and macOS volumes ignore case unless set otherwise.
    let fold = |p: &Path| {
        if cfg!(any(windows, target_os = "macos")) {
            PathBuf::from(p.to_string_lossy().to_lowercase())
        } else {
            p.to_path_buf()
        }
    };
    Ok(fold(&dir).starts_with(fold(&install)))
}

/// The device and inode of an existing file or directory.
#[cfg(unix)]
fn file_id(path: &Path) -> io::Result<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).map(|m| (m.dev(), m.ino()))
}

/// The volume serial number and file index of an existing file or
/// directory, read through a handle that asks for no access.
#[cfg(windows)]
fn file_id(path: &Path) -> io::Result<(u64, u64)> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, GetFileInformationByHandle,
    };
    // A directory opens only with backup semantics.
    let f = File::options()
        .access_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: the handle is open for the call, and `info` is the structure
    // the call fills.
    if unsafe { GetFileInformationByHandle(f.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    Ok((u64::from(info.dwVolumeSerialNumber), index))
}

#[cfg(not(any(unix, windows)))]
fn file_id(_: &Path) -> io::Result<(u64, u64)> {
    Err(io::Error::other("no file identity on this platform"))
}

/// An absolute `path` with its longest existing part resolved through every
/// link and short name, and the part that does not exist yet appended. A
/// `..` in that part climbs a directory creating it would make, so it is
/// taken lexically, and the result is resolved again, since the climb may
/// end in an existing link.
fn resolve(path: &Path) -> io::Result<PathBuf> {
    let parts: Vec<Component> = path.components().collect();
    for n in (1..=parts.len()).rev() {
        let head: PathBuf = parts[..n].iter().collect();
        let mut base = match fs::canonicalize(&head) {
            Ok(base) => base,
            // Something is there that does not resolve, such as a dangling
            // link; creating a directory through it fails too, but where it
            // points is unknown. A second try tells it from a directory
            // another builder created between the two calls.
            Err(e) if fs::symlink_metadata(&head).is_ok() => {
                fs::canonicalize(&head).map_err(|_| e)?
            }
            Err(_) => continue,
        };
        let rest = &parts[n..];
        if !rest.contains(&Component::ParentDir) {
            base.extend(rest);
            return Ok(base);
        }
        for c in rest {
            match c {
                Component::ParentDir => {
                    base.pop();
                }
                c => base.push(c),
            }
        }
        return resolve(&base);
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("no part of {} exists", path.display()),
    ))
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

    /// The install the terrain is read from.
    pub fn install(&self) -> &Path {
        &self.install
    }

    /// The terrain whose entries these are.
    pub fn map(&self) -> &Map {
        &self.map
    }

    /// The part of a key a check compares: the build, then for each install
    /// file of the entry's roles its path, size and modification time, from
    /// a stat or a listing. Opens no install file, so checking an entry over
    /// 450 airfield files costs one listing (ADR 0001).
    fn checked(&self, entry: &dyn Entry) -> Result<(Vec<Located>, String), Refusal> {
        let files = self.map.stat(&self.install, entry.roles())?;
        let b = &self.map.build;
        let mut key = format!("build {} {}\n", b.version, b.timestamp);
        for f in &files {
            key.push_str(&format!("{}\t{}\t{}\n", f.path, f.size, f.modified));
        }
        Ok((files, key))
    }

    /// The key an entry built from `files` carries: each file's line of the
    /// checked part followed by its class, version and payload size, read
    /// from its header now. A header that changes while the file's size and
    /// time do not is not seen until the build changes.
    fn built_key(&self, files: &[Located]) -> Result<(Vec<FileEntry>, String), Refusal> {
        let classified = install::classify(&self.install, files)?;
        let b = &self.map.build;
        let mut key = format!("build {} {}\n", b.version, b.timestamp);
        let dash = || "-".to_string();
        for (f, c) in files.iter().zip(&classified) {
            key.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\n",
                f.path,
                f.size,
                f.modified,
                c.class.clone().unwrap_or_else(dash),
                c.version.map_or_else(dash, |v| v.to_string()),
                c.payload_size.map_or_else(dash, |p| p.to_string()),
            ));
        }
        Ok((classified, key))
    }

    /// The key an entry built now would carry: the build, then for each
    /// install file of the entry's roles its path, size, modification time,
    /// class, version and payload size. Reads each file's header.
    pub fn key(&self, entry: &dyn Entry) -> Result<String, Refusal> {
        let (files, _) = self.checked(entry)?;
        Ok(self.built_key(&files)?.1)
    }

    /// Whether `key`, carried by an entry built earlier, is current for
    /// `entry`, from metadata alone.
    pub fn matches(&self, entry: &dyn Entry, key: &str) -> Result<bool, Refusal> {
        Ok(checked_part(key) == self.checked(entry)?.1)
    }

    pub fn state(&self, entry: &dyn Entry) -> Result<State, Refusal> {
        let (_, checked) = self.checked(entry)?;
        Ok(match read_head(&self.dir.join(entry.name())) {
            Ok(head) => judge(&head, entry.version(), &checked),
            Err(e) if e.kind() == io::ErrorKind::NotFound => State::Missing,
            Err(e) => State::Stale(format!("its header cannot be read: {e}")),
        })
    }

    /// The entry, built first if it is missing or stale. Only a build reads
    /// the headers of the files it derives from, so a file no decoder reads
    /// refuses the entries of its role and no other.
    pub fn get(&self, entry: &dyn Entry) -> Result<Opened, Refusal> {
        let path = self.dir.join(entry.name());
        let (files, checked) = self.checked(entry)?;
        if let Ok(head) = read_head(&path) {
            if judge(&head, entry.version(), &checked) == State::Current {
                return Ok(head.opened(path, false));
            }
            if entry.survives_build() && judge_but_build(&head, entry.version(), &checked) {
                return self.restamp(entry, head, &files, &checked);
            }
        }
        self.build(entry, files, &checked, false)
    }

    /// An entry made on another build whose files' metadata still match:
    /// their headers are read again, and when they match too the payload is
    /// written under the installed build's key, else the entry is rebuilt.
    /// An update is the one time a header changed under an unchanged size and
    /// time is looked for (ADR 0001).
    fn restamp(
        &self,
        entry: &dyn Entry,
        head: Head,
        files: &[Located],
        checked: &str,
    ) -> Result<Opened, Refusal> {
        let path = self.dir.join(entry.name());
        let (_, key) = self.built_key(files)?;
        if but_build(&key) != but_build(&head.key) {
            return self.build(entry, files.to_vec(), checked, false);
        }
        self.make_dir()?;
        let json = entry.json();
        let (open, close) = framing(entry.version(), &key, head.len, json);
        let mut src = &head.file;
        src.seek(SeekFrom::Start(head.offset))
            .map_err(|e| cannot("read", &path, e))?;
        self.write(
            entry.name(),
            |f| {
                f.write_all(&open)?;
                let n = io::copy(&mut src.take(head.len), f)?;
                if n != head.len {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "the entry ended inside its payload",
                    ));
                }
                f.write_all(close)
            },
            |dest| {
                read_head(dest).is_ok_and(|h| judge(&h, entry.version(), checked) == State::Current)
            },
        )?;
        let head = read_head(&path).map_err(|e| cannot("read", &path, e))?;
        Ok(head.opened(path, false))
    }

    /// The entry built now, whether or not the one there is current: for a
    /// build whose inputs the key does not cover, such as the hook's tables.
    pub fn rebuild(&self, entry: &dyn Entry) -> Result<Opened, Refusal> {
        let (files, checked) = self.checked(entry)?;
        self.build(entry, files, &checked, true)
    }

    fn build(
        &self,
        entry: &dyn Entry,
        files: Vec<Located>,
        checked: &str,
        forced: bool,
    ) -> Result<Opened, Refusal> {
        let path = self.dir.join(entry.name());
        // Refused before the build, which can take minutes, not after it.
        self.make_dir()?;
        let (classified, key) = self.built_key(&files)?;
        let payload = entry.build(self, &classified)?;
        let (open, close) = framing(entry.version(), &key, payload.len() as u64, entry.json());
        // A forced build must not settle for the entry another builder put
        // there, which lacks what forced it.
        let fill = |f: &mut File| {
            f.write_all(&open)?;
            f.write_all(&payload)?;
            f.write_all(close)
        };
        self.write(entry.name(), fill, |dest| {
            !forced
                && read_head(dest)
                    .is_ok_and(|h| judge(&h, entry.version(), checked) == State::Current)
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
        let cwd = std::env::current_dir()
            .map_err(|e| Refusal(format!("cannot read the working directory: {e}")))?;
        let refuse = |why: String| {
            Refusal(format!(
                "the cache {} {why}, which is never written to",
                self.cache.root().display(),
            ))
        };
        match is_inside(&self.dir, &self.install, &cwd) {
            Ok(false) => {}
            Ok(true) => {
                return Err(refuse(format!(
                    "is inside the install {}",
                    self.install.display()
                )));
            }
            Err(e) => {
                return Err(refuse(format!(
                    "cannot be told apart from the install {}: {e}",
                    self.install.display()
                )));
            }
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
    /// already there is one another builder just put in place. The caller
    /// has made the directory, and so has refused one inside the install.
    fn write(
        &self,
        name: &str,
        fill: impl FnOnce(&mut File) -> io::Result<()>,
        current: impl Fn(&Path) -> bool,
    ) -> Result<(), Refusal> {
        static N: AtomicU32 = AtomicU32::new(0);
        let dest = self.dir.join(name);
        let tmp = self.dir.join(format!(
            ".{name}.{}.{}.tmp",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let written = File::create(&tmp).and_then(|mut f| {
            fill(&mut f)?;
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

/// How a JSON entry opens, before its version.
const JSON_OPEN: &[u8] = b"{\"version\":";

/// What an entry holds before its payload, and after it.
fn framing(version: u32, key: &str, payload_len: u64, json: bool) -> (Vec<u8>, &'static [u8]) {
    if json {
        let key = serde_json::to_string(key).expect("a string serializes");
        let open = format!("{{\"version\":{version},\"key\":{key},\"payload\":").into_bytes();
        return (open, b"}");
    }
    let mut b = Vec::with_capacity(24 + key.len());
    b.extend_from_slice(MAGIC);
    b.extend_from_slice(&version.to_le_bytes());
    b.extend_from_slice(&(key.len() as u32).to_le_bytes());
    b.extend_from_slice(key.as_bytes());
    b.extend_from_slice(&payload_len.to_le_bytes());
    (b, b"")
}

#[cfg(test)]
fn encode(version: u32, key: &str, payload: &[u8], json: bool) -> Vec<u8> {
    let (mut b, close) = framing(version, key, payload.len() as u64, json);
    b.extend_from_slice(payload);
    b.extend_from_slice(close);
    b
}

/// A JSON entry's head: `{"version":N,"key":"...","payload":`, read only as
/// far as the payload, which may be a megabyte. Its payload ends one byte
/// before the file does, at the closing brace.
fn read_json_head(mut f: File, file_len: u64) -> io::Result<Head> {
    let bad = |why: &str| io::Error::new(io::ErrorKind::InvalidData, why.to_string());
    let mut b: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 16 << 10];
    let (version, key, offset) = loop {
        if let Some(head) = json_head(&b).map_err(bad)? {
            break head;
        }
        // Each byte of a key escapes to at most six.
        if b.len() as u64 > 64 + 6 * MAX_KEY {
            return Err(bad("its key is too long"));
        }
        let n = f.read(&mut chunk)?;
        if n == 0 {
            return Err(bad("its key runs past the end"));
        }
        b.extend_from_slice(&chunk[..n]);
    };
    if key.len() as u64 > MAX_KEY {
        return Err(bad("its key is too long"));
    }
    let offset = offset as u64;
    // A payload is one JSON value, so never empty; one cut short inside it
    // is seen only when it is parsed, as a raw entry's would be.
    if offset + 1 >= file_len {
        return Err(bad("it has no payload after its key"));
    }
    let mut last = [0u8; 1];
    f.seek(SeekFrom::Start(file_len - 1))?;
    f.read_exact(&mut last)?;
    if last != *b"}" {
        return Err(bad("it does not end where its payload does"));
    }
    Ok(Head {
        file: f,
        version,
        key,
        offset,
        len: file_len - 1 - offset,
        // The closing brace is not the payload's, so judge the rest.
        file_len: file_len - 1,
    })
}

/// The version, key and payload offset of a JSON entry whose first bytes
/// are `b`; `None` while `b` stops short of the payload.
fn json_head(b: &[u8]) -> Result<Option<(u32, String, usize)>, &'static str> {
    // The position after `lit` at `at`, `None` when `b` ends inside it.
    let lit = |at: usize, lit: &[u8]| -> Result<Option<usize>, &'static str> {
        let have = &b[at.min(b.len())..];
        if have.len() < lit.len() {
            return if lit.starts_with(have) {
                Ok(None)
            } else {
                Err("not a cache entry")
            };
        }
        if have.starts_with(lit) {
            Ok(Some(at + lit.len()))
        } else {
            Err("not a cache entry")
        }
    };
    let Some(at) = lit(0, JSON_OPEN)? else {
        return Ok(None);
    };
    let digits = b[at..].iter().take_while(|c| c.is_ascii_digit()).count();
    if at + digits == b.len() {
        return Ok(None);
    }
    let version = std::str::from_utf8(&b[at..at + digits])
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .ok_or("its version is not a number")?;
    let Some(q) = lit(at + digits, b",\"key\":\"")? else {
        return Ok(None);
    };
    // The key's closing quote is the first no backslash escapes.
    let mut i = q;
    let end = loop {
        match b.get(i) {
            None => return Ok(None),
            Some(b'\\') => i += 2,
            Some(b'"') => break i,
            Some(_) => i += 1,
        }
    };
    let key = serde_json::from_slice(&b[q - 1..=end]).map_err(|_| "its key is not a string")?;
    let Some(offset) = lit(end + 1, b",\"payload\":")? else {
        return Ok(None);
    };
    Ok(Some((version, key, offset)))
}

fn read_head(path: &Path) -> io::Result<Head> {
    let bad = |why: &str| io::Error::new(io::ErrorKind::InvalidData, why.to_string());
    let mut f = File::open(path)?;
    let file_len = f.metadata()?.len();
    let mut magic = [0u8; 8];
    f.read_exact(&mut magic)?;
    if magic.starts_with(&JSON_OPEN[..8]) {
        f.seek(SeekFrom::Start(0))?;
        return read_json_head(f, file_len);
    }
    if &magic != MAGIC {
        return Err(bad("not a cache entry"));
    }
    let mut fixed = [0u8; 8];
    f.read_exact(&mut fixed)?;
    let version = u32::from_le_bytes(fixed[..4].try_into().expect("4 bytes"));
    let key_len = u64::from(u32::from_le_bytes(fixed[4..8].try_into().expect("4 bytes")));
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

/// What a check compares of a stored key: each file's line without the
/// fields its header gave when the entry was built.
fn checked_part(key: &str) -> String {
    let mut out = String::new();
    for line in key.lines() {
        let cut = line
            .match_indices('\t')
            .nth(2)
            .map_or(line.len(), |(i, _)| i);
        out.push_str(&line[..cut]);
        out.push('\n');
    }
    out
}

/// The DCS version a key's first line names.
fn build_of(key: &str) -> Option<&str> {
    key.lines()
        .next()?
        .strip_prefix("build ")?
        .split(' ')
        .next()
}

/// A key without its first line, the build's.
fn but_build(key: &str) -> &str {
    key.split_once('\n').map_or("", |(_, rest)| rest)
}

/// Whether an entry would be current but for the build its key names.
fn judge_but_build(head: &Head, version: u32, checked: &str) -> bool {
    head.version == version
        && head.offset.checked_add(head.len) == Some(head.file_len)
        && but_build(&checked_part(&head.key)) == but_build(checked)
}

/// Whether an entry is current, given the checked part of the key an
/// entry built now would carry.
fn judge(head: &Head, version: u32, checked: &str) -> State {
    if head.version != version {
        State::Stale(format!("its version is {}, not {version}", head.version))
    } else if head.offset.checked_add(head.len) != Some(head.file_len) {
        State::Stale("its length does not match its header".into())
    } else if checked_part(&head.key) != checked {
        State::Stale("an install file or the build changed".into())
    } else {
        State::Current
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::container;
    use crate::scratch::Scratch;
    use std::sync::atomic::AtomicUsize;
    use std::time::{Duration, SystemTime};

    /// A test entry over the files of one role, counting its builds.
    struct Counted {
        name: &'static str,
        version: u32,
        roles: &'static [&'static str],
        builds: AtomicUsize,
        json: bool,
        survives: bool,
    }

    impl Counted {
        const fn new(name: &'static str, roles: &'static [&'static str]) -> Counted {
            Counted {
                name,
                version: 1,
                roles,
                builds: AtomicUsize::new(0),
                json: false,
                survives: true,
            }
        }
        /// The same as a JSON entry, whose payload records the directory
        /// it was built for.
        const fn json(name: &'static str, roles: &'static [&'static str]) -> Counted {
            let mut c = Counted::new(name, roles);
            c.json = true;
            c
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
        fn build(&self, cache: &TerrainCache, _: &[FileEntry]) -> Result<Vec<u8>, Refusal> {
            let (map, dir) = (cache.map(), cache.dir());
            self.builds.fetch_add(1, Ordering::SeqCst);
            if self.json {
                let v = serde_json::json!({"of": map.theatre, "dir": dir});
                return Ok(v.to_string().into_bytes());
            }
            Ok(format!("{} of {}", self.name, map.theatre).into_bytes())
        }
        fn json(&self) -> bool {
            self.json
        }
        fn survives_build(&self) -> bool {
            self.survives
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

        // The payload size alone, the file's size and time unchanged, is
        // not seen: a check reads no header.
        let mut moved = container::build("landscape5::Surface5File", 5, 101);
        moved.push(0);
        put(&f.install.0, SURFACE, &moved);
        f.touch(SURFACE, 1_000_000);
        assert_eq!(
            fs::metadata(f.install.0.join(SURFACE)).unwrap().len(),
            grown.len() as u64
        );
        assert_eq!(both(&t), (2, 1));
        f.touch(SURFACE, 1_000_001);
        assert_eq!(both(&t), (3, 1));
        assert!(
            t.key(&mesh)
                .unwrap()
                .contains("\tlandscape5::Surface5File\t5\t101\n")
        );

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

        // The build, which every entry's key carries, re-stamps each entry
        // whose files are unchanged rather than rebuilding it.
        build_file(&f.install.0, "20000202-000000");
        let t = f.terrain();
        assert_eq!(both(&t), (4, 2));
        let stored = read_head(&t.dir().join("mesh.toc")).unwrap().key;
        assert!(
            stored.starts_with(
                "build 2.9.0.1 20000202-000000
"
            ),
            "{stored}"
        );
    }

    #[test]
    fn an_update_re_stamps_an_entry_whose_files_survive_it_and_rebuilds_the_rest() {
        let f = Fixture::new();
        let mesh = Counted::new("mesh.toc", &["surface"]);
        let tables = Counted {
            survives: false,
            ..Counted::json("tables.json", &["surface"])
        };
        let doc = Counted::json("doc.json", &["roads"]);
        f.touch(SURFACE, 1_000_000);
        let t = f.terrain();
        for e in [&mesh as &dyn Entry, &tables, &doc] {
            t.get(e).unwrap();
        }
        let before = fs::read(t.dir().join("doc.json")).unwrap();

        build_file(&f.install.0, "20000202-000000");
        let t = f.terrain();
        let got = t.get(&mesh).unwrap();
        assert!(!got.built);
        assert_eq!(got.payload().unwrap(), b"mesh.toc of Caucasus");
        assert_eq!(t.state(&mesh).unwrap(), State::Current);
        assert!(
            t.get(&tables).unwrap().built,
            "the hook's tables are a build's own"
        );
        let after = t.get(&doc).unwrap();
        assert!(!after.built);
        assert_eq!(t.state(&doc).unwrap(), State::Current);
        let after = fs::read(&after.path).unwrap();
        assert_ne!(after, before);
        let payload = |b: &[u8]| -> serde_json::Value {
            serde_json::from_slice::<serde_json::Value>(b).unwrap()["payload"].clone()
        };
        assert_eq!(payload(&after), payload(&before));
        assert_eq!((mesh.builds(), tables.builds(), doc.builds()), (1, 2, 1));
        let names: Vec<String> = entries_in(t.dir()).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["doc.json", "mesh.toc", "tables.json"]);

        // A header changed under an unchanged size and time is seen at an
        // update, which reads the headers again.
        let mut moved = container::build("landscape5::Surface5File", 5, 101);
        moved.truncate(fs::metadata(f.install.0.join(SURFACE)).unwrap().len() as usize);
        put(&f.install.0, SURFACE, &moved);
        f.touch(SURFACE, 1_000_000);
        let t = f.terrain();
        assert!(
            !t.get(&mesh).unwrap().built,
            "within a build it goes unseen"
        );
        build_file(&f.install.0, "20000303-000000");
        let t = f.terrain();
        assert!(t.get(&mesh).unwrap().built);
        assert_eq!(mesh.builds(), 2);
    }

    const TAXIWAYS: &str = "Mods/terrains/Caucasus/AirfieldsTaxiways";

    /// Adds `n` airfields, each an `.rn4` and an `.rn5`.
    fn add_airfields(root: &Path, n: usize) {
        for i in 0..n {
            put(
                root,
                &format!("{TAXIWAYS}/F{i}.rn4"),
                &container::build("landscape4::lRoadNetwork", 5, 0),
            );
            put(
                root,
                &format!("{TAXIWAYS}/F{i}.rn5"),
                &container::build("t5::rn5file", 0, 0),
            );
        }
    }

    #[test]
    fn a_check_reads_no_header_and_a_build_reads_each_once() {
        let f = Fixture::new();
        add_airfields(&f.install.0, 20);
        let fields = Counted::new("tables.bin", &["airfield_rn4", "airfield_rn5"]);
        container::READS.set(0);
        let t = f.terrain();
        assert_eq!(container::READS.get(), 1, "opening reads the surface alone");

        container::READS.set(0);
        assert!(t.get(&fields).unwrap().built);
        assert_eq!(container::READS.get(), 40);
        let key = t.key(&fields).unwrap();
        assert!(key.contains("/F7.rn5\t"), "{key}");
        assert!(key.contains("\tt5::rn5file\t0\t-\n"), "{key}");

        container::READS.set(0);
        assert!(!t.get(&fields).unwrap().built);
        assert_eq!(t.state(&fields).unwrap(), State::Current);
        assert!(t.matches(&fields, &key).unwrap());
        assert_eq!(container::READS.get(), 0);
    }

    #[test]
    fn a_file_added_to_a_role_directory_rebuilds_only_its_entries() {
        let f = Fixture::new();
        add_airfields(&f.install.0, 2);
        let mesh = Counted::new("mesh.toc", &["surface"]);
        let fields = Counted::new("tables.bin", &["airfield_rn4", "airfield_rn5"]);
        let t = f.terrain();
        t.get(&mesh).unwrap();
        t.get(&fields).unwrap();
        add_airfields(&f.install.0, 3);
        assert!(matches!(t.state(&fields).unwrap(), State::Stale(_)));
        assert!(!t.get(&mesh).unwrap().built);
        assert!(t.get(&fields).unwrap().built);
        assert_eq!((mesh.builds(), fields.builds()), (1, 2));
    }

    #[test]
    fn an_unknown_taxiway_refuses_its_entries_and_no_other() {
        let f = Fixture::new();
        add_airfields(&f.install.0, 2);
        let bad = format!("{TAXIWAYS}/F1.rn5");
        put(&f.install.0, &bad, &container::build("t5::rn5file", 99, 0));
        let mesh = Counted::new("mesh.toc", &["surface"]);
        let fields = Counted::new("tables.bin", &["airfield_rn4", "airfield_rn5"]);
        let t = f.terrain();
        let e = t.get(&fields).unwrap_err();
        assert!(
            e.0.starts_with(&format!("{bad}: t5::rn5file version 99")),
            "{e}"
        );
        assert_eq!(fields.builds(), 0);
        assert!(t.get(&mesh).unwrap().built);
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
    fn index_lays_out_a_directory_per_terrain_and_tags_the_root() {
        let f = Fixture::new();
        let mesh = Counted::new("mesh.toc", &["surface"]);
        let t = f.terrain();
        assert_eq!(t.index(&[&mesh]).unwrap(), [("mesh.toc".to_string(), true)]);
        assert_eq!(
            t.index(&[&mesh]).unwrap(),
            [("mesh.toc".to_string(), false)]
        );
        assert_eq!(t.dir(), f.cache.0.join("Caucasus"));
        assert!(f.cache.0.join(TAG_FILE).is_file());

        let cache = Cache::new(&f.cache.0);
        let stored = cache.stored("Caucasus").unwrap();
        assert_eq!(stored.entries.len(), 1);
        assert_eq!(stored.builds, ["2.9.0.1"]);
        assert_eq!(
            stored.size(),
            fs::metadata(t.dir().join("mesh.toc")).unwrap().len()
        );
        assert!(cache.stored("Nevada").is_none());
    }

    #[test]
    fn clear_can_keep_validation() {
        let f = Fixture::new();
        let mesh = Counted::new("mesh.toc", &["surface"]);
        f.terrain().index(&[&mesh]).unwrap();
        let cache = Cache::new(&f.cache.0);
        put(&f.cache.0, "Nevada/validation.json", b"{}");
        put(&f.cache.0, "Caucasus/validation.json", b"{}");
        put(&f.cache.0, "Caucasus/horizon/0_0", b"tile");
        // A directory of the layout before ADR 0002, which `clear` removes.
        put(&f.cache.0, "Caucasus/2.8.0.1/mesh.toc", b"old");
        assert_eq!(cache.list().unwrap().len(), 2);

        assert_eq!(cache.validation_files(None).unwrap().len(), 2);
        cache.clear(Some("Caucasus"), false).unwrap();
        let left = cache.stored("Caucasus").unwrap();
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

    /// Asserts a cache at `root` is refused for the fixture's install and
    /// that nothing was made inside the install.
    fn refused(f: &Fixture, root: &Path) {
        let before = entries_in(&f.install.0);
        let map = install::open(&f.install.0, "Caucasus").unwrap();
        let mesh = Counted::new("mesh.toc", &["surface"]);
        let err = Cache::new(root)
            .terrain(&f.install.0, &map)
            .index(&[&mesh])
            .unwrap_err()
            .0;
        assert!(err.contains("inside the install"), "{err}");
        assert_eq!(entries_in(&f.install.0), before, "the install is untouched");
    }

    #[test]
    fn refuses_a_cache_inside_the_install_before_building() {
        let f = Fixture::new();
        let map = install::open(&f.install.0, "Caucasus").unwrap();
        let mesh = Counted::new("mesh.toc", &["surface"]);
        let inside = Cache::new(f.install.0.join("cache")).terrain(&f.install.0, &map);
        let err = inside.get(&mesh).unwrap_err().0;
        assert!(err.contains("inside the install"), "{err}");
        assert_eq!(mesh.builds(), 0, "nothing was built only to be refused");
        assert!(!f.install.0.join("cache").exists());
    }

    #[test]
    fn refuses_the_install_spelled_in_another_case_where_the_volume_ignores_it() {
        let f = Fixture::new();
        let upper = PathBuf::from(f.install.0.to_string_lossy().to_uppercase());
        if upper == f.install.0 || !upper.is_dir() {
            eprintln!("this volume tells case apart; skipped");
            return;
        }
        refused(&f, &upper.join("cache"));
    }

    #[test]
    fn refuses_a_relative_cache_from_the_install() {
        let f = Fixture::new();
        let dir = Path::new("cache").join("Caucasus");
        assert!(is_inside(&dir, &f.install.0, &f.install.0).unwrap());
        let elsewhere = Scratch::new();
        fs::create_dir_all(&elsewhere.0).unwrap();
        assert!(!is_inside(&dir, &f.install.0, &elsewhere.0).unwrap());
    }

    #[test]
    fn refuses_a_cache_that_climbs_into_the_install_through_a_new_directory() {
        let f = Fixture::new();
        fs::create_dir_all(&f.cache.0).unwrap();
        let name = f.install.0.file_name().unwrap();
        let climb = f.cache.0.join("new").join("..").join("..").join(name);
        refused(&f, &climb.join("cache"));
    }

    #[test]
    fn allows_a_cache_beside_the_install_whose_name_extends_it() {
        let f = Fixture::new();
        let mut beside = f.install.0.clone().into_os_string();
        beside.push("-cache");
        let beside = PathBuf::from(beside);
        let dir = beside.join("Caucasus");
        let cwd = std::env::temp_dir();
        assert!(!is_inside(&dir, &f.install.0, &cwd).unwrap());
    }

    #[test]
    fn refuses_a_cache_through_a_dangling_link() {
        let f = Fixture::new();
        fs::create_dir_all(&f.cache.0).unwrap();
        let link = f.cache.0.join("gone");
        if !link_dir(&f.cache.0.join("never-made"), &link) {
            return;
        }
        let err = is_inside(&link.join("cache"), &f.install.0, &f.cache.0);
        assert!(err.is_err(), "{err:?}");
    }

    #[test]
    fn refuses_a_cache_through_a_link_into_the_install() {
        let f = Fixture::new();
        fs::create_dir_all(&f.cache.0).unwrap();
        let link = f.cache.0.join("dcs");
        if !link_dir(&f.install.0, &link) {
            return;
        }
        refused(&f, &link.join("cache"));
    }

    /// Links `link` to the directory `target`: a symlink on Unix, a junction
    /// on Windows, which needs no privilege. False, with a note, where the
    /// link cannot be made.
    fn link_dir(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(target, link).is_ok();
        #[cfg(windows)]
        let made = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .is_ok_and(|o| o.status.success());
        if !made {
            eprintln!(
                "cannot link {} to {}; skipped",
                link.display(),
                target.display()
            );
        }
        made
    }

    #[cfg(windows)]
    #[test]
    fn refuses_the_install_by_its_short_name() {
        use std::os::windows::process::CommandExt;
        let f = Fixture::new();
        let out = std::process::Command::new("cmd")
            .raw_arg(format!(
                "/c for %I in (\"{}\") do @echo %~sI",
                f.install.0.display()
            ))
            .output()
            .unwrap();
        let short = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
        if short.file_name() == f.install.0.file_name() || !short.is_dir() {
            eprintln!("this volume makes no short names; skipped");
            return;
        }
        refused(&f, &short.join("cache"));
    }

    #[cfg(windows)]
    #[test]
    fn refuses_the_install_through_a_share_that_loops_back_to_it() {
        let f = Fixture::new();
        let local = f.install.0.to_string_lossy().into_owned();
        let Some((drive, rest)) = local.split_once(":\\") else {
            panic!("{local} has no drive letter");
        };
        let share = PathBuf::from(format!(r"\\localhost\{drive}$\{rest}"));
        if !share.is_dir() {
            eprintln!(
                "the administrative share {} is not reachable; skipped",
                share.display()
            );
            return;
        }
        refused(&f, &share.join("cache"));
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

        let mut huge = encode(1, &t.key(&mesh).unwrap(), b"", false);
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
    fn a_json_entry_is_json_carrying_its_key_and_is_checked_like_any_other() {
        let f = Fixture::new();
        let tables = Counted::json("tables.json", &["roads"]);
        let t = f.terrain();
        let got = t.get(&tables).unwrap();
        let doc: serde_json::Value = serde_json::from_slice(&fs::read(&got.path).unwrap()).unwrap();
        assert_eq!(doc["version"], 1);
        assert_eq!(doc["key"], t.key(&tables).unwrap());
        assert!(t.matches(&tables, doc["key"].as_str().unwrap()).unwrap());
        assert_eq!(doc["payload"]["of"], "Caucasus");
        assert_eq!(doc["payload"]["dir"], t.dir().to_str().unwrap());
        let payload: serde_json::Value = serde_json::from_slice(&got.payload().unwrap()).unwrap();
        assert_eq!(payload, doc["payload"]);

        assert_eq!(t.state(&tables).unwrap(), State::Current);
        assert!(!t.get(&tables).unwrap().built);
        f.touch(ROADS, 7);
        assert!(matches!(t.state(&tables).unwrap(), State::Stale(_)));
        assert!(t.get(&tables).unwrap().built);
        assert_eq!(tables.builds(), 2);
        // A rebuild builds whether or not the entry is current.
        assert!(t.rebuild(&tables).unwrap().built);
        assert_eq!(tables.builds(), 3);
    }

    #[test]
    fn a_hostile_json_head_is_stale_and_sizes_nothing() {
        let f = Fixture::new();
        let tables = Counted::json("tables.json", &["roads"]);
        let t = f.terrain();
        let path = t.get(&tables).unwrap().path;
        let key = serde_json::to_string(&t.key(&tables).unwrap()).unwrap();
        let bad = [
            r#"{"version":1,"key":"#.to_string(),
            r#"{"version":x,"key":"","payload":{}}"#.into(),
            format!(r#"{{"version":1,"key":{key},"data":{{}}}}"#),
            format!(r#"{{"version":1,"key":{key},"payload":}}"#),
            format!(
                r#"{{"version":1,"key":"{}","payload":{{}}}}"#,
                "\\\"".repeat(1 << 20)
            ),
            format!(r#"{{"key":{key},"version":1,"payload":{{}}}}"#),
        ];
        for b in &bad {
            fs::write(&path, b).unwrap();
            assert!(
                matches!(t.state(&tables).unwrap(), State::Stale(_)),
                "{}",
                &b[..b.len().min(60)]
            );
        }
        // An escaped quote inside the key does not end it.
        let quoted = encode(1, "a\"b", b"{}", true);
        fs::write(&path, &quoted).unwrap();
        let head = read_head(&path).unwrap();
        assert_eq!((head.key.as_str(), head.len), ("a\"b", 2));
        assert!(t.get(&tables).unwrap().built);
    }

    #[test]
    fn an_opened_entry_reads_the_file_its_header_came_from() {
        let f = Fixture::new();
        let mesh = Counted::new("mesh.toc", &["surface"]);
        let t = f.terrain();
        let held = t.get(&mesh).unwrap();
        // Another builder replaces it, with a longer key and so a later
        // payload.
        let other = encode(1, &format!("{}x\n", t.key(&mesh).unwrap()), b"other", false);
        let tmp = t.dir().join(".mesh.toc.test.tmp");
        fs::write(&tmp, other).unwrap();
        fs::rename(&tmp, &held.path).unwrap();
        assert_eq!(held.payload().unwrap(), b"mesh.toc of Caucasus");
    }

    #[cfg(unix)]
    #[test]
    fn clear_never_follows_a_link_out_of_the_cache() {
        let f = Fixture::new();
        let mesh = Counted::new("mesh.toc", &["surface"]);
        f.terrain().index(&[&mesh]).unwrap();
        let outside = Scratch::new();
        put(&outside.0, "2.8.0.1/keep", b"mine");
        put(&outside.0, "keep", b"mine");
        std::os::unix::fs::symlink(&outside.0, f.cache.0.join("Linked")).unwrap();
        std::os::unix::fs::symlink(&outside.0, f.cache.0.join("Caucasus/horizon")).unwrap();
        let cache = Cache::new(&f.cache.0);
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
