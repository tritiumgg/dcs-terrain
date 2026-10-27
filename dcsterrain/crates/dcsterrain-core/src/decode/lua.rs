//! Runs a map's Lua files in a sandbox and reads back the tables they set.
//!
//! The files are data written as Lua 5.1, so Lua 5.1 itself reads them. Each
//! runs in an environment of its own holding the pure parts of the base
//! library and `string`, `table` and `math`, and nothing that reaches a file,
//! the system or the rest of the state. `dofile` runs a script from the
//! install in the same environment, which is how ED's shared scripts give the
//! files their constants. `require("i_18n")` gives a `translate` that returns
//! its argument, and any other module is an empty table. A run is cut off
//! after a fixed number of instructions and a fixed amount of memory, and what
//! would escape those, `pcall` and pattern matching, is left out.
//!
//! A name nothing assigned reads as nil, as it does in the game, and is
//! reported: a beacon whose type names an undefined constant has no type.

pub mod tables;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mlua::chunk::ChunkMode;
use mlua::{HookTriggers, Lua, LuaOptions, StdLib, VmState};

use super::install;

/// A Lua value, as far as data needs one. Strings are kept as text: bytes
/// that are not UTF-8 are replaced, since every string these files hold is a
/// name or an id. A function or other value that is not data reads as absent.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Num(f64),
    Str(String),
    Table(Table),
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum Key {
    Bool(bool),
    Num(f64),
    Str(String),
}

/// A table, its entries in key order: Lua keeps none of its own.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Table {
    pub entries: Vec<(Key, Value)>,
}

