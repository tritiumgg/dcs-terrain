//! A command line read into a request: each flag's values by the shape of
//! the field it fills.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use super::*;

impl Spec {
    /// Reads the request from its flags' occurrences, in the order given.
    pub fn read(&self, given: &[(String, Vec<String>)]) -> Result<Draft, Usage> {
        let mut request = Map::new();
        let mut pending = Vec::new();
        for f in &self.fields {
            let flag = format!("--{}", kebab(&f.name));
            let mine: Vec<&Vec<String>> = given
                .iter()
                .filter(|(n, _)| *n == kebab(&f.name))
                .map(|(_, v)| v)
                .collect();
            if mine.is_empty() {
                if f.required {
                    return Err(Usage(format!("{flag} is required: {}", f.help)));
                }
                continue;
            }
            let mut r = Reader {
                pending: &mut pending,
            };
            let at = format!("/{}", f.name);
            match &f.shape {
                Shape::Bool => {
                    if mine.iter().any(|v| !v.is_empty()) {
                        return Err(Usage(format!("{flag} takes no value")));
                    }
                    request.insert(f.name.clone(), Value::Bool(true));
                }
                Shape::Place(PlaceAs::Split(prefix)) => {
                    let text = one(&flag, &mine)?;
                    r.place(&flag, "", text, PlaceAs::Split(prefix.clone()));
                }
                Shape::List(inner) => {
                    let mut items = Vec::new();
                    let objects = matches!(**inner, Shape::Object(_) | Shape::OneOf(_));
                    if objects {
                        for (i, values) in mine.iter().enumerate() {
                            let at = format!("{at}/{}", items.len());
                            if let [only] = values.as_slice()
                                && let Some(json) = json_value(&flag, only)?
                            {
                                if let Value::Array(a) = json {
                                    items.extend(a);
                                } else {
                                    items.push(json);
                                }
                                continue;
                            }
                            let label = format!("{flag} #{}", i + 1);
                            items.push(r.value(inner, values, &label, &at)?);
                        }
                    } else {
                        for values in &mine {
                            for v in values.iter() {
                                for piece in pieces(inner, v) {
                                    let at = format!("{at}/{}", items.len());
                                    items.push(r.value(inner, &[piece], &flag, &at)?);
                                }
                            }
                        }
                    }
                    request.insert(f.name.clone(), Value::Array(items));
                }
                Shape::Map(_) => {
                    let values: Vec<String> = mine.iter().flat_map(|v| v.iter().cloned()).collect();
                    request.insert(f.name.clone(), r.value(&f.shape, &values, &flag, &at)?);
                }
                _ => {
                    if mine.len() > 1 {
                        return Err(Usage(format!("{flag} is given more than once")));
                    }
                    let v = r.value(&f.shape, mine[0], &flag, &at)?;
                    request.insert(f.name.clone(), v);
                }
            }
        }
        Ok(Draft {
            request: Value::Object(request),
            pending,
        })
    }
}

/// The single value of a flag given once.
fn one<'a>(flag: &str, given: &[&'a Vec<String>]) -> Result<&'a str, Usage> {
    match given {
        [values] => match values.as_slice() {
            [v] => Ok(v),
            [] => Err(Usage(format!("{flag} needs a value"))),
            _ => Err(Usage(format!("{flag} takes one value"))),
        },
        _ => Err(Usage(format!("{flag} is given more than once"))),
    }
}

/// A value written for a list of numbers or words, split at its commas.
fn pieces(shape: &Shape, v: &str) -> Vec<String> {
    match shape {
        Shape::Number { .. } | Shape::Word(_) => v
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(String::from)
            .collect(),
        _ => vec![v.to_string()],
    }
}

/// `text` as JSON when it is written as JSON, or as `@<file>`.
fn json_value(flag: &str, text: &str) -> Result<Option<Value>, Usage> {
    let source = if let Some(path) = text.strip_prefix('@') {
        std::fs::read_to_string(path)
            .map_err(|e| Usage(format!("{flag}: {path} cannot be read: {e}")))?
    } else if text.starts_with(['{', '[', '"']) {
        text.to_string()
    } else {
        return Ok(None);
    };
    serde_json::from_str(&source)
        .map(Some)
        .map_err(|e| Usage(format!("{flag}: the JSON cannot be read: {e}")))
}

