//! The commands for people: one subcommand per operation, its flags read
//! into the request `query` takes, and its answer printed as a plain list,
//! or with `--json` as `query` prints it.
//!
//! Its own reader takes the command line, not clap's: a place such as
//! `-284887,683859` starts with a hyphen, and clap reads a value starting
//! with one only on terms that let the next flag be taken for a value too.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use dcsterrain_core::query::Answer;
use dcsterrain_core::synth;
use dcsterrain_core::types::Issue;
use serde_json::{Value, json};

use crate::flags::{self, Draft, PendingKind, PlaceAs, Shape, Spec, Usage, kebab};
use crate::{CacheDir, place, query, refuse};

/// One example per command, for its help. Each is read in a test, so none
/// can fall out of step with the flags.
pub const EXAMPLES: &[(&str, &str)] = &[
    ("describe", "dcsterrain describe --terrain Caucasus"),
    (
        "sample",
        "dcsterrain sample --terrain Caucasus --points airport:Kutaisi town:Zugdidi --layers height slope cover",
    ),
    (
        "geo",
        "dcsterrain geo --terrain Caucasus --to-latlon airport:Kutaisi --to-xz 42.18,42.48",
    ),
    (
        "score_site",
        "dcsterrain score-site --terrain Caucasus --at 38TLM2860066900 --footprint 120 80 --criteria slope-max 3",
    ),
    (
        "find_sites",
        "dcsterrain find-sites --terrain Caucasus --region airdrome-band Kutaisi 3000 15000 --footprint 120 80 --criteria surface-in land --criteria slope-max 3 --criteria dist-road-band 100 1000 --criteria clearing-max 0 --spacing 2000 --limit 3",
    ),
    (
        "visible",
        "dcsterrain visible --terrain Caucasus --a airport:Kutaisi@2 --b town:Zugdidi@100",
    ),
    (
        "viewshed",
        "dcsterrain viewshed --terrain Caucasus --observer airport:Kutaisi@10 --radius 20000 --target-alt-agl 2 --geojson-out seen.geojson",
    ),
    (
        "radar_floor",
        "dcsterrain radar-floor --terrain Caucasus --site town:Zugdidi@15 --radius 40000",
    ),
    (
        "coverage",
        "dcsterrain coverage --terrain Caucasus --site town:Zugdidi@10 --envelope 25000 30,100 --asset circle airport:Kutaisi 5000",
    ),
    (
        "exposure",
        "dcsterrain exposure --terrain Caucasus --sites town:Zugdidi@10 20000 --sites airport:Kutaisi@10 15000 --target-alt-agl 30",
    ),
    (
        "approach_spectrum",
        "dcsterrain approach-spectrum --terrain Caucasus --site town:Zugdidi@10 --radius 30000 --alt-bands-agl 30 100 --sector 270 90",
    ),
    (
        "unmask_profile",
        "dcsterrain unmask-profile --terrain Caucasus --site town:Zugdidi@10 --route airport:Senaki-Kolkhi@100 airport:Kutaisi@100",
    ),
    (
        "route",
        "dcsterrain route --terrain Caucasus --network roads --from town:Zugdidi --to airport:Kutaisi --avoid circle airport:Senaki-Kolkhi 3000",
    ),
    (
        "route_alternatives",
        "dcsterrain route-alternatives --terrain Caucasus --network roads --from town:Zugdidi --to airport:Kutaisi --k 3",
    ),
    (
        "nearest",
        "dcsterrain nearest --terrain Caucasus --kind airdrome --at town:Zugdidi --n 3",
    ),
    (
        "scenery_in",
        "dcsterrain scenery-in --terrain Caucasus --region circle airport:Kutaisi 500",
    ),
    (
        "chokepoints",
        "dcsterrain chokepoints --terrain Caucasus --region circle airport:Kutaisi 20000 --network roads --limit 5",
    ),
    (
        "trafficability",
        "dcsterrain trafficability --terrain Caucasus --at 38TLM2860066900 --max-slope-deg 25 --network roads",
    ),
    (
        "point_info",
        "dcsterrain point-info --terrain Caucasus --points airport:Kutaisi \"42.177616N 42.481292E\"",
    ),
    (
        "airfield_info",
        "dcsterrain airfield-info --terrain Caucasus --airdrome Batumi --fits width=16 length=17 height=5 helicopter",
    ),
    (
        "validate_placements",
        "dcsterrain validate-placements --terrain Caucasus --placements id=sam at=38TLM2860066900 max-slope-deg=5",
    ),
    (
        "towns",
        "dcsterrain towns --terrain Caucasus --region circle airport:Kutaisi 30000 --size town city",
    ),
];

