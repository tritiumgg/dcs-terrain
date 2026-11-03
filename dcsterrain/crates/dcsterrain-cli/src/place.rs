//! A place, as a person types it on the command line, read to the DCS `x`
//! and `z` a request takes. This is the one description of what a place
//! flag accepts; letters may be of either case in every form.
//!
//! - **DCS x,z**: two numbers in meters, `x` (north) first, one comma
//!   between, a space after it allowed: `-284887,683859`. An `m` on both
//!   numbers, `120m,-45m`, says they are meters, which two numbers each
//!   within ±180 need, since they could as well be latitude and longitude.
//! - **MGRS**: the zone, its band, the square's two letters, then an even
//!   count of 2 to 10 digits, spaces anywhere or nowhere:
//!   `38T KM 91980 72567`, `38tkm919725`. The point is the center of the
//!   square the digits name, or of its part in the zone and band written
//!   when their edge cuts it.
//! - **Latitude and longitude**, each half with its hemisphere letter, N or
//!   S and E or W, all before or all after the numbers, the halves apart by
//!   spaces or a comma: `42.177616N 42.481292E`, `N42.177616 E42.481292`.
//!   Each half is degrees, degrees and minutes, or degrees, minutes and
//!   seconds, marked `°`, `'` and `"` (or the `’` and `”` a word processor
//!   writes) or apart by spaces:
//!   `42°10'39.4"N 42°28'52.7"E`, `N42 10 39.4 E42 28 52.7`. Latitude
//!   normally comes first; the letters let the other order be read too. A
//!   sign is refused: the letter gives it.
//! - **An airport or a town by name**, its kind always written:
//!   `airport:Kutaisi`, `town:Zugdidi`. An airport answers to its `name_id`
//!   and its display name, a town to each of its names in the town list.
//!   Names match whole, regardless of case, accents, spaces, hyphens,
//!   underscores, apostrophes and dots.
//!
//! Nothing is guessed. A name with no kind, two small numbers with no `m`,
//! and a name two places share are refused with what to type instead. A
//! place never holds `@`.

use dcsterrain_core::query::{self, MgrsSquare, Terrain, parse_mgrs};
use dcsterrain_core::types::{DescribeRequest, GeoRequest, Issue, LatLon, Point};
use serde_json::{Value, json};

/// The example of each form an error lists.
const FORMS: &str = "A place is one of:
  x,z in DCS meters        -284887,683859
  MGRS                     38T KM 91980 72567
  latitude and longitude   42.177616N 42.481292E
  an airport by name       airport:Kutaisi
  a town by name           town:Zugdidi";

/// How many of the closest names an unknown name is answered with.
const CLOSEST: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Airport,
    Town,
}

impl Kind {
    fn prefix(self) -> &'static str {
        match self {
            Kind::Airport => "airport",
            Kind::Town => "town",
        }
    }
}

/// A place as written, before the terrain is asked anything.
#[derive(Debug, Clone, PartialEq)]
pub enum Written {
    Xz {
        x: f64,
        z: f64,
    },
    Mgrs(MgrsSquare),
    LatLon {
        lat: f64,
        lon: f64,
    },
    Name {
        kind: Kind,
        name: String,
    },
    /// A name with no kind, which is refused once the names that match it
    /// are known.
    Bare(String),
}

/// Why `text` is not a place, as the error's message.
#[derive(Debug, Clone, PartialEq)]
pub enum Unread {
    /// What is wrong, ending a sentence that starts with the place.
    Malformed(String),
    /// Two numbers each within ±180, as written.
    TwoSmall(String, String),
}

