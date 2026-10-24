//! Reads the tables a map's Lua files declare, without running them.
//!
//! The files are data written as Lua 5.1: assignments of literals and table
//! constructors, a `dofile` of ED's shared scripts for the constants they
//! name, and `_ = require("i_18n").translate` to mark strings for
//! translation. That much is read. Anything that would need code to run, a
//! function body, a loop, an operator other than unary minus, stops the read
//! with the line it is on, so a file that grows logic is refused rather than
//! half read.
//!
//! A name nobody assigned reads as nil, as it would in Lua, and is reported:
//! the game takes a beacon whose type names an undefined constant as a beacon
//! with no type, and so does this reader.

pub mod tables;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// A Lua value. Strings are kept as text: bytes that are not UTF-8 are
/// replaced, since every string these files hold is a name or an id.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Bool(bool),
    Num(f64),
    Str(String),
    Table(Table),
    Func(Builtin),
}

/// The functions a data file may call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    Dofile,
    Require,
    /// `i_18n`'s `translate`, which returns its argument when no catalogue
    /// is loaded.
    Translate,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Key {
    Num(f64),
    Str(String),
    Bool(bool),
}

impl Key {
    fn of(v: Value) -> Option<Key> {
        match v {
            Value::Num(n) if !n.is_nan() => Some(Key::Num(n)),
            Value::Str(s) => Some(Key::Str(s)),
            Value::Bool(b) => Some(Key::Bool(b)),
            _ => None,
        }
    }
}

/// A table, its entries in the order the constructor gave them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Table {
    pub entries: Vec<(Key, Value)>,
    /// The first comment after the opening brace and before the first
    /// field. ED labels some entries this way, and the label is data.
    pub comment: Option<String>,
}

impl Table {
    /// Sets `k`, in place if it is already there; nil removes it, as in Lua.
    fn set(&mut self, k: Key, v: Value) {
        let at = self.entries.iter().position(|(e, _)| *e == k);
        match (at, v) {
            (Some(i), Value::Nil) => {
                self.entries.remove(i);
            }
            (Some(i), v) => self.entries[i].1 = v,
            (None, Value::Nil) => {}
            (None, v) => self.entries.push((k, v)),
        }
    }

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
    /// in key order, whatever order the constructor gave them in; an empty
    /// table is an empty list. `None` when the keys are anything else.
    pub fn list(&self) -> Option<Vec<&Value>> {
        let mut keyed: Vec<(f64, &Value)> = Vec::with_capacity(self.entries.len());
        for (k, v) in &self.entries {
            match k {
                Key::Num(n) => keyed.push((*n, v)),
                _ => return None,
            }
        }
        keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
        let from = keyed.first().map_or(1.0, |e| e.0);
        let consecutive = (from == 0.0 || from == 1.0)
            && keyed
                .iter()
                .zip(0..)
                .all(|((n, _), i)| *n == from + f64::from(i));
        consecutive.then(|| keyed.into_iter().map(|(_, v)| v).collect())
    }
}

/// Why a file cannot be read as data. The line is 1-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub line: u32,
    pub why: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.why)
    }
}

/// What a file assigned at its top level, and the names it read that
/// nothing assigned.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Chunk {
    pub globals: BTreeMap<String, Value>,
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

/// What `dofile` asks for: the constants the file at a path defines (see
/// [`constants`]), or why it cannot be read.
pub type Include<'a> = dyn FnMut(&str) -> Result<Vec<(String, f64)>, String> + 'a;

/// Reads `text`, handing each path a `dofile` names to `include` and taking
/// what it returns as globals.
pub fn read(text: &[u8], include: &mut Include) -> Result<Chunk, Error> {
    let lexed = lex(text)?;
    let mut p = Parser {
        toks: lexed.toks,
        comments: lexed.comments,
        at: 0,
        locals: BTreeMap::new(),
        chunk: Chunk::default(),
        include,
    };
    p.chunk()?;
    Ok(p.chunk)
}