/// The flags every command takes beside its request's, and whether each
/// takes a value.
const SHARED: &[(&str, bool)] = &[
    ("terrain", true),
    ("install", true),
    ("cache", true),
    ("allow-crs-residual", false),
    ("seed", true),
    ("size-km", true),
    ("cell", true),
    ("threads", true),
    ("verbose", false),
    ("json", false),
    ("geojson-out", true),
    ("show-request", false),
    ("help", false),
];

/// A command line read: the shared flags, and the request's in order.
#[derive(Debug, Default, PartialEq)]
pub struct Line {
    pub shared: Vec<(String, Option<String>)>,
    pub given: Vec<(String, Vec<String>)>,
}

impl Line {
    fn get(&self, name: &str) -> Option<&str> {
        self.shared
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .and_then(|(_, v)| v.as_deref())
    }

    fn has(&self, name: &str) -> bool {
        self.shared.iter().any(|(n, _)| n == name)
    }
}

/// Whether a field's flag takes more than one value.
fn takes_many(shape: &Shape) -> bool {
    matches!(
        shape,
        Shape::List(_) | Shape::Object(_) | Shape::OneOf(_) | Shape::Map(_)
    )
}

/// Reads a command line into its flags, each with its values.
pub fn tokens(spec: &Spec, args: &[String]) -> Result<Line, Usage> {
    let mut line = Line::default();
    // The flag the next bare value goes to, and whether it takes more than
    // one.
    let mut open: Option<(bool, bool)> = None;
    let fields: Vec<(String, &Shape)> = spec
        .fields
        .iter()
        .map(|f| (kebab(&f.name), &f.shape))
        .collect();
    for arg in args {
        let flag = arg.strip_prefix("--");
        let looks_like_value =
            arg.starts_with('-') && arg[1..].starts_with(|c: char| c.is_ascii_digit() || c == '.');
        if arg == "-h" {
            line.shared.push(("help".into(), None));
            open = None;
            continue;
        }
        if let Some(flag) = flag.filter(|f| !f.is_empty()) {
            let (name, value) = match flag.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (flag, None),
            };
            if let Some((_, takes)) = SHARED.iter().find(|(n, _)| *n == name) {
                if name == "geojson-out" && !spec.geojson {
                    return Err(Usage(format!(
                        "--geojson-out: {} returns no GeoJSON",
                        spec.command()
                    )));
                }
                match (takes, value) {
                    (false, Some(_)) => return Err(Usage(format!("--{name} takes no value"))),
                    (true, Some(v)) => {
                        line.shared.push((name.into(), Some(v)));
                        open = None;
                    }
                    (true, None) => {
                        line.shared.push((name.into(), None));
                        open = Some((true, false));
                    }
                    (false, None) => {
                        line.shared.push((name.into(), None));
                        open = None;
                    }
                }
                continue;
            }
            if let Some((_, shape)) = fields.iter().find(|(n, _)| n == name) {
                let many = takes_many(shape);
                let switch = **shape == Shape::Bool;
                let values: Vec<String> = value.into_iter().collect();
                let filled = !values.is_empty();
                line.given.push((name.into(), values));
                open = match (switch, many, filled) {
                    (true, ..) => None,
                    (false, true, _) => Some((false, true)),
                    (false, false, true) => None,
                    (false, false, false) => Some((false, false)),
                };
                continue;
            }
            let names: Vec<String> = SHARED
                .iter()
                .map(|(n, _)| n.to_string())
                .chain(fields.iter().map(|(n, _)| n.clone()))
                .collect();
            let near = flags::closest(name, names.iter().map(String::as_str))
                .map(|n| format!(" Did you mean `--{n}`?"))
                .unwrap_or_default();
            return Err(Usage(format!(
                "--{name} is not a flag of {}.{near}",
                spec.command()
            )));
        }
        if arg.starts_with('-') && arg.len() > 1 && !looks_like_value {
            return Err(Usage(format!("{arg} is not a flag of {}", spec.command())));
        }
        match open {
            Some((true, _)) => {
                if let Some(last) = line.shared.last_mut() {
                    last.1 = Some(arg.clone());
                }
                open = None;
            }
            Some((false, many)) => {
                if let Some(last) = line.given.last_mut() {
                    last.1.push(arg.clone());
                }
                if !many {
                    open = None;
                }
            }
            None => {
                return Err(Usage(format!("\"{arg}\" follows no flag that takes it")));
            }
        }
    }
    if let Some((name, None)) = line
        .shared
        .iter()
        .find(|(n, v)| v.is_none() && SHARED.iter().any(|(s, t)| s == n && *t))
    {
        return Err(Usage(format!("--{name} needs a value")));
    }
    Ok(line)
}

