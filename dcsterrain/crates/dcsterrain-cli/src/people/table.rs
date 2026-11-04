//! The one table every readable answer is printed through: columns sized
//! to their widest cell, numbers aligned on the right, and a page never
//! wider than `WIDTH`. A column that cannot fit is cut short or left out,
//! and the page says which.

use std::fmt::Write as _;
use std::io::IsTerminal as _;

/// The widest a page is, in characters: a terminal's 80 with room for the
/// MGRS a place carries beside its x and z.
pub const WIDTH: usize = 100;

/// The space between two columns.
const GAP: &str = "  ";

/// Whether color is wanted: only on a terminal, and never under `NO_COLOR`.
pub fn color_wanted() -> bool {
    std::io::stdout().is_terminal() && !anstyle_query::no_color() && ansi_works()
}

#[cfg(windows)]
fn ansi_works() -> bool {
    anstyle_query::windows::enable_ansi_colors().unwrap_or(false)
}

#[cfg(not(windows))]
fn ansi_works() -> bool {
    anstyle_query::term_supports_ansi_color()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

#[derive(Debug, Clone)]
struct Column {
    header: String,
    align: Align,
    /// The fewest characters it may be cut to, with an ellipsis; none when
    /// it is never cut.
    min: Option<usize>,
    /// Columns are left out lowest first when the page is too narrow; one
    /// at `KEEP` never is.
    keep: u8,
}

/// The rank of a column never left out.
pub const KEEP: u8 = u8::MAX;

#[derive(Debug, Clone, Default)]
pub struct Table {
    columns: Vec<Column>,
    rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new() -> Self {
        Self::default()
    }

    /// A column always shown.
    pub fn col(mut self, header: &str, align: Align) -> Self {
        self.columns.push(Column {
            header: header.into(),
            align,
            min: None,
            keep: KEEP,
        });
        self
    }

    /// A column left out when the page is too narrow, the lowest `keep`
    /// first.
    pub fn optional(mut self, header: &str, align: Align, keep: u8) -> Self {
        self.columns.push(Column {
            header: header.into(),
            align,
            min: None,
            keep,
        });
        self
    }

    pub fn row(&mut self, cells: Vec<String>) {
        debug_assert_eq!(cells.len(), self.columns.len(), "a cell per column");
        self.rows.push(cells);
    }

    /// The widths the columns take on a page `width` wide, none for a
    /// column left out.
    fn widths(&self, width: usize) -> Vec<Option<usize>> {
        let natural: Vec<usize> = self
            .columns
            .iter()
            .enumerate()
            .map(|(i, c)| {
                self.rows
                    .iter()
                    .map(|r| chars(&r[i]))
                    .chain([chars(&c.header)])
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let mut shown: Vec<Option<usize>> = natural.iter().copied().map(Some).collect();
        loop {
            let total = |s: &[Option<usize>]| {
                let n = s.iter().flatten().count();
                s.iter().flatten().sum::<usize>() + GAP.len() * n.saturating_sub(1)
            };
            let mut over = total(&shown).saturating_sub(width);
            if over == 0 {
                return shown;
            }
            // Cut first, then leave out.
            for (i, c) in self.columns.iter().enumerate() {
                if let (Some(min), Some(w)) = (c.min, shown[i]) {
                    let floor = min.max(chars(&c.header));
                    let by = over.min(w.saturating_sub(floor));
                    shown[i] = Some(w - by);
                    over -= by;
                }
            }
            if over == 0 {
                return shown;
            }
            let drop = self
                .columns
                .iter()
                .enumerate()
                .filter(|&(i, c)| c.keep != KEEP && shown[i].is_some())
                .min_by_key(|&(i, c)| (c.keep, std::cmp::Reverse(i)))
                .map(|(i, _)| i);
            match drop {
                Some(i) => {
                    shown[i] = None;
                    // A column left out gives back its natural width to
                    // those cut.
                    for (j, c) in self.columns.iter().enumerate() {
                        if c.min.is_some() {
                            shown[j] = Some(natural[j]);
                        }
                    }
                }
                None => return shown,
            }
        }
    }
}

/// The length of `s` as a terminal shows it, one column per character.
pub fn chars(s: &str) -> usize {
    s.chars().count()
}

/// `s` cut to `width` characters, its last an ellipsis when cut.
fn fit(s: &str, width: usize) -> String {
    if chars(s) <= width {
        return s.to_string();
    }
    let mut out: String = s.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// A page of text: lines, tables and notes, as printed.
pub struct Page {
    text: String,
    color: bool,
    /// Whether a heading was the last thing written, so its table follows
    /// with no empty line.
    headed: bool,
}

impl Page {
    pub fn new(color: bool) -> Self {
        Self {
            text: String::new(),
            color,
            headed: false,
        }
    }

    fn styled(&mut self, code: &str, s: &str) {
        if self.color {
            let _ = write!(self.text, "\x1b[{code}m{s}\x1b[0m");
        } else {
            self.text.push_str(s);
        }
    }

    /// A line of words, wrapped at the page's width.
    pub fn line(&mut self, words: &str) {
        self.wrapped(words, None);
    }

    /// A line in a quieter style: what the page left out, and where to find
    /// it.
    pub fn note(&mut self, words: &str) {
        self.wrapped(words, Some("2"));
    }

    /// An empty line, never two in a row.
    pub fn blank(&mut self) {
        if !self.text.is_empty() && !self.text.ends_with("\n\n") {
            self.text.push('\n');
        }
    }

    fn wrapped(&mut self, words: &str, code: Option<&str>) {
        let mut line = String::new();
        let mut lines = Vec::new();
        for word in words.split(' ') {
            if !line.is_empty() && chars(&line) + 1 + chars(word) > WIDTH {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
        for l in lines {
            match code {
                Some(c) => self.styled(c, &l),
                None => self.text.push_str(&l),
            }
            self.text.push('\n');
        }
    }

    /// The table, after an empty line, then a note naming any column left
    /// out to fit.
    pub fn table(&mut self, t: &Table) {
        if !std::mem::take(&mut self.headed) {
            self.blank();
        }
        let widths = t.widths(WIDTH);
        let cells = |row: &[String]| {
            let mut line = String::new();
            for (i, c) in t.columns.iter().enumerate() {
                let Some(w) = widths[i] else { continue };
                if !line.is_empty() {
                    line.push_str(GAP);
                }
                let cell = fit(&row[i], w);
                let pad = " ".repeat(w - chars(&cell));
                match c.align {
                    Align::Left => {
                        line.push_str(&cell);
                        line.push_str(&pad);
                    }
                    Align::Right => {
                        line.push_str(&pad);
                        line.push_str(&cell);
                    }
                }
            }
            line.trim_end().to_string()
        };
        let headers: Vec<String> = t.columns.iter().map(|c| c.header.clone()).collect();
        let header = cells(&headers);
        self.styled("1", &header);
        self.text.push('\n');
        for row in &t.rows {
            self.text.push_str(&cells(row));
            self.text.push('\n');
        }
        let left: Vec<&str> = t
            .columns
            .iter()
            .zip(&widths)
            .filter(|(_, w)| w.is_none())
            .map(|(c, _)| c.header.as_str())
            .collect();
        if !left.is_empty() {
            let n = left.len();
            let (s, them) = if n == 1 { ("", "it") } else { ("s", "them") };
            self.note(&format!(
                "{n} column{s} left out to fit {WIDTH} characters: {}. --json has {them}.",
                left.join(", ")
            ));
        }
    }

    /// The page, ending with how to see everything.
    pub fn finish(mut self) -> String {
        self.blank();
        self.note("Every field, unrounded: add --json.");
        self.text
    }
}

/// A count and its noun, `1 site` or `3 sites`.
pub fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `v` to `places` decimals, with no negative zero; a dash when none.
pub fn fixed(v: Option<f64>, places: usize) -> String {
    match v {
        Some(v) if v.is_finite() => {
            let s = format!("{v:.places$}");
            match s.strip_prefix('-') {
                Some(rest) if rest.chars().all(|c| c == '0' || c == '.') => rest.to_string(),
                _ => s,
            }
        }
        _ => "—".into(),
    }
}

#[cfg(test)]
mod tests;
