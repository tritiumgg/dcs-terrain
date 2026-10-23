//! What an install says about itself: the DCS build, and a map's theatre id.

use std::fmt;
use std::fs;
use std::path::Path;

use serde_json::Value;

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
pub fn entry_id(text: &str) -> Option<String> {
    let b = text.as_bytes();
    // (depth, bare, position, id): the least wins.
    let mut best: Option<(i64, bool, usize, String)> = None;
    let mut depth = 0i64;
    let mut counted = 0;
    for i in 0..b.len() {
        if !b[i..].starts_with(b"id") {
            continue;
        }
        let found = bracketed(b, i)
            .map(|(at, id)| (at, false, id))
            .or_else(|| bare(b, i).map(|id| (i, true, id)));
        let Some((at, is_bare, id)) = found else {
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
        let cand = (depth, is_bare, at, id);
        if best.as_ref().is_none_or(|b| (cand.0, cand.1) < (b.0, b.1)) {
            best = Some(cand);
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

/// `= "value"` or `= 'value'` from `i`, after optional space.
fn assigned(b: &[u8], i: usize) -> Option<String> {
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
    String::from_utf8(b[i + 1..i + 1 + len].to_vec()).ok()
}

/// `['id'] = "..."` with `id` at `i`; returns the `[` position and the value.
fn bracketed(b: &[u8], i: usize) -> Option<(usize, String)> {
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
    assigned(b, close + 1).map(|id| (open, id))
}

/// `id = "..."` with `id` at `i` and no word character before it.
fn bare(b: &[u8], i: usize) -> Option<String> {
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    if i > 0 && word(b[i - 1]) {
        return None;
    }
    assigned(b, i + 2)
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
}