/// The example `op`'s help ends with.
pub fn example(op: &str) -> Option<&'static str> {
    EXAMPLES.iter().find(|(o, _)| *o == op).map(|(_, e)| *e)
}

/// Prints `op`'s help.
pub fn help(spec: &Spec) -> ExitCode {
    print!("{}", spec.help(example(&spec.op)));
    ExitCode::SUCCESS
}

/// A command line that cannot be read: what is wrong, and where to look.
fn usage(spec: &Spec, Usage(why): Usage) -> ExitCode {
    eprintln!(
        "error: {why}\n\nFor the flags {0} takes, run: dcsterrain {0} --help",
        spec.command()
    );
    ExitCode::from(2)
}

/// What the shared flags say: how to open the terrain, and what to print.
struct Shared {
    opener: query::Opener,
    json: bool,
    geojson_out: Option<PathBuf>,
    show_request: bool,
}

fn number<T: std::str::FromStr>(line: &Line, name: &str, default: T) -> Result<T, Usage> {
    match line.get(name) {
        None => Ok(default),
        Some(v) => v.parse().map_err(|_| {
            Usage(format!(
                "--{name} takes a whole number, and \"{v}\" is not one"
            ))
        }),
    }
}

fn shared(line: &Line) -> Result<Shared, Usage> {
    let Some(terrain) = line.get("terrain") else {
        return Err(Usage(
            "--terrain is required: the terrain's id, as `list-terrains` shows it, or `synth`"
                .into(),
        ));
    };
    let synth = (
        number(line, "seed", synth::DEFAULT_SEED)?,
        number(line, "size-km", synth::DEFAULT_SIZE_KM)?,
        number(line, "cell", synth::CELL_SIZE_M)?,
    );
    Ok(Shared {
        opener: query::Opener {
            install: line.get("install").map(PathBuf::from),
            id: Some(terrain.to_string()),
            synth,
            allow_crs_residual: line.has("allow-crs-residual"),
            cache: CacheDir {
                dir: line.get("cache").map(PathBuf::from),
            },
            terrain: None,
            opens: 0,
        },
        json: line.has("json"),
        geojson_out: line.get("geojson-out").map(PathBuf::from),
        show_request: line.has("show-request"),
    })
}

/// The JSON pointer's parent and last step.
fn split_pointer(pointer: &str) -> (&str, &str) {
    pointer.rsplit_once('/').unwrap_or(("", pointer))
}

