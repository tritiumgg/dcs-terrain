//! The flags of the commands for people, generated from each operation's
//! request schema, and read into the request `query` takes.
//!
//! A person can guess a flag from the schema by these rules:
//!
//! - One subcommand per operation, its name with `_` written `-`.
//! - One flag per request field, `--` and its name with `_` written `-`;
//!   `geojson` is `--geojson-out <file>` instead.
//! - A required field is a required flag; a defaulted one is optional.
//! - `x` and `z` side by side are one place, `at`; `min_x` and `min_z` are
//!   `min`, and so on.
//! - A point with a height is one value, `<place>@<m>`: meters above the
//!   ground, or above sea level with `msl` after the number.
//! - A list takes several values after its flag, or the flag again; numbers
//!   and words may also be apart by commas.
//! - An object takes its fields as values, in the order the help lists
//!   them, or as `key=value` in any order; an optional one only as
//!   `key=value`. A nested field is `field.key=value`. A list among the
//!   fields takes every value but those the fields after it need, or the
//!   values after its key up to the next key.
//! - A field that is one of several shapes takes the shape's name first.
//! - A list of objects takes one object each time its flag is given.
//! - A map takes `key=value` pairs.
//! - Wherever a value cannot be typed as words, JSON is taken, inline or
//!   from `@<file>`.
//!
//! Reading a command line needs no terrain: places are left as written and
//! read once the terrain is open.

use serde_json::Value;

/// How a place is written into the request.
#[derive(Debug, Clone, PartialEq)]
pub enum PlaceAs {
    /// `{"x": .., "z": ..}`.
    Object,
    /// `[x, z]`.
    Pair,
    /// `x` and `z` beside the object's other fields, named with this
    /// prefix: `min_` writes `min_x` and `min_z`.
    Split(String),
}

/// What a field holds, and so how it is typed.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Number {
        integer: bool,
    },
    Bool,
    /// One word of a closed set.
    Word(Vec<String>),
    Text,
    Place(PlaceAs),
    /// A place with a height; `agl_only` when the height can only be above
    /// the ground.
    Height {
        agl_only: bool,
    },
    LatLon,
    /// An airdrome, by name or id.
    Airdrome,
    List(Box<Shape>),
    Object(Vec<Field>),
    OneOf(OneOf),
    Map(Box<Shape>),
    /// Anything else, taken as JSON.
    Json,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// The request's name for it, or for a place written split, its name
    /// here: `at`, `min`.
    pub name: String,
    pub shape: Shape,
    pub required: bool,
    pub default: Option<Value>,
    pub help: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tagging {
    /// `{"Circle": {..}}`, or `"Point"` for a shape with no fields.
    External,
    /// `{"name": "slope_max", "params": {..}}`.
    Adjacent { tag: String, content: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct OneOf {
    pub tagging: Tagging,
    pub variants: Vec<Variant>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    /// The request's name for it.
    pub tag: String,
    pub help: String,
    /// None for a shape with no fields.
    pub fields: Option<Vec<Field>>,
}

/// A command line that cannot be read, which exits 2.
#[derive(Debug, Clone, PartialEq)]
pub struct Usage(pub String);

/// A place or an airdrome as typed, to be read once the terrain is open.
#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    /// Where in the request: a JSON pointer to the value, or for a place
    /// written split, to the object it is written into.
    pub pointer: String,
    /// The flag it was given to, as messages name it.
    pub flag: String,
    pub text: String,
    pub kind: PendingKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PendingKind {
    Place(PlaceAs),
    Airdrome,
}

/// A request read from a command line, its places not yet read.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub request: Value,
    pub pending: Vec<Pending>,
}

/// The flag a field name is typed as.
pub fn kebab(name: &str) -> String {
    name.replace('_', "-")
}

/// A word as the request spells it: lower case, `-` written `_`.
fn snake(word: &str) -> String {
    word.to_ascii_lowercase().replace('-', "_")
}

/// A name folded for comparing: case, `-` and `_` set aside.
fn folded(word: &str) -> String {
    word.chars()
        .filter(|c| *c != '-' && *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

/// The word a shape is typed as: `AirdromeBand` and `slope_max` are
/// `airdrome-band` and `slope-max`.
fn word(tag: &str) -> String {
    let mut out = String::new();
    for (i, c) in tag.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('-');
        }
        out.push(if c == '_' {
            '-'
        } else {
            c.to_ascii_lowercase()
        });
    }
    out
}

/// The edits that turn `a` into `b`.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == *cb { prev } else { prev + 1 };
            prev = row[j + 1];
            row[j + 1] = cost.min(row[j] + 1).min(prev + 1);
        }
    }
    row[b.len()]
}