impl Table {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.entries.iter().find_map(|(k, v)| match k {
            Key::Str(s) if s == name => Some(v),
            _ => None,
        })
    }

    pub fn index(&self, i: f64) -> Option<&Value> {
        self.entries.iter().find_map(|(k, v)| match k {
            Key::Num(n) if *n == i => Some(v),
            _ => None,
        })
    }

    pub fn str(&self, name: &str) -> Option<&str> {
        match self.get(name)? {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn num(&self, name: &str) -> Option<f64> {
        match self.get(name)? {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn table(&self, name: &str) -> Option<&Table> {
        match self.get(name)? {
            Value::Table(t) => Some(t),
            _ => None,
        }
    }

    /// The values of a list keyed by consecutive integers from 0 or from 1,
    /// in key order; an empty table is an empty list. `None` when the keys
    /// are anything else.
    pub fn list(&self) -> Option<Vec<&Value>> {
        let from = match self.entries.first() {
            None => return Some(Vec::new()),
            Some((Key::Num(n), _)) if *n == 0.0 || *n == 1.0 => *n,
            Some(_) => return None,
        };
        let consecutive = self
            .entries
            .iter()
            .zip(0..)
            .all(|((k, _), i)| *k == Key::Num(from + f64::from(i)));
        consecutive.then(|| self.entries.iter().map(|(_, v)| v).collect())
    }
}

/// Why a file cannot be read. Lua's own message, which names the file and
/// line, or the sandbox's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<mlua::Error> for Error {
    fn from(e: mlua::Error) -> Self {
        // A Rust callback's error arrives wrapped with a traceback; the cause
        // is the message.
        let mut e = &e;
        while let mlua::Error::CallbackError { cause, .. } = e {
            e = cause;
        }
        Error(e.to_string())
    }
}

/// What a file set in its environment, the scripts it ran, and the names it
/// read that nothing assigned.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Chunk {
    pub globals: BTreeMap<String, Value>,
    /// Each script a `dofile` ran, relative to the install as the disk
    /// spells it, with its size, in the order first run.
    pub included: Vec<(String, u64)>,
    /// Names read that nothing assigned, and `require` of each module the
    /// sandbox stands in for, as `require("utils")`.
    pub unresolved: BTreeSet<String>,
}

impl Chunk {
    pub fn table(&self, name: &str) -> Option<&Table> {
        match self.globals.get(name)? {
            Value::Table(t) => Some(t),
            _ => None,
        }
    }
}

/// How many instructions a file and the scripts it runs may take. The
/// largest map's beacons take a few hundred thousand; a loop that never ends
/// takes all of them.
const INSTRUCTIONS: u64 = 200_000_000;
const HOOK_EVERY: u32 = 10_000;

/// How much memory one file's state may hold. Syria's towns take a few MB.
const MEMORY: usize = 512 << 20;

/// The parts of the base library a data file may call: none reaches a file
/// or another environment. `pcall` is left out because it would catch the
/// instruction cap's error and carry on; no ED file calls it.
const BASE: [&str; 14] = [
    "assert",
    "error",
    "ipairs",
    "next",
    "pairs",
    "rawequal",
    "rawget",
    "rawset",
    "select",
    "setmetatable",
    "getmetatable",
    "tonumber",
    "tostring",
    "type",
];
const LIBRARIES: [&str; 4] = ["string", "table", "math", "unpack"];

/// `string` functions taken out of the state: matching a pattern runs in C,
/// where neither cap sees it, and can take exponential time; `dump` yields
/// bytecode. No ED file calls any of them.
const UNSAFE_STRING: [&str; 5] = ["find", "match", "gmatch", "gsub", "dump"];

/// The names the sandbox puts in an environment, which are not the file's.
fn provided(name: &str) -> bool {
    BASE.contains(&name)
        || LIBRARIES.contains(&name)
        || matches!(name, "dofile" | "require" | "package" | "_G")
}

/// Runs the file at `rel` under `install` and reads what it set.
pub fn run(install: &Path, rel: &str) -> Result<Chunk, Error> {
    let text = fs::read(install.join(rel)).map_err(|e| Error(format!("{rel}: {e}")))?;
    run_text(install, rel, &text)
}

/// Runs `text`, named `rel`, with `dofile` reading from `install`.
pub fn run_text(install: &Path, rel: &str, text: &[u8]) -> Result<Chunk, Error> {
    let lua = Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::MATH,
        LuaOptions::new(),
    )?;
    lua.set_memory_limit(MEMORY)?;
    let spent = Rc::new(Cell::new(0u64));
    let budget = spent.clone();
    lua.set_global_hook(
        HookTriggers::new().every_nth_instruction(HOOK_EVERY),
        move |_, _| {
            budget.set(budget.get() + u64::from(HOOK_EVERY));
            if budget.get() > INSTRUCTIONS {
                Err(mlua::Error::runtime(format!(
                    "ran past {INSTRUCTIONS} instructions, which data does not need"
                )))
            } else {
                Ok(VmState::Continue)
            }
        },
    )?;

    let globals = lua.globals();
    // The state's own table, which strings reach as their methods too.
    let string: mlua::Table = globals.raw_get("string")?;
    for name in UNSAFE_STRING {
        string.raw_set(name, mlua::Value::Nil)?;
    }
    let env = lua.create_table()?;
    for name in BASE.iter().chain(&LIBRARIES) {
        env.raw_set(*name, globals.raw_get::<mlua::Value>(*name)?)?;
    }
    env.raw_set("_G", env.clone())?;
    let package = lua.create_table()?;
    package.raw_set("path", "")?;
    package.raw_set("cpath", "")?;
    env.raw_set("package", package)?;

    let unresolved = Rc::new(RefCell::new(BTreeSet::new()));
    let missing = unresolved.clone();
    let meta = lua.create_table()?;
    meta.raw_set(
        "__index",
        lua.create_function(move |_, (_, k): (mlua::Value, mlua::Value)| {
            if let mlua::Value::String(s) = k {
                missing
                    .borrow_mut()
                    .insert(String::from_utf8_lossy(&s.as_bytes()).into_owned());
            }
            Ok(mlua::Value::Nil)
        })?,
    )?;
    env.set_metatable(Some(meta))?;

    let stood_in = unresolved.clone();
    let translate = lua.create_function(|_, v: mlua::Value| Ok(v))?;
    env.raw_set(
        "require",
        lua.create_function(move |lua, name: String| {
            let module = lua.create_table()?;
            if name == "i_18n" {
                module.raw_set("translate", translate.clone())?;
            } else {
                stood_in.borrow_mut().insert(format!("require({name:?})"));
            }
            Ok(module)
        })?,
    )?;

    let included = Rc::new(RefCell::new(Vec::<(String, u64)>::new()));
    let ran = included.clone();
    let root: PathBuf = install.to_path_buf();
    let scope = env.clone();
    env.raw_set(
        "dofile",
        lua.create_function(move |lua, path: String| {
            let rel = install::resolve(&root, &path).ok_or_else(|| {
                mlua::Error::runtime(format!(
                    "dofile names {path}, which the install does not hold"
                ))
            })?;
            let bytes = fs::read(root.join(&rel)).map_err(mlua::Error::external)?;
            if !ran.borrow().iter().any(|(p, _)| *p == rel) {
                ran.borrow_mut().push((rel.clone(), bytes.len() as u64));
            }
            lua.load(bytes.as_slice())
                .set_mode(ChunkMode::Text)
                .set_name(format!("@{rel}"))
                .set_environment(scope.clone())
                .exec()
        })?,
    )?;

    lua.load(text)
        .set_mode(ChunkMode::Text)
        .set_name(format!("@{rel}"))
        .set_environment(env.clone())
        .exec()?;

    let mut out = Chunk::default();
    let mut left = VALUES;
    for pair in env.pairs::<mlua::Value, mlua::Value>() {
        let (k, v) = pair?;
        let mlua::Value::String(k) = k else { continue };
        let name = String::from_utf8_lossy(&k.as_bytes()).into_owned();
        if provided(&name) {
            continue;
        }
        if let Some(v) = data(v, 0, &mut left)? {
            out.globals.insert(name, v);
        }
    }
    out.included = included.take();
    out.unresolved = unresolved.take();
    Ok(out)
}