/// Writes each place and airdrome the command line names into the request,
/// read on the open terrain.
fn resolve(opener: &mut query::Opener, draft: Draft) -> Result<Value, Vec<Issue>> {
    let Draft {
        mut request,
        pending,
    } = draft;
    if pending.is_empty() {
        return Ok(request);
    }
    let terrain = opener.current()?;
    let mut issues = Vec::new();
    for p in pending {
        let target = match &p.kind {
            PendingKind::Place(PlaceAs::Split(_)) => p.pointer.as_str(),
            _ => split_pointer(&p.pointer).0,
        };
        let value = match &p.kind {
            PendingKind::Airdrome => place::airport(terrain, &p.flag, &p.text),
            PendingKind::Place(kind) => {
                place::resolve(terrain, &p.flag, &p.text).map(|at| match kind {
                    PlaceAs::Object => json!({"x": at.x, "z": at.z}),
                    PlaceAs::Pair => json!([at.x, at.z]),
                    PlaceAs::Split(prefix) => json!({
                        format!("{prefix}x"): at.x,
                        format!("{prefix}z"): at.z,
                    }),
                })
            }
        };
        let value = match value {
            Ok(v) => v,
            Err(e) => {
                issues.extend(e);
                continue;
            }
        };
        let Some(slot) = request.pointer_mut(target) else {
            continue;
        };
        match (&p.kind, slot) {
            (PendingKind::Place(PlaceAs::Split(_)), Value::Object(m)) => {
                if let Value::Object(v) = value {
                    m.extend(v);
                }
            }
            (_, Value::Object(m)) => {
                m.insert(split_pointer(&p.pointer).1.to_string(), value);
            }
            (_, Value::Array(a)) => {
                if let Ok(i) = split_pointer(&p.pointer).1.parse::<usize>()
                    && i < a.len()
                {
                    a[i] = value;
                }
            }
            _ => {}
        }
    }
    if !issues.is_empty() {
        return Err(issues);
    }
    corners(&mut request);
    Ok(request)
}

/// Two corners given in either order make a box: each `min_` is the lesser.
fn corners(v: &mut Value) {
    match v {
        Value::Object(m) => {
            for axis in ["x", "z"] {
                let (lo, hi) = (format!("min_{axis}"), format!("max_{axis}"));
                if let (Some(a), Some(b)) = (
                    m.get(&lo).and_then(Value::as_f64),
                    m.get(&hi).and_then(Value::as_f64),
                ) && a > b
                {
                    m.insert(lo, json!(b));
                    m.insert(hi, json!(a));
                }
            }
            m.values_mut().for_each(corners);
        }
        Value::Array(a) => a.iter_mut().for_each(corners),
        _ => {}
    }
}

/// An issue's place in the request as the flags write it:
/// `criteria.2.params.deg` is `--criteria #3 deg`.
fn flag_of(path: &str) -> String {
    if path.is_empty() || path.starts_with("--") {
        return path.to_string();
    }
    let mut words = Vec::new();
    for (i, step) in path.split('.').enumerate() {
        if i == 0 {
            let step = match step {
                "x" | "z" => "at",
                s => s,
            };
            words.push(format!("--{}", kebab(step)));
        } else if let Ok(n) = step.parse::<usize>() {
            words.push(format!("#{}", n + 1));
        } else if step != "params" {
            words.push(kebab(step));
        }
    }
    words.join(" ")
}

/// Prints a refusal: `query`'s document with `--json`, else each issue in
/// words on stderr.
fn refused(json: bool, issues: Vec<Issue>) -> ExitCode {
    if json {
        return refuse(issues);
    }
    for i in &issues {
        let at = flag_of(&i.path);
        if at.is_empty() {
            eprintln!("error: {}", i.message);
        } else {
            eprintln!("error: {at}: {}", i.message);
        }
    }
    ExitCode::FAILURE
}

/// A value on one line, as the plain list shows it.
fn shown(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "none".into(),
        v => v.to_string(),
    }
}