fn number(flag: &str, text: &str, integer: bool) -> Result<Value, Usage> {
    let t = text.trim();
    if integer {
        return t.parse::<u64>().map(Value::from).map_err(|_| {
            Usage(format!(
                "{flag} takes a whole number, and \"{text}\" is not one"
            ))
        });
    }
    match t.parse::<f64>() {
        Ok(v) if v.is_finite() => Ok(json!(v)),
        _ => Err(Usage(format!(
            "{flag} takes a number, and \"{text}\" is not one"
        ))),
    }
}

/// A height written after `@`: meters above the ground, or above sea level
/// with `msl`. Returns the request's name for it and the number.
fn height(flag: &str, text: &str, agl_only: bool) -> Result<(&'static str, f64), Usage> {
    let t = text.trim().to_ascii_lowercase();
    let (n, key) = if let Some(n) = t.strip_suffix("msl") {
        if agl_only {
            return Err(Usage(format!(
                "{flag}: this height is above the ground; write it without `msl`"
            )));
        }
        (n, "alt")
    } else {
        (t.strip_suffix("agl").unwrap_or(&t), "alt_agl")
    };
    let n = n.trim().strip_suffix('m').unwrap_or(n.trim());
    match n.parse::<f64>() {
        Ok(v) if v.is_finite() => Ok((key, v)),
        _ => Err(Usage(format!(
            "{flag}: \"{text}\" is not a height. Write meters above the ground, as \
             `@10`{}",
            if agl_only {
                ""
            } else {
                ", or above sea level, as `@1200msl`"
            }
        ))),
    }
}

/// Where places met on the command line are noted.
struct Reader<'a> {
    pending: &'a mut Vec<Pending>,
}