/// Reads the form of `text`, trying each form in turn: a kind's prefix,
/// then MGRS, latitude and longitude, and x,z. Once a form fits, its rules
/// decide, so a malformed MGRS is reported as one.
pub fn read(text: &str) -> Result<Written, Unread> {
    let t = text.trim();
    let malformed = |why: &str| Err(Unread::Malformed(format!("it {why}")));
    if t.is_empty() {
        return malformed("is empty");
    }
    if t.contains('@') {
        return malformed("has `@`, which no place takes");
    }
    if let Some((k, name)) = t.split_once(':')
        && let k = k.trim_end()
        && !k.is_empty()
        && k.chars().all(|c| c.is_ascii_alphabetic())
    {
        let kind = match k.to_ascii_lowercase().as_str() {
            "airport" => Kind::Airport,
            "town" => Kind::Town,
            _ => {
                return malformed(&format!(
                    "has the kind `{k}:`, and the kinds are airport: and town:"
                ));
            }
        };
        let name = name.trim();
        if name.is_empty() {
            return malformed(&format!("names no {}", kind.prefix()));
        }
        return Ok(Written::Name {
            kind,
            name: name.to_string(),
        });
    }
    let squeezed: Vec<char> = t.chars().filter(|c| !c.is_whitespace()).collect();
    let zone = squeezed.iter().take_while(|c| c.is_ascii_digit()).count();
    let letters = squeezed[zone..]
        .iter()
        .take_while(|c| c.is_ascii_alphabetic())
        .count();
    if (1..=2).contains(&zone) && letters == 3 {
        return parse_mgrs(t).map(Written::Mgrs).map_err(Unread::Malformed);
    }
    let digit = t.chars().any(|c| c.is_ascii_digit());
    let hemisphere = t.chars().any(|c| "NSEWnsew".contains(c));
    let latlon_char =
        |c: char| c.is_ascii_digit() || c.is_whitespace() || "NSEWnsew.,+-°º'\"′″’”".contains(c);
    if digit && hemisphere && t.chars().all(latlon_char) {
        return latlon(t).map_err(|why| Unread::Malformed(format!("it {why}")));
    }
    let xz_char = |c: char| c.is_ascii_digit() || c.is_whitespace() || ".,+-mM".contains(c);
    if digit && t.contains(',') && t.chars().all(xz_char) {
        return xz(t);
    }
    // A name may hold digits, but never a comma, which only coordinates
    // take.
    if t.chars().any(char::is_alphabetic) && !t.contains(',') {
        return Ok(Written::Bare(t.to_string()));
    }
    malformed("is not in any form a place takes")
}

/// A number as a person writes one: a sign, digits and a decimal point.
fn number(s: &str) -> Option<f64> {
    let body = s.strip_prefix(['+', '-']).unwrap_or(s);
    let (whole, fraction) = body.split_once('.').unwrap_or((body, ""));
    let digits = |d: &str| d.chars().all(|c| c.is_ascii_digit());
    let ok = !(whole.is_empty() && fraction.is_empty()) && digits(whole) && digits(fraction);
    ok.then(|| s.parse().ok()).flatten()
}

fn xz(t: &str) -> Result<Written, Unread> {
    let malformed = |why: &str| Err(Unread::Malformed(format!("it {why}")));
    let parts: Vec<&str> = t.split(',').map(str::trim).collect();
    if parts.len() != 2 {
        return malformed("has more than one comma; x,z is two numbers with one comma between");
    }
    let meters: Vec<bool> = parts.iter().map(|p| p.ends_with(['m', 'M'])).collect();
    let bare: Vec<&str> = parts
        .iter()
        .map(|p| p.strip_suffix(['m', 'M']).unwrap_or(p).trim_end())
        .collect();
    let (Some(x), Some(z)) = (number(bare[0]), number(bare[1])) else {
        return malformed(
            "is not two numbers; x,z is two numbers in meters, such as -284887,683859",
        );
    };
    if meters[0] != meters[1] {
        return malformed("has `m` on one number only; put it on both or neither");
    }
    if !meters[0] && x.abs() <= 180.0 && z.abs() <= 180.0 {
        return Err(Unread::TwoSmall(bare[0].to_string(), bare[1].to_string()));
    }
    Ok(Written::Xz { x, z })
}

