//! The help of each command, written from its request's descriptions.

use std::fmt::Write as _;

use serde_json::Value;

use super::read::order_of;
use super::*;

/// How a value of `shape` is written, for the help.
pub fn typed(shape: &Shape) -> String {
    match shape {
        Shape::Number { integer: true } => "<n>".into(),
        Shape::Number { integer: false } => "<number>".into(),
        Shape::Bool => String::new(),
        Shape::Word(words) => words.join("|"),
        Shape::Text => "<text>".into(),
        Shape::Place(_) => "<place>".into(),
        Shape::Height { agl_only: true } => "<place>@<m>".into(),
        Shape::Height { agl_only: false } => "<place>@<m>[msl]".into(),
        Shape::LatLon => "<lat>,<lon>".into(),
        Shape::Airdrome => "<airport>".into(),
        Shape::List(inner) => match **inner {
            Shape::Object(_) | Shape::OneOf(_) => typed(inner),
            _ => format!("{}...", typed(inner)),
        },
        Shape::Object(fields) => order_of(fields),
        Shape::OneOf(_) => "<shape> <values>...".into(),
        Shape::Map(_) => "<name>=<value>...".into(),
        Shape::Json => "<json>|@<file>".into(),
    }
}

/// `text` wrapped to `width`, each line after the first indented by
/// `indent`.
fn wrapped(text: &str, indent: usize, width: usize) -> String {
    let mut out = String::new();
    let mut line = 0;
    for word in text.split_whitespace() {
        if line > 0 && line + 1 + word.len() > width.saturating_sub(indent) {
            out.push('\n');
            out.push_str(&" ".repeat(indent));
            line = 0;
        } else if line > 0 {
            out.push(' ');
            line += 1;
        }
        out.push_str(word);
        line += word.len();
    }
    out
}

/// One flag's lines in the help.
fn flag_line(out: &mut String, left: &str, help: &str) {
    const COLUMN: usize = 30;
    const WIDTH: usize = 100;
    if left.len() + 4 > COLUMN {
        let _ = writeln!(out, "  {left}");
        let _ = writeln!(
            out,
            "{}{}",
            " ".repeat(COLUMN),
            wrapped(help, COLUMN, WIDTH)
        );
    } else {
        let _ = writeln!(
            out,
            "  {left:<w$}{}",
            wrapped(help, COLUMN, WIDTH),
            w = COLUMN - 2
        );
    }
}

/// A field's help with its default.
fn field_help(f: &Field) -> String {
    let mut h = f.help.clone();
    if f.required {
        h.push_str(" Required.");
    } else if let Some(d) = &f.default {
        let shown = match d {
            Value::Array(a) if a.is_empty() => None,
            Value::Object(m) if m.is_empty() => None,
            Value::Bool(false) => None,
            Value::Array(a) => Some(a.iter().map(shown_number).collect::<Vec<_>>().join(" ")),
            Value::String(s) => Some(s.clone()),
            v => Some(shown_number(v)),
        };
        if let Some(s) = shown {
            let _ = write!(h, " [default: {s}]");
        }
    }
    h
}

/// A number as a person writes it: `8000`, not `8000.0`.
fn shown_number(v: &Value) -> String {
    match v.as_f64() {
        Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", f as i64),
        _ => match v {
            Value::String(s) => s.clone(),
            v => v.to_string(),
        },
    }
}

/// What each field that is one of several shapes, or an object, takes.
fn shapes_section(out: &mut String, f: &Field) {
    let (list, o) = match &f.shape {
        Shape::OneOf(o) => (false, o),
        Shape::List(inner) => match &**inner {
            Shape::OneOf(o) => (true, o),
            _ => return,
        },
        _ => return,
    };
    let _ = writeln!(
        out,
        "\n--{} takes one of these{}:",
        kebab(&f.name),
        if list { ", and may be given again" } else { "" }
    );
    for v in &o.variants {
        let name = word(&v.tag);
        let left = match &v.fields {
            Some(fields) if !fields.is_empty() => format!("{name} {}", order_of(fields)),
            _ => name,
        };
        flag_line(out, &left, &v.help);
    }
}