impl Reader<'_> {
    fn place(&mut self, flag: &str, pointer: &str, text: &str, kind: PlaceAs) {
        self.pending.push(Pending {
            pointer: pointer.to_string(),
            flag: flag.to_string(),
            text: text.to_string(),
            kind: PendingKind::Place(kind),
        });
    }

    /// The value `shape` takes from `values`, written at `pointer`.
    fn value(
        &mut self,
        shape: &Shape,
        values: &[String],
        flag: &str,
        pointer: &str,
    ) -> Result<Value, Usage> {
        let single = |values: &[String]| match values {
            [v] => Ok(v.clone()),
            [] => Err(Usage(format!("{flag} needs a value"))),
            _ => Err(Usage(format!(
                "{flag} takes one value, and got {}",
                values.join(" ")
            ))),
        };
        match shape {
            Shape::Number { integer } => number(flag, &single(values)?, *integer),
            Shape::Bool => {
                let v = single(values)?;
                match v.to_ascii_lowercase().as_str() {
                    "true" | "yes" => Ok(Value::Bool(true)),
                    "false" | "no" => Ok(Value::Bool(false)),
                    _ => Err(Usage(format!("{flag} is true or false, not \"{v}\""))),
                }
            }
            Shape::Word(words) => {
                let v = single(values)?;
                let found = words.iter().find(|w| folded(w) == folded(&v));
                match found {
                    Some(w) => Ok(Value::String(w.clone())),
                    None => Err(Usage(format!(
                        "{flag} is one of {}, not \"{v}\".{}",
                        words.join(", "),
                        suggest(&snake(&v), words.iter().map(String::as_str), "")
                    ))),
                }
            }
            Shape::Text => Ok(Value::String(single(values)?)),
            Shape::Json => {
                let v = single(values)?;
                match json_value(flag, &v)? {
                    Some(j) => Ok(j),
                    None => Err(Usage(format!("{flag} takes JSON, inline or as @<file>"))),
                }
            }
            Shape::Place(kind) => {
                let v = single(values)?;
                self.place(flag, pointer, &v, kind.clone());
                Ok(Value::Null)
            }
            Shape::Airdrome => {
                let v = single(values)?;
                self.pending.push(Pending {
                    pointer: pointer.to_string(),
                    flag: flag.to_string(),
                    text: v,
                    kind: PendingKind::Airdrome,
                });
                Ok(Value::Null)
            }
            Shape::Height { agl_only } => {
                let v = single(values)?;
                let Some((at, h)) = v.rsplit_once('@') else {
                    return Err(Usage(format!(
                        "{flag}: \"{v}\" needs a height. Write it after `@`: \
                         `{v}@10` is 10 m above the ground"
                    )));
                };
                let (key, h) = height(flag, h, *agl_only)?;
                self.place(flag, pointer, at, PlaceAs::Split(String::new()));
                Ok(json!({ key: h }))
            }
            Shape::LatLon => {
                let v = single(values)?;
                latlon(&v).ok_or_else(|| {
                    Usage(format!(
                        "{flag}: \"{v}\" is not a latitude and longitude. Write \
                         `42.18,42.48`, or `42.18N 42.48E`"
                    ))
                })
            }
            Shape::List(inner) => {
                let mut items = Vec::new();
                for v in values {
                    for piece in pieces(inner, v) {
                        let at = format!("{pointer}/{}", items.len());
                        items.push(self.value(inner, &[piece], flag, &at)?);
                    }
                }
                Ok(Value::Array(items))
            }
            Shape::Map(inner) => {
                let mut map = Map::new();
                for v in values {
                    let Some((k, x)) = v.split_once('=') else {
                        return Err(Usage(format!(
                            "{flag} takes `name=value` pairs, and \"{v}\" is not one"
                        )));
                    };
                    let at = format!("{pointer}/{}", snake(k));
                    let label = format!("{flag} {k}");
                    map.insert(snake(k), self.value(inner, &[x.to_string()], &label, &at)?);
                }
                Ok(Value::Object(map))
            }
            Shape::Object(fields) => self.object(fields, values, flag, pointer),
            Shape::OneOf(o) => self.one_of(o, values, flag, pointer),
        }
    }

    fn one_of(
        &mut self,
        o: &OneOf,
        values: &[String],
        flag: &str,
        pointer: &str,
    ) -> Result<Value, Usage> {
        let names = || o.variants.iter().map(|v| word(&v.tag));
        let Some(first) = values.first() else {
            let list: Vec<String> = names().collect();
            return Err(Usage(format!("{flag} needs one of {}", list.join(", "))));
        };
        if values.len() == 1
            && let Some(j) = json_value(flag, first)?
        {
            return Ok(j);
        }
        let Some(variant) = o.variants.iter().find(|v| folded(&v.tag) == folded(first)) else {
            let list: Vec<String> = names().collect();
            let typed = kebab(&snake(first));
            return Err(Usage(format!(
                "{flag} is one of {}, not \"{first}\".{}",
                list.join(", "),
                suggest(&typed, list.iter().map(String::as_str), "")
            )));
        };
        let rest = &values[1..];
        let label = format!("{flag} {}", word(&variant.tag));
        let inner = match (&variant.fields, &o.tagging) {
            (None, _) => {
                if !rest.is_empty() {
                    return Err(Usage(format!("{label} takes no values")));
                }
                None
            }
            (Some(fields), Tagging::External) => {
                let at = format!("{pointer}/{}", variant.tag);
                Some(self.object(fields, rest, &label, &at)?)
            }
            (Some(fields), Tagging::Adjacent { content, .. }) => {
                let at = format!("{pointer}/{content}");
                Some(self.object(fields, rest, &label, &at)?)
            }
        };
        Ok(match (&o.tagging, inner) {
            (Tagging::External, None) => Value::String(variant.tag.clone()),
            (Tagging::External, Some(v)) => json!({ variant.tag.clone(): v }),
            (Tagging::Adjacent { tag, .. }, None) => json!({ tag.clone(): variant.tag }),
            (Tagging::Adjacent { tag, content }, Some(v)) => {
                json!({ tag.clone(): variant.tag, content.clone(): v })
            }
        })
    }

    /// An object from its values: in order, or `key=value`.
    fn object(
        &mut self,
        fields: &[Field],
        values: &[String],
        flag: &str,
        pointer: &str,
    ) -> Result<Value, Usage> {
        let key_of = |v: &str| -> Option<(usize, Option<String>, String)> {
            let (k, x) = v.split_once('=')?;
            let (head, tail) = match k.split_once('.') {
                Some((h, t)) => (h, Some(t.to_string())),
                None => (k, None),
            };
            let i = fields
                .iter()
                .position(|f| folded(&f.name) == folded(head))?;
            Some((i, tail, x.to_string()))
        };
        // Each field's values as written: bare in order, or by key.
        let mut given: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        let mut bare = Vec::new();
        for v in values {
            if let Some((i, tail, x)) = key_of(v) {
                let entry = given.entry(i).or_default();
                match tail {
                    Some(t) => entry.push(format!("{t}={x}")),
                    None => entry.extend(pieces(&fields[i].shape, &x)),
                }
                continue;
            }
            // A switch's own name sets it.
            if let Some(i) = fields
                .iter()
                .position(|f| f.shape == Shape::Bool && folded(&f.name) == folded(v))
            {
                given.entry(i).or_default().push("true".into());
                continue;
            }
            if v.contains('=') && !v.starts_with(['{', '[']) {
                let (k, _) = v.split_once('=').unwrap_or_default();
                let names: Vec<String> = fields.iter().map(|f| kebab(&f.name)).collect();
                return Err(Usage(format!(
                    "{flag} has no `{k}`; it takes {}.{}",
                    names.join(", "),
                    suggest(&kebab(&snake(k)), names.iter().map(String::as_str), "")
                )));
            }
            bare.push(v.clone());
        }
        // Bare values fill the fields one value can be, in order; a list
        // last among them takes all that are left.
        let slots: Vec<usize> = fields
            .iter()
            .enumerate()
            .filter(|(_, f)| f.required && takes_bare(&f.shape))
            .map(|(i, _)| i)
            .collect();
        let mut bare = bare.into_iter();
        for (n, &i) in slots.iter().enumerate() {
            if given.contains_key(&i) {
                continue;
            }
            let last = n + 1 == slots.len();
            let entry = given.entry(i).or_default();
            match &fields[i].shape {
                Shape::List(inner) if last => {
                    for v in bare.by_ref() {
                        entry.extend(pieces(inner, &v));
                    }
                }
                Shape::List(inner) => {
                    if let Some(v) = bare.next() {
                        entry.extend(pieces(inner, &v));
                    }
                }
                _ => entry.extend(bare.next()),
            }
            if entry.is_empty() {
                given.remove(&i);
            }
        }
        let extra: Vec<String> = bare.collect();
        if !extra.is_empty() {
            return Err(Usage(format!(
                "{flag} got more values than it takes: {}. It takes {}",
                extra.join(" "),
                order_of(fields)
            )));
        }
        let mut out = Map::new();
        for (i, f) in fields.iter().enumerate() {
            let label = format!("{flag} {}", kebab(&f.name));
            let Some(values) = given.remove(&i) else {
                if f.required {
                    return Err(Usage(format!(
                        "{flag} needs `{}`: {}. It takes {}",
                        kebab(&f.name),
                        f.help.trim_end_matches('.'),
                        order_of(fields)
                    )));
                }
                continue;
            };
            match &f.shape {
                Shape::Place(PlaceAs::Split(prefix)) => {
                    let [text] = values.as_slice() else {
                        return Err(Usage(format!("{label} takes one place")));
                    };
                    self.place(&label, pointer, text, PlaceAs::Split(prefix.clone()));
                }
                Shape::Object(_) | Shape::OneOf(_) => {
                    // `field=v` holds JSON, or values apart by commas.
                    let mut parts = Vec::new();
                    for v in &values {
                        if v.contains('=') || v.starts_with(['{', '[', '"', '@']) {
                            parts.push(v.clone());
                        } else {
                            parts.extend(v.split(',').map(String::from));
                        }
                    }
                    let at = format!("{pointer}/{}", f.name);
                    out.insert(f.name.clone(), self.value(&f.shape, &parts, &label, &at)?);
                }
                _ => {
                    let at = format!("{pointer}/{}", f.name);
                    out.insert(f.name.clone(), self.value(&f.shape, &values, &label, &at)?);
                }
            }
        }
        Ok(Value::Object(out))
    }
}