fn latlon(t: &str) -> Result<Written, String> {
    let chars: Vec<char> = t.chars().collect();
    let at: Vec<usize> = (0..chars.len())
        .filter(|&k| "NSEWnsew".contains(chars[k]))
        .collect();
    if at.len() != 2 {
        return Err(format!(
            "has {} hemisphere letters; latitude and longitude take one each, N or S and E or W",
            at.len()
        ));
    }
    let piece = |a: usize, b: usize| -> String { chars[a..b].iter().collect() };
    let blank = |s: &str| s.chars().all(|c| c.is_whitespace() || c == ',');
    let (before, between, after) = (
        piece(0, at[0]),
        piece(at[0] + 1, at[1]),
        piece(at[1] + 1, chars.len()),
    );
    let (first, second) = match (blank(&before), blank(&after)) {
        (true, false) => (between, after),
        (false, true) => (before, between),
        (true, true) => {
            return Err(
                "puts one hemisphere letter before its number and the other after; put both \
                 before or both after"
                    .into(),
            );
        }
        (false, false) => {
            return Err(
                "has numbers both before and after its hemisphere letters; put both letters \
                 before their numbers or both after"
                    .into(),
            );
        }
    };
    let letter = |k: usize| chars[at[k]].to_ascii_uppercase();
    let (l1, l2) = (letter(0), letter(1));
    let lat_letter = |l: char| l == 'N' || l == 'S';
    let ((lat_text, lat_l), (lon_text, lon_l)) = match (lat_letter(l1), lat_letter(l2)) {
        (true, false) => ((first, l1), (second, l2)),
        (false, true) => ((second, l2), (first, l1)),
        _ => {
            return Err(format!(
                "has the letters {l1} and {l2}; one must be N or S and the other E or W"
            ));
        }
    };
    let lat = degrees(&lat_text, lat_l)?;
    let lon = degrees(&lon_text, lon_l)?;
    if lat > 90.0 {
        return Err(format!("has latitude {lat}°, over 90°"));
    }
    if lon > 180.0 {
        return Err(format!("has longitude {lon}°, over 180°"));
    }
    let sign = |l: char| if l == 'S' || l == 'W' { -1.0 } else { 1.0 };
    Ok(Written::LatLon {
        lat: sign(lat_l) * lat,
        lon: sign(lon_l) * lon,
    })
}

/// The unsigned degrees of one half: degrees, minutes and seconds, each
/// marked or apart by spaces, only the last with a decimal part.
fn degrees(text: &str, letter: char) -> Result<f64, String> {
    let half = text.trim_matches(|c: char| c.is_whitespace() || c == ',');
    if half.contains(',') {
        return Err(format!(
            "has a comma inside its {letter} half; a decimal is written with a point"
        ));
    }
    if half.contains(['+', '-']) {
        return Err(format!(
            "has a sign on its {letter} half; the letter gives the sign, so write it alone"
        ));
    }
    let mut parts: Vec<(String, Option<usize>)> = Vec::new();
    let mut current = String::new();
    for c in half.chars() {
        let mark = match c {
            '°' | 'º' => Some(0),
            // A word processor turns ' and " into ’ and ”.
            '\'' | '′' | '’' => Some(1),
            '"' | '″' | '”' => Some(2),
            _ => None,
        };
        if mark.is_some() || c.is_whitespace() {
            if !current.is_empty() {
                parts.push((std::mem::take(&mut current), mark));
            } else if mark.is_some() {
                return Err(format!(
                    "has a mark with no number before it in its {letter} half"
                ));
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        parts.push((current, None));
    }
    if parts.is_empty() || parts.len() > 3 {
        return Err(format!(
            "has {} numbers in its {letter} half; a half is degrees, then minutes, then seconds",
            parts.len()
        ));
    }
    let names = ["degrees", "minutes", "seconds"];
    let mut total = 0.0;
    for (k, (s, mark)) in parts.iter().enumerate() {
        if let Some(m) = mark
            && *m != k
        {
            return Err(format!(
                "marks its {letter} half's {} as {}",
                names[k], names[*m]
            ));
        }
        let v = number(s).ok_or_else(|| format!("has `{s}` in its {letter} half, not a number"))?;
        if s.contains('.') && k + 1 < parts.len() {
            return Err(format!(
                "has a decimal part on its {letter} half's {}; only the last number may have one",
                names[k]
            ));
        }
        if k > 0 && v >= 60.0 {
            return Err(format!(
                "has {} {v} in its {letter} half; they must be under 60",
                names[k]
            ));
        }
        total += v / 60f64.powi(k as i32);
    }
    Ok(total)
}

/// A name folded for matching: lower case, accents taken off Latin
/// letters, and spaces, hyphens, underscores, apostrophes and dots dropped.
pub fn fold(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars().flat_map(char::to_lowercase) {
        // Combining marks are the accents of text written decomposed.
        if c.is_whitespace() || "-_.'’‘`".contains(c) || ('\u{300}'..='\u{36f}').contains(&c) {
            continue;
        }
        match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => out.push('a'),
            'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => out.push('c'),
            'ď' | 'đ' | 'ð' => out.push('d'),
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => out.push('e'),
            'ĝ' | 'ğ' | 'ġ' | 'ģ' => out.push('g'),
            'ĥ' | 'ħ' => out.push('h'),
            'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => out.push('i'),
            'ĵ' => out.push('j'),
            'ķ' => out.push('k'),
            'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => out.push('l'),
            'ñ' | 'ń' | 'ņ' | 'ň' => out.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => out.push('o'),
            'ŕ' | 'ŗ' | 'ř' => out.push('r'),
            'ś' | 'ŝ' | 'ş' | 'š' | 'ș' => out.push('s'),
            'ţ' | 'ť' | 'ŧ' | 'ț' => out.push('t'),
            'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => out.push('u'),
            'ŵ' => out.push('w'),
            'ý' | 'ÿ' | 'ŷ' => out.push('y'),
            'ź' | 'ż' | 'ž' => out.push('z'),
            'ß' => out.push_str("ss"),
            'æ' => out.push_str("ae"),
            'œ' => out.push_str("oe"),
            'þ' => out.push_str("th"),
            c => out.push(c),
        }
    }
    out
}

/// The edits that turn `a` into `b`: insertions, deletions and
/// substitutions of one character.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (above + 1)
                .min(row[j] + 1)
                .min(diagonal + usize::from(ca != *cb));
            diagonal = above;
        }
    }
    row[b.len()]
}