/// An object's fields, for a flag that takes one.
fn object_section(out: &mut String, f: &Field) {
    let fields = match &f.shape {
        Shape::Object(fields) => fields,
        Shape::List(inner) => match &**inner {
            Shape::Object(fields) => fields,
            _ => return,
        },
        _ => return,
    };
    let _ = writeln!(out, "\n--{} takes {}:", kebab(&f.name), order_of(fields));
    for g in fields {
        let left = format!("{} {}", kebab(&g.name), typed(&g.shape));
        flag_line(out, left.trim_end(), &field_help(g));
    }
}

/// The shared flags, as the help lists them.
pub const SHARED: &[(&str, &str)] = &[
    (
        "--terrain <id>",
        "The terrain's id, as `list-terrains` shows it, or `synth` for the synthetic terrain, which needs no install. Required.",
    ),
    (
        "--install <dir>",
        "The DCS install root. On Windows it defaults to the one the registry names.",
    ),
    (
        "--cache <dir>",
        "The cache directory; the user's cache location by default.",
    ),
    (
        "--allow-crs-residual",
        "Accept a projection fitted to the terrain's beacons that misses one by more than 2 m.",
    ),
    ("--seed <n>", "The synthetic terrain's seed. [default: 1]"),
    (
        "--size-km <n>",
        "The synthetic terrain's side, in kilometers. [default: 70]",
    ),
    (
        "--cell <n>",
        "The synthetic terrain's cell, in meters. [default: 50]",
    ),
    (
        "--threads <n>",
        "The most threads the work spreads over; every core when omitted.",
    ),
    ("--verbose", "More progress and diagnostics on stderr."),
];

impl Spec {
    /// The `--help` text.
    pub fn help(&self, example: Option<&str>) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "{}", self.summary);
        if !self.more.is_empty() {
            let _ = writeln!(out, "\n{}", wrapped(&self.more, 0, 100));
        }
        let required: Vec<String> = self
            .fields
            .iter()
            .filter(|f| f.required)
            .map(|f| {
                let value = match &f.shape {
                    Shape::Object(_) | Shape::OneOf(_) | Shape::List(_) | Shape::Map(_) => {
                        "<...>".to_string()
                    }
                    s => typed(s),
                };
                format!("--{} {value}", kebab(&f.name))
            })
            .collect();
        let usage = format!(
            "dcsterrain {} --terrain <id> {}[options]",
            self.command(),
            required.iter().map(|r| format!("{r} ")).collect::<String>()
        );
        let _ = writeln!(out, "\nUsage: {}", wrapped(&usage, 9, 100));
        if !self.fields.is_empty() {
            let _ = writeln!(out, "\nRequest:");
            for f in &self.fields {
                let left = format!("--{} {}", kebab(&f.name), typed(&f.shape));
                flag_line(&mut out, left.trim_end(), &field_help(f));
            }
        }
        let _ = writeln!(out, "\nOutput:");
        flag_line(
            &mut out,
            "--json",
            "Print the answer as `query` prints it, byte for byte.",
        );
        if self.geojson {
            flag_line(
                &mut out,
                "--geojson-out <file>",
                "Write the answer's GeoJSON to <file>, for any map program.",
            );
        }
        flag_line(
            &mut out,
            "--show-request",
            "Print the JSON request the flags make, for `query`, and stop.",
        );
        let _ = writeln!(out, "\nTerrain:");
        for (left, help) in SHARED {
            flag_line(&mut out, left, help);
        }
        for f in &self.fields {
            shapes_section(&mut out, f);
            object_section(&mut out, f);
        }
        let _ = writeln!(
            out,
            "\nA <place> is DCS x,z in meters (-284887,683859), MGRS (38T KM 91980 72567),\n\
             latitude and longitude (42.177616N 42.481292E), airport:<name> or town:<name>.\n\
             A height follows it after `@`: meters above the ground, or above sea level\n\
             with `msl` (airport:Kutaisi@10, town:Zugdidi@1200msl).\n\
             An object's values go in the order shown, or as key=value in any order."
        );
        if let Some(e) = example {
            let _ = writeln!(out, "\nExample:\n  {e}");
        }
        out
    }
}

#[cfg(test)]
mod tests;