/// The numeric constants a shared script assigns at its top level: each line
/// that starts in the first column as `NAME = <number>`, with an optional
/// minus, semicolon and trailing comment. The rest of the file, which may be
/// code, is not read.
pub fn constants(text: &[u8]) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    for line in text.split(|&c| c == b'\n') {
        if !line
            .first()
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
        {
            continue;
        }
        let Ok(l) = lex(line) else { continue };
        let k: Vec<&Tok> = l.toks.iter().map(|t| &t.kind).collect();
        let value = match k.as_slice() {
            [Tok::Name(_), Tok::Sym("="), Tok::Num(n)]
            | [Tok::Name(_), Tok::Sym("="), Tok::Num(n), Tok::Sym(";")] => *n,
            [Tok::Name(_), Tok::Sym("="), Tok::Sym("-"), Tok::Num(n)]
            | [
                Tok::Name(_),
                Tok::Sym("="),
                Tok::Sym("-"),
                Tok::Num(n),
                Tok::Sym(";"),
            ] => -*n,
            _ => continue,
        };
        if let Tok::Name(name) = k[0] {
            out.push((name.clone(), value));
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Name(String),
    Keyword(&'static str),
    Str(String),
    Num(f64),
    Sym(&'static str),
}

#[derive(Debug, Clone)]
struct Token {
    kind: Tok,
    pos: usize,
    line: u32,
}

struct Lexed {
    toks: Vec<Token>,
    comments: Vec<(usize, String)>,
}

const KEYWORDS: [&str; 21] = [
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local",
    "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
];

/// Longest first, so `...` is not read as `..` and `.`.
const SYMBOLS: [&str; 26] = [
    "...", "..", "==", "~=", "<=", ">=", "+", "-", "*", "/", "%", "^", "#", "<", ">", "=", "(",
    ")", "{", "}", "[", "]", ";", ":", ",", ".",
];

fn lex(b: &[u8]) -> Result<Lexed, Error> {
    let mut toks = Vec::new();
    let mut comments = Vec::new();
    let mut i = 0;
    let mut line = 1u32;
    let err = |line, why: String| Error { line, why };
    while i < b.len() {
        let c = b[i];
        if c == b'\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        let at_line = line;
        if b[i..].starts_with(b"--") {
            let (text, end) = match long_bracket(b, i + 2) {
                Some(level) => long_body(b, i + 2, level)
                    .ok_or_else(|| err(line, "a long comment is never closed".into()))?,
                None => {
                    let end = b[i..]
                        .iter()
                        .position(|&c| c == b'\n')
                        .map_or(b.len(), |n| i + n);
                    (String::from_utf8_lossy(&b[i + 2..end]).into_owned(), end)
                }
            };
            line += count_lines(&b[i..end]);
            comments.push((start, text.trim().to_string()));
            i = end;
            continue;
        }
        let kind = if c.is_ascii_alphabetic() || c == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let word = String::from_utf8_lossy(&b[start..i]).into_owned();
            match KEYWORDS.iter().find(|k| **k == word) {
                Some(k) => Tok::Keyword(k),
                None => Tok::Name(word),
            }
        } else if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            let (n, end) = number(b, i).ok_or_else(|| {
                err(
                    line,
                    format!("{:?} is not a number", lossy(&b[i..b.len().min(i + 16)])),
                )
            })?;
            i = end;
            Tok::Num(n)
        } else if c == b'"' || c == b'\'' {
            let (s, end) = short_string(b, i).map_err(|why| err(line, why))?;
            line += count_lines(&b[i..end]);
            i = end;
            Tok::Str(s)
        } else if let Some(level) = (c == b'[').then(|| long_bracket(b, i)).flatten() {
            let (s, end) = long_body(b, i, level)
                .ok_or_else(|| err(line, "a long string is never closed".into()))?;
            line += count_lines(&b[i..end]);
            i = end;
            Tok::Str(s)
        } else if let Some(s) = SYMBOLS.iter().find(|s| b[i..].starts_with(s.as_bytes())) {
            i += s.len();
            Tok::Sym(s)
        } else {
            return Err(err(line, format!("unexpected character {:?}", c as char)));
        };
        toks.push(Token {
            kind,
            pos: start,
            line: at_line,
        });
    }
    Ok(Lexed { toks, comments })
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn count_lines(b: &[u8]) -> u32 {
    b.iter().filter(|&&c| c == b'\n').count() as u32
}

/// The level of a long bracket `[==[` opening at `i`.
fn long_bracket(b: &[u8], i: usize) -> Option<usize> {
    if b.get(i) != Some(&b'[') {
        return None;
    }
    let level = b[i + 1..].iter().take_while(|&&c| c == b'=').count();
    (b.get(i + 1 + level) == Some(&b'[')).then_some(level)
}

/// The body of the long bracket opening at `i`, and the position past its
/// close. A newline straight after the opening bracket is not part of it.
fn long_body(b: &[u8], i: usize, level: usize) -> Option<(String, usize)> {
    let mut from = i + level + 2;
    if b[from..].starts_with(b"\r\n") {
        from += 2;
    } else if b.get(from) == Some(&b'\n') {
        from += 1;
    }
    let close = [b"]".as_slice(), &vec![b'='; level], b"]"].concat();
    let n = b[from..].windows(close.len()).position(|w| w == close)?;
    Some((lossy(&b[from..from + n]), from + n + close.len()))
}

/// A Lua 5.1 numeral at `i`: decimal with optional fraction and exponent,
/// or hexadecimal integer.
fn number(b: &[u8], i: usize) -> Option<(f64, usize)> {
    let mut j = i;
    if b[i..].starts_with(b"0x") || b[i..].starts_with(b"0X") {
        j += 2;
        while j < b.len() && b[j].is_ascii_hexdigit() {
            j += 1;
        }
        let digits = std::str::from_utf8(&b[i + 2..j]).ok()?;
        return Some((u64::from_str_radix(digits, 16).ok()? as f64, j));
    }
    while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'.') {
        j += 1;
    }
    if j < b.len() && (b[j] == b'e' || b[j] == b'E') {
        j += 1;
        if j < b.len() && (b[j] == b'+' || b[j] == b'-') {
            j += 1;
        }
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
    }
    // Lua reads `3x` as one malformed numeral, not a number and a name.
    if j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
        return None;
    }
    let n = std::str::from_utf8(&b[i..j]).ok()?.parse().ok()?;
    Some((n, j))
}