/// Tables nested deeper than this are not data.
const DEPTH: usize = 64;

/// How many values a file's globals may hold, counting a table each time it
/// is reached: a table that holds itself twice at each level doubles at each.
/// The largest table read, Syria's towns, holds about 10 000.
const VALUES: usize = 1_000_000;

/// `v` as data, or `None` for a value that is not: a function, a thread,
/// userdata.
fn data(v: mlua::Value, depth: usize, left: &mut usize) -> Result<Option<Value>, Error> {
    if *left == 0 {
        return Err(Error(format!("the globals hold more than {VALUES} values")));
    }
    *left -= 1;
    Ok(Some(match v {
        mlua::Value::Nil => Value::Nil,
        mlua::Value::Boolean(b) => Value::Bool(b),
        mlua::Value::Integer(n) => Value::Num(n as f64),
        mlua::Value::Number(n) => Value::Num(n),
        mlua::Value::String(s) => Value::Str(String::from_utf8_lossy(&s.as_bytes()).into_owned()),
        mlua::Value::Table(t) => {
            if depth == DEPTH {
                return Err(Error(format!("a table nests deeper than {DEPTH}")));
            }
            let mut entries = Vec::new();
            for pair in t.pairs::<mlua::Value, mlua::Value>() {
                let (k, v) = pair?;
                let key = match data(k, depth + 1, left)? {
                    Some(Value::Bool(b)) => Key::Bool(b),
                    Some(Value::Num(n)) => Key::Num(n),
                    Some(Value::Str(s)) => Key::Str(s),
                    _ => continue,
                };
                if let Some(v) = data(v, depth + 1, left)? {
                    entries.push((key, v));
                }
            }
            entries.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            Value::Table(Table { entries })
        }
        _ => return Ok(None),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;

    fn put(root: &Path, rel: &str, text: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    fn parse(text: &str) -> Chunk {
        let s = Scratch::new();
        run_text(&s.0, "m.lua", text.as_bytes()).unwrap()
    }

    fn fails(text: &str) -> String {
        let s = Scratch::new();
        run_text(&s.0, "m.lua", text.as_bytes()).unwrap_err().0
    }

    #[test]
    fn reads_tables_and_translated_strings() {
        let c = parse(
            "local gettext = require(\"i_18n\")\nlocal       _ = gettext.translate\n\
             beacons = {\n\t{\n\t\tdisplay_name = _('Anapa');\n\t\tposition = { -1321.5, 12, 0x10 };\n\
             \t\tok = true; gone = nil;\n\t};\n}\n",
        );
        let b = c.table("beacons").unwrap().list().unwrap();
        let Value::Table(b) = b[0] else { panic!() };
        assert_eq!(b.str("display_name"), Some("Anapa"));
        let p: Vec<&Value> = b.table("position").unwrap().list().unwrap();
        assert_eq!(
            p,
            [&Value::Num(-1321.5), &Value::Num(12.0), &Value::Num(16.0)]
        );
        assert_eq!(b.get("ok"), Some(&Value::Bool(true)));
        assert_eq!(b.get("gone"), None);
        assert!(c.unresolved.is_empty(), "{:?}", c.unresolved);
        assert!(!c.globals.contains_key("gettext"), "locals are not globals");
    }

    #[test]
    fn lists_keys_in_order_whatever_order_they_were_given() {
        let c = parse("t = { [31] = 'c', [2] = 'b', [1] = 'a' }\nu = { [1] = 'y', [0] = 'x' }\n");
        let t = c.table("t").unwrap();
        assert_eq!(t.list(), None, "1, 2 and 31 are not consecutive");
        assert_eq!(t.index(31.0), Some(&Value::Str("c".into())));
        let u: Vec<&Value> = c.table("u").unwrap().list().unwrap();
        assert_eq!(u, [&Value::Str("x".into()), &Value::Str("y".into())]);
        assert_eq!(Table::default().list(), Some(vec![]));
        let c = parse("t = { [\"KUTAISI\"] = { latitude = 42.27 } }");
        assert!(c.table("t").unwrap().list().is_none());
    }

    #[test]
    fn a_name_nothing_assigned_is_nil_and_reported() {
        let c = parse("t = { type = BEACON_TYPE_AIRPORT_TACAN; callsign = 'NDO' }");
        let t = c.table("t").unwrap();
        assert_eq!(t.get("type"), None);
        assert_eq!(t.str("callsign"), Some("NDO"));
        assert_eq!(
            c.unresolved.iter().collect::<Vec<_>>(),
            ["BEACON_TYPE_AIRPORT_TACAN"]
        );
    }

    #[test]
    fn a_dofile_runs_the_installs_script_in_the_same_environment() {
        let s = Scratch::new();
        put(
            &s.0,
            "Scripts/World/Radio/BeaconTypes.lua",
            "package.path = package.path..';./Scripts/?.lua;'\nlocal utils = require('utils')\n\
             BEACON_TYPE_HOMER = 8\nlocal function homer(x) return { type = BEACON_TYPE_HOMER, x = x } end\n\
             SAMPLE = homer(3)\n",
        );
        put(
            &s.0,
            "Scripts/World/Radio/FrequencyBands.lua",
            "HF = 0\nUHF = 3\n",
        );
        let text = "dofile('scripts/world/radio/BeaconTypes.lua')\n\
                    dofile('Scripts/World/Radio/FrequencyBands.lua')\n\
                    dofile('Scripts/World/Radio/FrequencyBands.lua')\n\
                    f = { [HF] = 1, [UHF] = 2, kind = BEACON_TYPE_HOMER }\n";
        let c = run_text(&s.0, "m.lua", text.as_bytes()).unwrap();
        let f = c.table("f").unwrap();
        assert_eq!(f.index(0.0), Some(&Value::Num(1.0)));
        assert_eq!(f.index(3.0), Some(&Value::Num(2.0)));
        assert_eq!(f.num("kind"), Some(8.0));
        assert_eq!(c.table("SAMPLE").unwrap().num("x"), Some(3.0));
        let ran: Vec<&str> = c.included.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            ran,
            [
                "Scripts/World/Radio/BeaconTypes.lua",
                "Scripts/World/Radio/FrequencyBands.lua"
            ]
        );
        assert_eq!(c.included[1].1, 15);
        assert!(
            c.unresolved.contains("require(\"utils\")"),
            "{:?}",
            c.unresolved
        );
        assert!(!c.globals.contains_key("package"));
    }

    #[test]
    fn a_dofile_outside_the_install_is_refused() {
        let e = fails("\n\ndofile('Scripts/x.lua')\n");
        assert!(
            e.contains("dofile names Scripts/x.lua, which the install does not hold"),
            "{e}"
        );
        assert!(fails("dofile('../../etc/passwd')").contains("does not hold"));
    }

    #[test]
    fn names_the_file_and_line_of_an_error() {
        let e = fails("x = 1\nlocal t = nil\ny = t.field\n");
        assert!(e.contains("m.lua:3:"), "{e}");
        assert!(fails("t = { 1, 2").contains("m.lua:1:"));
    }

    #[test]
    fn nothing_reaches_a_file_or_the_system() {
        for code in [
            "io.open('x')",
            "os.exit(1)",
            "loadfile('x')",
            "loadstring('return 1')()",
            "getfenv(0)",
            "debug.getinfo(1)",
            "print('x')",
            "pcall(error)",
            "string.find('a', 'a')",
            "('a'):match('a')",
            "string.gsub('a', 'a', 'b')",
            "('a'):gmatch('a')",
            "string.dump(tostring)",
        ] {
            let e = fails(code);
            assert!(e.contains("attempt to"), "{code}: {e}");
        }
    }

    #[test]
    fn a_loop_that_never_ends_is_cut_off() {
        let e = fails("while true do end");
        assert!(e.contains("instructions, which data does not need"), "{e}");
        let e = fails("t = {} while true do t[#t + 1] = string.rep('x', 1000000) .. #t end");
        assert!(e.contains("memory"), "{e}");
    }

    #[test]
    fn refuses_bytecode() {
        let s = Scratch::new();
        let e = run_text(&s.0, "m.lua", b"\x1bLua\x51\x00").unwrap_err().0;
        assert!(e.contains("binary"), "{e}");
    }

    #[test]
    fn a_table_that_holds_itself_over_and_over_is_cut_off() {
        let e = fails("a = {} for i = 1, 40 do a = { a, a } end x = a");
        assert!(e.contains("more than 1000000 values"), "{e}");
    }

    #[test]
    fn replaces_bytes_that_are_not_utf8() {
        let s = Scratch::new();
        let c = run_text(&s.0, "m.lua", b"a = 'Boulder\xf1ity'").unwrap();
        assert_eq!(c.globals["a"], Value::Str("Boulder\u{fffd}ity".into()));
    }
}