/// One airport or town a name may mean.
struct Named {
    kind: Kind,
    /// The name it is shown by.
    shown: String,
    keys: Vec<String>,
    at: Option<(f64, f64)>,
    /// An airport's `name_id`, the name a request takes it by.
    name_id: Option<String>,
}

impl Named {
    fn typed(&self) -> String {
        format!("{}:{}", self.kind.prefix(), self.shown)
    }
}

/// Every airport and town of the terrain's tables, which `airfield_info`
/// and `nearest` answer with.
fn names(terrain: &Terrain) -> Result<Vec<Named>, Vec<Issue>> {
    let tables = (terrain.source().tables()).map_err(|e| vec![Issue::new("refused", &e.0, "")])?;
    let mut all = Vec::new();
    for a in &tables.airdromes {
        let mut keys = vec![fold(&a.name_id)];
        keys.extend(a.display_name.as_deref().map(fold));
        all.push(Named {
            kind: Kind::Airport,
            shown: a.display_name.clone().unwrap_or_else(|| a.name_id.clone()),
            keys,
            at: a.x.zip(a.z),
            name_id: Some(a.name_id.clone()),
        });
    }
    for t in &tables.towns {
        let mut keys = vec![fold(&t.name)];
        keys.extend(t.display_name.as_deref().map(fold));
        all.push(Named {
            kind: Kind::Town,
            shown: t.display_name.clone().unwrap_or_else(|| t.name.clone()),
            keys,
            at: Some((t.x, t.z)),
            name_id: None,
        });
    }
    Ok(all)
}

/// The names of `pool` closest to `key`, closest first, ties in the order
/// they are typed.
fn closest<'a>(pool: impl Iterator<Item = &'a Named>, key: &str) -> Vec<String> {
    let mut near: Vec<(usize, String)> = pool
        .map(|n| {
            let d = n
                .keys
                .iter()
                .map(|k| distance(k, key))
                .min()
                .unwrap_or(usize::MAX);
            (d, n.typed())
        })
        .collect();
    near.sort();
    near.dedup_by(|a, b| a.1 == b.1);
    near.into_iter().take(CLOSEST).map(|(_, t)| t).collect()
}

/// The terrain's id, as messages name it.
fn terrain_name(terrain: &Terrain) -> String {
    terrain.provenance().terrain
}

/// `x` and `z` to the meter, as a choice shows them.
fn shown_xz(x: f64, z: f64) -> String {
    format!("x {x:.0} z {z:.0}")
}

/// Reads `text`, given to the flag `path`, to the point it names on
/// `terrain`: a refusal is one issue whose message completes a line that
/// begins with the flag.
pub fn resolve(terrain: &Terrain, path: &str, text: &str) -> Result<Point, Vec<Issue>> {
    resolve_among(terrain, || names(terrain), path, text)
}