/// The plain list: one line per top-level field of the answer, a list's
/// or an object's members below it, one line each, so no field is dropped.
pub fn plain(answer: &Value, geojson_out: Option<&std::path::Path>) -> String {
    let mut out = String::new();
    let Value::Object(top) = answer else {
        return format!("{}\n", shown(answer));
    };
    for (k, v) in top {
        match v {
            _ if k == "geojson" => {
                let n = v["features"].as_array().map_or(0, Vec::len);
                let to = geojson_out
                    .map(|p| format!(", written to {}", p.display()))
                    .unwrap_or_default();
                out += &format!("{k}: FeatureCollection, {n} features{to}\n");
            }
            Value::Array(a) if a.is_empty() => out += &format!("{k}: none\n"),
            Value::Array(a) => {
                out += &format!(
                    "{k}: {} item{}\n",
                    a.len(),
                    if a.len() == 1 { "" } else { "s" }
                );
                for item in a {
                    out += &format!("  {}\n", shown(item));
                }
            }
            Value::Object(m) if !m.is_empty() => {
                out += &format!("{k}:\n");
                for (k2, v2) in m {
                    out += &format!("  {k2}: {}\n", shown(v2));
                }
            }
            v => out += &format!("{k}: {}\n", shown(v)),
        }
    }
    out
}

/// The answer's `geojson`, written to `path`.
fn write_geojson(answer: &Answer, path: &std::path::Path) -> Result<(), Vec<Issue>> {
    let value = match answer {
        Answer::Value(v) => v,
        Answer::Rasters(r) => &r.value,
    };
    let fail = |why: String| vec![Issue::new("internal_error", &why, "--geojson-out")];
    let Some(geojson) = value.get("geojson") else {
        return Err(fail("the answer holds no GeoJSON".into()));
    };
    let file = std::fs::File::create(path)
        .map_err(|e| fail(format!("{} cannot be written: {e}", path.display())))?;
    let mut out = std::io::BufWriter::new(file);
    serde_json::to_writer(&mut out, geojson)
        .map_err(std::io::Error::from)
        .and_then(|()| out.write_all(b"\n"))
        .and_then(|()| out.flush())
        .map_err(|e| fail(format!("{} cannot be written: {e}", path.display())))
}

/// Runs operation `op` from the arguments after its name. `init` sets up
/// the log and the threads, from the flags before and after the name.
pub fn run(op: &str, args: &[String], init: impl FnOnce(bool, Option<u16>)) -> ExitCode {
    let Some(spec) = Spec::of(op) else {
        eprintln!("error: no operation `{op}`");
        return ExitCode::from(2);
    };
    let line = match tokens(&spec, args) {
        Ok(l) => l,
        Err(u) => return usage(&spec, u),
    };
    if line.has("help") {
        return help(&spec);
    }
    let threads = match line.get("threads").map(str::parse::<u16>) {
        None => None,
        Some(Ok(n)) if n > 0 => Some(n),
        Some(_) => {
            return usage(&spec, Usage("--threads takes a whole number from 1".into()));
        }
    };
    let draft = match spec.read(&line.given) {
        Ok(d) => d,
        Err(u) => return usage(&spec, u),
    };
    let Shared {
        mut opener,
        json,
        geojson_out,
        show_request,
    } = match shared(&line) {
        Ok(s) => s,
        Err(u) => return usage(&spec, u),
    };
    init(line.has("verbose"), threads);

    let mut request = match resolve(&mut opener, draft) {
        Ok(r) => r,
        Err(issues) => return refused(json, issues),
    };
    if geojson_out.is_some()
        && let Value::Object(m) = &mut request
    {
        m.insert("geojson".into(), Value::Bool(true));
    }
    if show_request {
        println!("{request}");
        return ExitCode::SUCCESS;
    }
    let answer = match opener.answer(op, &request) {
        Ok(a) => a,
        Err(issues) => return refused(json, issues),
    };
    if let Some(path) = &geojson_out
        && let Err(issues) = write_geojson(&answer, path)
    {
        return refused(json, issues);
    }
    if json {
        return query::print(&answer);
    }
    let value = match &answer {
        Answer::Value(v) => std::borrow::Cow::Borrowed(v),
        Answer::Rasters(r) => match serde_json::to_value(r) {
            Ok(v) => std::borrow::Cow::Owned(v),
            Err(e) => {
                return refused(
                    false,
                    vec![Issue::new("internal_error", &e.to_string(), "")],
                );
            }
        },
    };
    print!("{}", plain(&value, geojson_out.as_deref()));
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests;