/// The one of `names` closest to `typed`, if any is close enough to be a
/// slip.
pub fn closest<'a>(typed: &str, names: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    names
        .map(|n| (distance(typed, n), n))
        .filter(|(d, n)| *d <= (n.len() / 3).max(2) && *d < typed.chars().count())
        .min()
        .map(|(_, n)| n)
}

/// " Did you mean `x`?" when one is close.
fn suggest<'a>(typed: &str, names: impl Iterator<Item = &'a str>, quote: &str) -> String {
    closest(typed, names)
        .map(|n| format!(" Did you mean `{quote}{n}`?"))
        .unwrap_or_default()
}

// Reading the schema.

/// The schema node a `$ref` names, or the node itself.
fn deref<'a>(node: &'a Value, defs: &'a Value) -> (&'a Value, Option<&'a str>) {
    match node.get("$ref").and_then(Value::as_str) {
        Some(r) => {
            let name = r.trim_start_matches("#/$defs/");
            (&defs[name], Some(name))
        }
        None => (node, None),
    }
}

fn describe(node: &Value) -> String {
    node.get("description")
        .and_then(Value::as_str)
        .map(|d| d.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

/// The shape of `node`, and whether it may be null.
fn shape_of(node: &Value, defs: &Value) -> (Shape, bool) {
    if let Some(Value::Array(any)) = node.get("anyOf") {
        let null = any.iter().any(|v| v["type"] == "null");
        let rest: Vec<&Value> = any.iter().filter(|v| v["type"] != "null").collect();
        if let [one] = rest.as_slice() {
            return (shape_of(one, defs).0, null);
        }
        return (Shape::Json, null);
    }
    let (node, name) = deref(node, defs);
    match name {
        Some("Point") => return (Shape::Place(PlaceAs::Object), false),
        Some("Point3") => return (Shape::Height { agl_only: false }, false),
        Some("Observer") => return (Shape::Height { agl_only: true }, false),
        Some("LatLon") => return (Shape::LatLon, false),
        Some("AirdromeRef") => return (Shape::Airdrome, false),
        _ => {}
    }
    if let Some(Value::Array(words)) = node.get("enum") {
        let words = words.iter().filter_map(Value::as_str).map(String::from);
        return (Shape::Word(words.collect()), false);
    }
    if let Some(Value::Array(shapes)) = node.get("oneOf") {
        return (one_of(shapes, defs), false);
    }
    let (kind, null) = match &node["type"] {
        Value::String(t) => (t.as_str(), false),
        Value::Array(ts) => {
            let null = ts.iter().any(|t| t == "null");
            let t = ts.iter().filter_map(Value::as_str).find(|t| *t != "null");
            (t.unwrap_or(""), null)
        }
        _ => ("", false),
    };
    let shape = match kind {
        "number" => Shape::Number { integer: false },
        "integer" => Shape::Number { integer: true },
        "boolean" => Shape::Bool,
        "string" => Shape::Text,
        "array" => {
            let items = &node["items"];
            let pair = items["type"] == "array" && items["minItems"] == 2 && items["maxItems"] == 2;
            if pair {
                Shape::List(Box::new(Shape::Place(PlaceAs::Pair)))
            } else {
                Shape::List(Box::new(shape_of(items, defs).0))
            }
        }
        "object" => match (node.get("properties"), node.get("additionalProperties")) {
            (Some(_), _) => Shape::Object(fields_of(node, defs)),
            (None, Some(inner)) if inner.is_object() => {
                Shape::Map(Box::new(shape_of(inner, defs).0))
            }
            _ => Shape::Json,
        },
        _ => Shape::Json,
    };
    (shape, null)
}

/// A field that is one of several shapes.
fn one_of(shapes: &[Value], defs: &Value) -> Shape {
    let word = |s: &Value| {
        s.get("const")
            .or_else(|| s.get("enum").and_then(|e| e.get(0)))
            .and_then(Value::as_str)
            .map(String::from)
    };
    let objects = shapes.iter().any(|s| s.get("properties").is_some());
    if !objects {
        let words: Option<Vec<String>> = shapes.iter().map(word).collect();
        return words.map(Shape::Word).unwrap_or(Shape::Json);
    }
    // Every shape names itself in a field holding a constant: adjacent.
    let tag = shapes.iter().find_map(|s| {
        let props = s.get("properties")?.as_object()?;
        props
            .iter()
            .find(|(_, v)| v.get("const").is_some())
            .map(|(k, _)| k.clone())
    });
    let mut variants = Vec::new();
    match tag {
        Some(tag) => {
            let mut content = String::new();
            for s in shapes {
                let props = s["properties"].as_object().cloned().unwrap_or_default();
                let name = props.get(&tag).and_then(word).unwrap_or_else(|| "?".into());
                let inner = props.iter().find(|(k, _)| **k != tag);
                let fields = inner.map(|(k, v)| {
                    content = k.clone();
                    fields_of(deref(v, defs).0, defs)
                });
                variants.push(Variant {
                    tag: name,
                    help: describe(s),
                    fields,
                });
            }
            Shape::OneOf(OneOf {
                tagging: Tagging::Adjacent { tag, content },
                variants,
            })
        }
        None => {
            for s in shapes {
                if let Some(w) = word(s) {
                    variants.push(Variant {
                        tag: w,
                        help: describe(s),
                        fields: None,
                    });
                    continue;
                }
                let Some((name, inner)) = s["properties"].as_object().and_then(|p| p.iter().next())
                else {
                    return Shape::Json;
                };
                let (inner, _) = deref(inner, defs);
                let help = [describe(s), describe(inner)]
                    .into_iter()
                    .find(|h| !h.is_empty())
                    .unwrap_or_default();
                variants.push(Variant {
                    tag: name.clone(),
                    help,
                    fields: Some(fields_of(inner, defs)),
                });
            }
            Shape::OneOf(OneOf {
                tagging: Tagging::External,
                variants,
            })
        }
    }
}

/// The fields of an object: the required ones in the schema's order, then
/// the rest by name, with each `x` and `z` pair made one place.
pub fn fields_of(node: &Value, defs: &Value) -> Vec<Field> {
    let props = node["properties"].as_object().cloned().unwrap_or_default();
    let required: Vec<&str> = node["required"]
        .as_array()
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut order: Vec<&String> = Vec::new();
    for r in &required {
        if let Some((k, _)) = props.get_key_value(*r) {
            order.push(k);
        }
    }
    for k in props.keys() {
        if !order.contains(&k) {
            order.push(k);
        }
    }
    let mut fields = Vec::new();
    for k in order {
        let node = &props[k];
        // `x` takes its `z` with it; `z` alone is then already taken.
        let prefix = k.strip_suffix('x');
        let partner = prefix.map(|p| format!("{p}z"));
        let both_numbers = |a: &Value, b: &Value| {
            let n = |v: &Value| matches!(shape_of(v, defs).0, Shape::Number { integer: false });
            n(a) && n(b)
        };
        if let (Some(prefix), Some(partner)) = (prefix, &partner)
            && (prefix.is_empty() || prefix.ends_with('_'))
            && let Some(z) = props.get(partner)
            && both_numbers(node, z)
        {
            let name = match prefix.strip_suffix('_') {
                Some(p) => p.to_string(),
                None => "at".to_string(),
            };
            let help = describe(node);
            let help = match help.split_once(':') {
                Some((head, _)) => format!("{head}."),
                None => "The place.".into(),
            };
            fields.push(Field {
                name,
                shape: Shape::Place(PlaceAs::Split(prefix.to_string())),
                required: required.contains(&k.as_str()),
                default: None,
                help,
            });
            continue;
        }
        if k.ends_with('z')
            && let Some(p) = k.strip_suffix('z')
            && (p.is_empty() || p.ends_with('_'))
            && props.contains_key(&format!("{p}x"))
            && fields
                .iter()
                .any(|f: &Field| f.shape == Shape::Place(PlaceAs::Split(p.to_string())))
        {
            continue;
        }
        let (shape, _) = shape_of(node, defs);
        fields.push(Field {
            name: k.clone(),
            shape,
            required: required.contains(&k.as_str()),
            default: node.get("default").cloned(),
            help: describe(node),
        });
    }
    fields
}

/// One operation's flags.
#[derive(Debug, Clone)]
pub struct Spec {
    pub op: String,
    pub summary: String,
    /// The description beyond its first sentence or paragraph.
    pub more: String,
    pub fields: Vec<Field>,
    /// Whether it takes `--geojson-out`.
    pub geojson: bool,
}

impl Spec {
    /// The flags of operation `op`, from its request schema.
    pub fn of(op: &str) -> Option<Spec> {
        let root = Value::Object(dcsterrain_core::types::request_schema(op).ok()?);
        let defs = root.get("$defs").cloned().unwrap_or(Value::Null);
        let geojson = root["properties"].get("geojson").is_some();
        let mut fields = fields_of(&root, &defs);
        fields.retain(|f| f.name != "geojson");
        let text = root
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let (summary, more) = text.split_once("\n\n").unwrap_or((text, ""));
        Some(Spec {
            op: op.to_string(),
            summary: summary.split_whitespace().collect::<Vec<_>>().join(" "),
            more: more.split_whitespace().collect::<Vec<_>>().join(" "),
            fields,
            geojson,
        })
    }

    pub fn command(&self) -> String {
        kebab(&self.op)
    }
}

mod help;
mod read;

#[cfg(test)]
mod tests;