/// `resolve`, with the airports and towns `names` gives.
fn resolve_among(
    terrain: &Terrain,
    names: impl Fn() -> Result<Vec<Named>, Vec<Issue>>,
    path: &str,
    text: &str,
) -> Result<Point, Vec<Issue>> {
    let issue = |code: &str, message: String| vec![Issue::new(code, &message, path)];
    let quoted = format!("\"{}\"", text.trim());
    let written = match read(text) {
        Ok(w) => w,
        Err(Unread::Malformed(why)) => {
            return Err(issue(
                "invalid_place",
                format!("{quoted} is not a place: {why}.\n{FORMS}"),
            ));
        }
        Err(Unread::TwoSmall(a, b)) => {
            let unsigned = |s: &str| s.trim_start_matches(['+', '-']).to_string();
            let negative = |s: &str| s.starts_with('-');
            let mut m = format!(
                "{quoted} could be latitude and longitude or DCS meters near the terrain's origin."
            );
            if number(&a).is_some_and(|v| v.abs() <= 90.0) {
                let ns = if negative(&a) { 'S' } else { 'N' };
                let ew = if negative(&b) { 'W' } else { 'E' };
                m += &format!(
                    "\nFor latitude and longitude, type: {}{ns} {}{ew}",
                    unsigned(&a),
                    unsigned(&b)
                );
            }
            m += &format!("\nFor DCS meters, type: {a}m,{b}m");
            return Err(issue("invalid_place", m));
        }
    };
    let (x, z) = match written {
        Written::Xz { x, z } => (x, z),
        Written::Mgrs(sq) => to_xz(terrain, sq.lat, sq.lon)?,
        Written::LatLon { lat, lon } => to_xz(terrain, lat, lon)?,
        Written::Name { kind, name } => {
            let all = names()?;
            let key = fold(&name);
            let of_kind = || all.iter().filter(|n| n.kind == kind);
            let found: Vec<&Named> = of_kind().filter(|n| n.keys.contains(&key)).collect();
            let what = kind.prefix();
            if key.is_empty() {
                let m = format!("{quoted} is not a place: it names no {what}.\n{FORMS}");
                return Err(issue("invalid_place", m));
            }
            if found.is_empty() {
                let mut m = format!(
                    "no {what} on {} is named \"{name}\".",
                    terrain_name(terrain)
                );
                let near = closest(of_kind(), &key);
                if !near.is_empty() {
                    m += &format!("\nThe closest names: {}.", near.join(", "));
                }
                return Err(issue("unknown_place", m));
            }
            // Rows the list repeats at one position are one place.
            let mut placed: Vec<(f64, f64)> = Vec::new();
            for p in found.iter().filter_map(|n| n.at) {
                if !placed.iter().any(|q| (p.0 - q.0).hypot(p.1 - q.1) < 1.0) {
                    placed.push(p);
                }
            }
            match placed.as_slice() {
                [] => {
                    let m = format!("{} has no runway, so no position.", found[0].typed());
                    return Err(issue("invalid_place", m));
                }
                [one] => *one,
                many => {
                    let grids = mgrs_of(terrain, many)?;
                    let mut m = format!(
                        "\"{name}\" names {} {what}s on {}. Type the position of the one you mean:",
                        many.len(),
                        terrain_name(terrain)
                    );
                    for (&(x, z), g) in many.iter().zip(&grids) {
                        m += &format!("\n  {g}   {}", shown_xz(x, z));
                    }
                    return Err(issue("ambiguous_place", m));
                }
            }
        }
        Written::Bare(name) => {
            let all = names()?;
            let key = fold(&name);
            let mut typed: Vec<String> = Vec::new();
            for n in all.iter().filter(|n| n.keys.contains(&key)) {
                if !typed.contains(&n.typed()) {
                    typed.push(n.typed());
                }
            }
            let m = if typed.is_empty() {
                let mut m = format!(
                    "{quoted} is not a place: a name needs its kind, and no airport or town on {} has it.",
                    terrain_name(terrain)
                );
                let near = closest(all.iter(), &key);
                if !near.is_empty() {
                    m += &format!("\nThe closest names: {}.", near.join(", "));
                }
                m + "\n" + FORMS
            } else {
                format!("{quoted} needs a kind. Type {}.", typed.join(" or "))
            };
            return Err(issue("invalid_place", m));
        }
    };
    let extent = query::describe(terrain, DescribeRequest {})?.extent;
    if let Some(e) = extent
        && !((e.min_x..=e.max_x).contains(&x) && (e.min_z..=e.max_z).contains(&z))
    {
        return Err(issue(
            "off_terrain",
            format!(
                "{} lies off {}, which covers x {:.0} to {:.0} and z {:.0} to {:.0}.",
                text.trim(),
                terrain_name(terrain),
                e.min_x,
                e.max_x,
                e.min_z,
                e.max_z
            ),
        ));
    }
    Ok(Point { x, z })
}