fn short_string(b: &[u8], i: usize) -> Result<(String, usize), String> {
    let q = b[i];
    let mut out = Vec::new();
    let mut j = i + 1;
    loop {
        let Some(&c) = b.get(j) else {
            return Err("a string is never closed".into());
        };
        j += 1;
        match c {
            _ if c == q => break,
            b'\n' => return Err("a string runs past the end of its line".into()),
            b'\\' => {
                let Some(&e) = b.get(j) else {
                    return Err("a string is never closed".into());
                };
                j += 1;
                match e {
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'r' => out.push(b'\r'),
                    b'a' => out.push(7),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'v' => out.push(11),
                    b'\n' => out.push(b'\n'),
                    b'0'..=b'9' => {
                        let mut n = u32::from(e - b'0');
                        for _ in 0..2 {
                            match b.get(j) {
                                Some(d) if d.is_ascii_digit() => {
                                    n = n * 10 + u32::from(d - b'0');
                                    j += 1;
                                }
                                _ => break,
                            }
                        }
                        out.push(
                            u8::try_from(n).map_err(|_| format!("escape \\{n} is too large"))?,
                        );
                    }
                    // Lua 5.1 keeps any other escaped character as itself.
                    e => out.push(e),
                }
            }
            c => out.push(c),
        }
    }
    Ok((lossy(&out), j))
}