/// Whether a field can take a value written bare, in order.
fn takes_bare(shape: &Shape) -> bool {
    match shape {
        Shape::Object(_) | Shape::OneOf(_) | Shape::Map(_) | Shape::Json | Shape::Bool => false,
        Shape::List(inner) => takes_bare(inner),
        _ => true,
    }
}

/// How an object's values are written, for a message: its bare values in
/// order, then its keys.
pub(super) fn order_of(fields: &[Field]) -> String {
    let mut words = Vec::new();
    for f in fields {
        let k = kebab(&f.name);
        if f.required && takes_bare(&f.shape) {
            let more = if matches!(f.shape, Shape::List(_)) {
                "..."
            } else {
                ""
            };
            words.push(format!("<{k}>{more}"));
        } else {
            words.push(format!("{k}="));
        }
    }
    words.join(" ")
}

/// A latitude and longitude written `lat,lon`, or as a place writes one.
fn latlon(text: &str) -> Option<Value> {
    if let Some((a, b)) = text.split_once(',')
        && let (Ok(lat), Ok(lon)) = (a.trim().parse::<f64>(), b.trim().parse::<f64>())
    {
        return Some(json!({"lat": lat, "lon": lon}));
    }
    match crate::place::read(text) {
        Ok(crate::place::Written::LatLon { lat, lon }) => Some(json!({"lat": lat, "lon": lon})),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