/// Reads `text`, given to the flag `path`, to the airport it names on
/// `terrain`, as a request names one: its id when `text` is digits, else
/// its `name_id`. The name matches as a place's does, with or without
/// `airport:`, since the flag already says the kind.
pub fn airport(terrain: &Terrain, path: &str, text: &str) -> Result<Value, Vec<Issue>> {
    airport_among(|| names(terrain), &terrain_name(terrain), path, text)
}

/// `airport`, with the airports `names` gives.
fn airport_among(
    names: impl Fn() -> Result<Vec<Named>, Vec<Issue>>,
    terrain: &str,
    path: &str,
    text: &str,
) -> Result<Value, Vec<Issue>> {
    let issue = |code: &str, message: String| vec![Issue::new(code, &message, path)];
    let t = text.trim();
    let name = match t.split_once(':') {
        Some((k, rest)) if k.trim().eq_ignore_ascii_case("airport") => rest.trim(),
        Some((k, _)) if k.chars().all(|c| c.is_ascii_alphabetic()) => {
            let m = format!("\"{t}\" is not an airport: this flag takes airport:<name> or a name.");
            return Err(issue("invalid_place", m));
        }
        _ => t,
    };
    if !name.is_empty() && name.chars().all(|c| c.is_ascii_digit()) {
        return Ok(name.parse::<i64>().map(Value::from).unwrap_or(json!(name)));
    }
    let key = fold(name);
    if key.is_empty() {
        return Err(issue("invalid_place", format!("\"{t}\" names no airport.")));
    }
    let all = names()?;
    let airports = || all.iter().filter(|n| n.kind == Kind::Airport);
    let mut found: Vec<&str> = airports()
        .filter(|n| n.keys.contains(&key))
        .filter_map(|n| n.name_id.as_deref())
        .collect();
    found.dedup();
    match found.as_slice() {
        [one] => Ok(Value::String((*one).to_string())),
        [] => {
            let mut m = format!("no airport on {terrain} is named \"{name}\".");
            let near = closest(airports(), &key);
            if !near.is_empty() {
                m += &format!("\nThe closest names: {}.", near.join(", "));
            }
            Err(issue("unknown_place", m))
        }
        many => {
            let typed: Vec<String> = many.iter().map(|n| format!("airport:{n}")).collect();
            let m = format!(
                "\"{name}\" names {} airports on {terrain}. Type one of: {}.",
                many.len(),
                typed.join(", ")
            );
            Err(issue("ambiguous_place", m))
        }
    }
}

/// DCS `x` and `z` of a latitude and longitude, as `geo` gives them.
fn to_xz(terrain: &Terrain, lat: f64, lon: f64) -> Result<(f64, f64), Vec<Issue>> {
    let q = GeoRequest {
        to_latlon: Vec::new(),
        to_xz: vec![LatLon { lat, lon }],
    };
    let p = &query::geo(terrain, q)?.to_xz[0];
    Ok((p.x, p.z))
}

/// The MGRS of each point, as `geo` gives it.
fn mgrs_of(terrain: &Terrain, at: &[(f64, f64)]) -> Result<Vec<String>, Vec<Issue>> {
    let q = GeoRequest {
        to_latlon: at.iter().map(|&(x, z)| Point { x, z }).collect(),
        to_xz: Vec::new(),
    };
    Ok(query::geo(terrain, q)?
        .to_latlon
        .into_iter()
        .map(|g| g.mgrs.unwrap_or_else(|| "(no MGRS)".into()))
        .collect())
}

#[cfg(test)]
mod tests;