struct Parser<'a> {
    toks: Vec<Token>,
    comments: Vec<(usize, String)>,
    at: usize,
    locals: BTreeMap<String, Value>,
    chunk: Chunk,
    include: &'a mut Include<'a>,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at).map(|t| &t.kind)
    }

    fn line(&self) -> u32 {
        self.toks
            .get(self.at)
            .or(self.toks.last())
            .map_or(1, |t| t.line)
    }

    fn fail<T>(&self, why: impl Into<String>) -> Result<T, Error> {
        Err(Error {
            line: self.line(),
            why: why.into(),
        })
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.at).map(|t| t.kind.clone());
        self.at += 1;
        t
    }

    fn eat(&mut self, sym: &str) -> bool {
        if matches!(self.peek(), Some(Tok::Sym(s)) if *s == sym) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, sym: &str) -> Result<(), Error> {
        if self.eat(sym) {
            Ok(())
        } else {
            let found = self.describe();
            self.fail(format!("expected {sym:?}, found {found}"))
        }
    }

    fn describe(&self) -> String {
        match self.peek() {
            None => "the end of the file".into(),
            Some(Tok::Name(n)) => format!("{n:?}"),
            Some(Tok::Keyword(k)) => format!("{k:?}"),
            Some(Tok::Str(_)) => "a string".into(),
            Some(Tok::Num(_)) => "a number".into(),
            Some(Tok::Sym(s)) => format!("{s:?}"),
        }
    }

    fn name(&mut self) -> Result<String, Error> {
        match self.next() {
            Some(Tok::Name(n)) => Ok(n),
            _ => {
                self.at -= 1;
                let found = self.describe();
                self.fail(format!("expected a name, found {found}"))
            }
        }
    }

    fn chunk(&mut self) -> Result<(), Error> {
        while let Some(t) = self.peek().cloned() {
            match t {
                Tok::Sym(";") => self.at += 1,
                Tok::Keyword("local") => self.local()?,
                Tok::Name(_) | Tok::Sym("(") => self.assignment_or_call()?,
                Tok::Keyword(k) => return self.fail(format!("{k:?} is code, which is not read")),
                _ => {
                    let found = self.describe();
                    return self.fail(format!("expected a statement, found {found}"));
                }
            }
        }
        Ok(())
    }

    fn local(&mut self) -> Result<(), Error> {
        self.at += 1;
        if matches!(self.peek(), Some(Tok::Keyword("function"))) {
            return self.fail("\"function\" is code, which is not read");
        }
        let mut names = vec![self.name()?];
        while self.eat(",") {
            names.push(self.name()?);
        }
        let values = if self.eat("=") {
            self.exprs()?
        } else {
            Vec::new()
        };
        let mut values = values.into_iter();
        for n in names {
            let v = values.next().unwrap_or(Value::Nil);
            self.locals.insert(n, v);
        }
        Ok(())
    }

    fn assignment_or_call(&mut self) -> Result<(), Error> {
        // A plain name followed by `=` or `,` is an assignment target.
        if let (Some(Tok::Name(n)), Some(Tok::Sym("=" | ","))) = (
            self.peek().cloned(),
            self.toks.get(self.at + 1).map(|t| t.kind.clone()),
        ) {
            self.at += 1;
            let mut names = vec![n];
            while self.eat(",") {
                names.push(self.name()?);
            }
            self.expect("=")?;
            let mut values = self.exprs()?.into_iter();
            for n in names {
                let v = values.next().unwrap_or(Value::Nil);
                if let Some(slot) = self.locals.get_mut(&n) {
                    *slot = v;
                } else if v == Value::Nil {
                    self.chunk.globals.remove(&n);
                } else {
                    self.chunk.globals.insert(n, v);
                }
            }
            return Ok(());
        }
        let (_, called) = self.prefix()?;
        if !called {
            if matches!(self.peek(), Some(Tok::Sym("="))) {
                return self.fail("only a plain name can be assigned");
            }
            return self.fail("an expression is not a statement");
        }
        Ok(())
    }

    fn exprs(&mut self) -> Result<Vec<Value>, Error> {
        let mut out = vec![self.expr()?];
        while self.eat(",") {
            out.push(self.expr()?);
        }
        Ok(out)
    }

    fn expr(&mut self) -> Result<Value, Error> {
        let v = self.unary()?;
        if let Some(Tok::Sym(s @ ("+" | "-" | "*" | "/" | "%" | "^" | "..")))
        | Some(Tok::Sym(s @ ("==" | "~=" | "<=" | ">=" | "<" | ">"))) = self.peek()
        {
            let s = *s;
            return self.fail(format!("operator {s:?} is code, which is not read"));
        }
        if let Some(Tok::Keyword(k @ ("and" | "or"))) = self.peek() {
            let k = *k;
            return self.fail(format!("operator {k:?} is code, which is not read"));
        }
        Ok(v)
    }

    fn unary(&mut self) -> Result<Value, Error> {
        if self.eat("-") {
            return match self.unary()? {
                Value::Num(n) => Ok(Value::Num(-n)),
                _ => self.fail("minus applies to a number only"),
            };
        }
        self.simple()
    }

    fn simple(&mut self) -> Result<Value, Error> {
        match self.peek().cloned() {
            Some(Tok::Keyword("nil")) => {
                self.at += 1;
                Ok(Value::Nil)
            }
            Some(Tok::Keyword("true")) => {
                self.at += 1;
                Ok(Value::Bool(true))
            }
            Some(Tok::Keyword("false")) => {
                self.at += 1;
                Ok(Value::Bool(false))
            }
            Some(Tok::Num(n)) => {
                self.at += 1;
                Ok(Value::Num(n))
            }
            Some(Tok::Str(s)) => {
                self.at += 1;
                Ok(Value::Str(s))
            }
            Some(Tok::Sym("{")) => self.table().map(Value::Table),
            Some(Tok::Name(_) | Tok::Sym("(")) => self.prefix().map(|(v, _)| v),
            Some(Tok::Keyword(k)) => self.fail(format!("{k:?} is code, which is not read")),
            _ => {
                let found = self.describe();
                self.fail(format!("expected a value, found {found}"))
            }
        }
    }

    /// A name or parenthesised value, then any field reads and calls. Also
    /// says whether it ended in a call, which is all a statement may be.
    fn prefix(&mut self) -> Result<(Value, bool), Error> {
        let mut v = if self.eat("(") {
            let v = self.expr()?;
            self.expect(")")?;
            v
        } else {
            let n = self.name()?;
            self.lookup(&n)
        };
        let mut called = false;
        loop {
            match self.peek().cloned() {
                Some(Tok::Sym(".")) => {
                    self.at += 1;
                    let field = self.name()?;
                    v = self.field(v, Value::Str(field))?;
                    called = false;
                }
                Some(Tok::Sym("[")) => {
                    self.at += 1;
                    let k = self.expr()?;
                    self.expect("]")?;
                    v = self.field(v, k)?;
                    called = false;
                }
                Some(Tok::Sym(":")) => {
                    return self.fail("a method call is code, which is not read");
                }
                Some(Tok::Sym("(")) => {
                    self.at += 1;
                    let args = if self.eat(")") {
                        Vec::new()
                    } else {
                        let a = self.exprs()?;
                        self.expect(")")?;
                        a
                    };
                    v = self.call(v, args)?;
                    called = true;
                }
                Some(Tok::Str(s)) => {
                    self.at += 1;
                    v = self.call(v, vec![Value::Str(s)])?;
                    called = true;
                }
                Some(Tok::Sym("{")) => {
                    let t = self.table()?;
                    v = self.call(v, vec![Value::Table(t)])?;
                    called = true;
                }
                _ => return Ok((v, called)),
            }
        }
    }

    fn lookup(&mut self, name: &str) -> Value {
        if let Some(v) = self.locals.get(name) {
            return v.clone();
        }
        if let Some(v) = self.chunk.globals.get(name) {
            return v.clone();
        }
        match name {
            "dofile" => Value::Func(Builtin::Dofile),
            "require" => Value::Func(Builtin::Require),
            _ => {
                self.chunk.unresolved.insert(name.to_string());
                Value::Nil
            }
        }
    }

    fn field(&self, of: Value, k: Value) -> Result<Value, Error> {
        let Value::Table(t) = of else {
            return self.fail("a field is read from something that is not a table");
        };
        Ok(match Key::of(k) {
            Some(k) => t
                .entries
                .iter()
                .find(|(e, _)| *e == k)
                .map_or(Value::Nil, |(_, v)| v.clone()),
            None => Value::Nil,
        })
    }

    fn call(&mut self, f: Value, args: Vec<Value>) -> Result<Value, Error> {
        let first = args.into_iter().next().unwrap_or(Value::Nil);
        match f {
            Value::Func(Builtin::Translate) => Ok(first),
            Value::Func(Builtin::Require) => match first {
                Value::Str(m) if m == "i_18n" => Ok(Value::Table(Table {
                    entries: vec![(
                        Key::Str("translate".into()),
                        Value::Func(Builtin::Translate),
                    )],
                    comment: None,
                })),
                Value::Str(m) => self.fail(format!("requires {m:?}, which is not read")),
                _ => self.fail("require takes a module name"),
            },
            Value::Func(Builtin::Dofile) => {
                let Value::Str(path) = first else {
                    return self.fail("dofile takes a path");
                };
                let defined = (self.include)(&path).or_else(|why| self.fail(why))?;
                for (name, n) in defined {
                    self.chunk.globals.insert(name, Value::Num(n));
                }
                Ok(Value::Nil)
            }
            _ => self.fail("calls something that is not a known function"),
        }
    }

    fn table(&mut self) -> Result<Table, Error> {
        let open = self.toks[self.at].pos;
        self.expect("{")?;
        let first_field = self.toks.get(self.at).map_or(usize::MAX, |t| t.pos);
        let comment = self
            .comments
            .iter()
            .find(|(p, _)| *p > open && *p < first_field)
            .map(|(_, c)| c.clone());
        let mut t = Table {
            entries: Vec::new(),
            comment,
        };
        let mut next = 1.0;
        while !self.eat("}") {
            if self.eat("[") {
                let k = self.expr()?;
                self.expect("]")?;
                self.expect("=")?;
                let v = self.expr()?;
                match Key::of(k) {
                    Some(k) => t.set(k, v),
                    None => return self.fail("a table key is nil or not a number"),
                }
            } else if let (Some(Tok::Name(n)), Some(Tok::Sym("="))) = (
                self.peek().cloned(),
                self.toks.get(self.at + 1).map(|t| t.kind.clone()),
            ) {
                self.at += 2;
                let v = self.expr()?;
                t.set(Key::Str(n), v);
            } else {
                let v = self.expr()?;
                t.set(Key::Num(next), v);
                next += 1.0;
            }
            if !self.eat(",") && !self.eat(";") {
                self.expect("}")?;
                break;
            }
        }
        Ok(t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_include(path: &str) -> Result<Vec<(String, f64)>, String> {
        Err(format!("no file {path}"))
    }

    fn parse(text: &str) -> Chunk {
        read(text.as_bytes(), &mut no_include).unwrap()
    }

    fn fails(text: &str) -> Error {
        read(text.as_bytes(), &mut no_include).unwrap_err()
    }

    #[test]
    fn reads_literals_tables_and_translated_strings() {
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
        assert!(c.unresolved.is_empty());
        assert!(!c.globals.contains_key("gettext"), "locals are not globals");
    }

    #[test]
    fn reads_bracketed_keys_and_lists_them_in_key_order() {
        let c = parse("t = { [31] = 'c', [2] = 'b', [1] = 'a' }\nu = { [0] = 'x', [1] = 'y' }\n");
        let t = c.table("t").unwrap();
        // 1, 2 and 31 are not consecutive.
        assert_eq!(t.list(), None);
        assert_eq!(t.index(31.0), Some(&Value::Str("c".into())));
        let u: Vec<&Value> = c.table("u").unwrap().list().unwrap();
        assert_eq!(u, [&Value::Str("x".into()), &Value::Str("y".into())]);
        assert_eq!(Table::default().list(), Some(vec![]));
        let c = parse("t = { [\"KUTAISI\"] = { latitude = 42.27 } }");
        assert!(c.table("t").unwrap().list().is_none());
    }

    #[test]
    fn decodes_escapes_and_long_strings() {
        let c = parse("a = 'it\\'s \\65\\t' b = [[\nfirst\nsecond]] c = [==[x]]y]==]");
        assert_eq!(c.globals["a"], Value::Str("it's A\t".into()));
        assert_eq!(c.globals["b"], Value::Str("first\nsecond".into()));
        assert_eq!(c.globals["c"], Value::Str("x]]y".into()));
    }

    #[test]
    fn keeps_the_comment_that_opens_a_table() {
        let c = parse(
            "radio = {\n\t{\n\t\t-- Anapa\n\t\tradioId = 'airfield12_0';\n\t\t-- later\n\t};\n\t{ radioId = 'x' };\n}\n",
        );
        let r = c.table("radio").unwrap().list().unwrap();
        let (Value::Table(a), Value::Table(b)) = (r[0], r[1]) else {
            panic!()
        };
        assert_eq!(a.comment.as_deref(), Some("Anapa"));
        assert_eq!(b.comment, None);
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
    fn takes_the_constants_a_dofile_names() {
        let mut asked = Vec::new();
        let mut include = |p: &str| {
            asked.push(p.to_string());
            Ok(constants(
                b"-- Frequency bands\nHF = 0\nUHF = 3;\nM = -1 -- discard\n\tINNER = 9\nF = f(1)\n",
            ))
        };
        let c = read(
            b"dofile('Scripts/World/Radio/FrequencyBands.lua')\nf = { [HF] = 1, [UHF] = 2, m = M, i = INNER }\n",
            &mut include,
        )
        .unwrap();
        assert_eq!(asked, ["Scripts/World/Radio/FrequencyBands.lua"]);
        let f = c.table("f").unwrap();
        assert_eq!(f.index(0.0), Some(&Value::Num(1.0)));
        assert_eq!(f.index(3.0), Some(&Value::Num(2.0)));
        assert_eq!(f.num("m"), Some(-1.0));
        // Indented or computed, it is not a constant.
        assert_eq!(f.get("i"), None);
        assert!(!c.globals.contains_key("F"));
    }

    #[test]
    fn a_dofile_that_cannot_be_read_stops_the_read() {
        let e = fails("\n\ndofile('Scripts/x.lua')\n");
        assert_eq!(e.to_string(), "line 3: no file Scripts/x.lua");
    }

    #[test]
    fn refuses_code() {
        assert_eq!(fails("x = 1\nif x then y = 2 end").line, 2);
        assert!(fails("local function f() end").why.contains("\"function\""));
        assert!(fails("t = { a = 1 + 2 }").why.contains("operator \"+\""));
        assert!(
            fails("t = { a = 'x' .. 'y' }")
                .why
                .contains("operator \"..\"")
        );
        assert!(
            fails(
                "t = {}
t.a = 1"
            )
            .why
            .contains("plain name")
        );
        assert!(fails("print('x')").why.contains("not a known function"));
        assert!(
            fails("local u = require('utils')")
                .why
                .contains("\"utils\"")
        );
        assert!(fails("t = { 1, 2").why.contains("the end of the file"));
        assert!(fails("a = 'open").why.contains("never closed"));
        assert!(fails("a = 3x").why.contains("not a number"));
    }

    #[test]
    fn counts_lines_across_comments_and_long_strings() {
        let e = fails("--[[ one\ntwo ]]\na = [[x\ny]]\n-- c\nwhile");
        assert_eq!(e.line, 6, "{e}");
    }

    #[test]
    fn replaces_bytes_that_are_not_utf8() {
        let c = read(b"a = 'Boulder\xf1ity'", &mut no_include).unwrap();
        assert_eq!(c.globals["a"], Value::Str("Boulder\u{fffd}ity".into()));
    }
}
