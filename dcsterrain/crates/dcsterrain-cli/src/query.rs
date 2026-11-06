//! `dcsterrain query`: one request from the command line, or a batch of
//! them as JSON lines on stdin, in the manner of ndjson tools and of a
//! language server's stream of messages. A batch answers each line as it
//! arrives, in order, so a caller can hold the pipe open and ask as it goes.

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, ErrorKind, Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use dcsterrain_core::query::{self, Answer, Terrain};
use dcsterrain_core::synth::{self, Synth};
use dcsterrain_core::types::{BatchOutcome, BatchRequest, BatchResponse, Issue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{CacheDir, install_root, refuse_on};

/// A line of a batch's answer: a `BatchResponse` on the wire, with a result
/// that need not be whole in memory.
#[derive(Serialize)]
#[serde(untagged)]
enum Reply {
    Answered {
        line: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<Value>,
        op: String,
        result: Answer,
    },
    Refused(BatchResponse),
}

/// The terrain a command names and how to open it, opened when first
/// needed and held from then on.
pub struct Opener {
    pub install: Option<PathBuf>,
    /// The terrain's id; none until one is named.
    pub id: Option<String>,
    /// The synthetic terrain's seed, side and cell.
    pub synth: (u32, u32, u32),
    pub allow_crs_residual: bool,
    pub cache: CacheDir,
    pub terrain: Option<Terrain>,
    /// How many times a terrain has been opened.
    pub opens: u32,
}

fn issue(code: &str, message: &str) -> Vec<Issue> {
    vec![Issue::new(code, message, "")]
}

impl Opener {
    fn open(&self) -> Result<Terrain, Vec<Issue>> {
        let Some(id) = self.id.as_deref() else {
            let why = "no terrain is named: give `terrain`, an id `list_terrains` shows";
            return Err(vec![Issue::new("invalid_request", why, "terrain")]);
        };
        let (seed, size_km, cell) = self.synth;
        if id == synth::TERRAIN_ID {
            return match Synth::new(seed, size_km, cell) {
                Ok(s) => Ok(Terrain::Synth(Box::new(s))),
                Err(e) => Err(issue("terrain_refused", &e.0)),
            };
        }
        let defaults = (
            synth::DEFAULT_SEED,
            synth::DEFAULT_SIZE_KM,
            synth::CELL_SIZE_M,
        );
        if self.synth != defaults {
            return Err(issue(
                "invalid_flag",
                "--seed, --size-km and --cell set the synthetic terrain only",
            ));
        }
        let root = install_root(self.install.clone()).map_err(|e| issue("no_install", &e))?;
        let cache = self.cache.open().map_err(|e| issue("no_cache", &e))?;
        Terrain::open(&root, id, &cache, self.allow_crs_residual)
            .map_err(|e| issue("terrain_refused", &e.0))
    }

    /// Names the terrain the next answer is about. Another than the one held
    /// lets that one go before the next opens, so only one is ever open and
    /// a session over many terrains holds the memory of one.
    pub(crate) fn name(&mut self, id: Option<String>) {
        if self.id != id {
            self.terrain = None;
            self.id = id;
        }
    }

    /// The terrain as its files are now: the one held, unless they changed
    /// under it, as a patch changes them. The check is a few stats, and a
    /// refusal here is one request's: the next tries again.
    pub(crate) fn current(&mut self) -> Result<&Terrain, Vec<Issue>> {
        if self.terrain.as_ref().is_some_and(Terrain::changed) {
            tracing::debug!("the files of {} changed", self.id.as_deref().unwrap_or("?"));
            self.terrain = None;
        }
        if self.terrain.is_none() {
            self.terrain = Some(self.open()?);
            self.opens += 1;
            tracing::debug!("opened terrain {}", self.id.as_deref().unwrap_or("?"));
        }
        Ok(self.terrain.as_ref().expect("opened above"))
    }

    /// One answer. A panic is one request's failure and not the batch's; the
    /// terrain it passed through is dropped, its locks perhaps poisoned, and
    /// the next request opens it again.
    pub(crate) fn answer(&mut self, op: &str, request: &Value) -> Result<Answer, Vec<Issue>> {
        self.contained(|terrain| query::answer_to_write(terrain, op, request))
    }

    fn contained(
        &mut self,
        ask: impl FnOnce(&Terrain) -> Result<Answer, Vec<Issue>>,
    ) -> Result<Answer, Vec<Issue>> {
        let answered = catch_unwind(AssertUnwindSafe(|| ask(self.current()?)));
        answered.unwrap_or_else(|panic| {
            self.terrain = None;
            let why = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("no reason given");
            Err(issue("internal_error", why))
        })
    }

    /// The response to one line of a batch.
    fn line(&mut self, line: u64, text: &[u8]) -> Reply {
        let refused = |id, op, message: String| {
            Reply::Refused(BatchResponse {
                line,
                id,
                op,
                outcome: BatchOutcome::Issues(issue("invalid_request", &message)),
            })
        };
        let value: Value = match serde_json::from_slice(text) {
            Ok(v) => v,
            Err(e) => return refused(None, None, format!("the line is not JSON: {e}")),
        };
        let asked = match BatchRequest::deserialize(&value) {
            Ok(q) => q,
            Err(e) => {
                let id = value.get("id").filter(|id| !id.is_null()).cloned();
                let op = value.get("op").and_then(Value::as_str).map(String::from);
                return refused(id, op, format!("the line is not a request: {e}"));
            }
        };
        match self.answer(&asked.op, &asked.request) {
            Ok(result) => Reply::Answered {
                line,
                id: asked.id,
                op: asked.op,
                result,
            },
            Err(issues) => Reply::Refused(BatchResponse {
                line,
                id: asked.id,
                op: Some(asked.op),
                outcome: BatchOutcome::Issues(issues),
            }),
        }
    }
}

/// Where `query` writes: stdout, or the file `--output` names, created or
/// truncated. A file is written as bytes, so the answer is UTF-8 whatever
/// the shell would have made of stdout.
pub fn sink(output: Option<&Path>) -> Result<Box<dyn Write>, Vec<Issue>> {
    let Some(path) = output else {
        return Ok(Box::new(std::io::stdout().lock()));
    };
    match File::create(path) {
        Ok(f) => Ok(Box::new(BufWriter::new(f))),
        Err(e) => {
            let message = format!("--output {} cannot be written: {e}", path.display());
            Err(vec![Issue::new("invalid_flag", &message, "")])
        }
    }
}

/// Where a batch reads: stdin, or the file `--input` names. It is refused
/// where it is a directory, or the file `--output` names, which creating the
/// output would empty before a line was read.
pub fn source(input: Option<&Path>, output: Option<&Path>) -> Result<Box<dyn BufRead>, Vec<Issue>> {
    let Some(path) = input else {
        return Ok(Box::new(std::io::stdin().lock()));
    };
    let refused = |why: String| {
        let message = format!("--input {} {why}", path.display());
        Err(vec![Issue::new("invalid_flag", &message, "")])
    };
    let same = output.is_some_and(|o| {
        let (a, b) = (std::fs::canonicalize(path), std::fs::canonicalize(o));
        matches!((a, b), (Ok(a), Ok(b)) if a == b)
    });
    if same {
        return refused("is also --output: answer to another file".into());
    }
    match File::open(path) {
        Ok(_) if path.is_dir() => refused("is a directory".into()),
        Ok(f) => Ok(Box::new(BufReader::new(f))),
        Err(e) => refused(format!("cannot be read: {e}")),
    }
}

/// Answers one request. The answer, or its issues, is the one document on
/// `out`.
pub fn one(mut opener: Opener, op: &str, request: Option<String>, mut out: impl Write) -> ExitCode {
    let request: Value = match request.as_deref().map(serde_json::from_str).transpose() {
        Ok(v) => v.unwrap_or_else(|| json!({})),
        Err(e) => {
            let message = format!("the request is not JSON: {e}");
            return refuse_on(issue("invalid_request", &message), &mut out);
        }
    };
    match opener.answer(op, &request) {
        Ok(v) => print_on(&v, out),
        Err(issues) => refuse_on(issues, &mut out),
    }
}

/// Prints an answer as the one document on stdout, as `query` does.
pub fn print(answer: &Answer) -> ExitCode {
    print_on(answer, std::io::stdout().lock())
}

fn print_on(answer: &Answer, out: impl Write) -> ExitCode {
    let mut out = BufWriter::new(out);
    let written = serde_json::to_writer(&mut out, answer)
        .map_err(std::io::Error::from)
        .and_then(|()| out.write_all(b"\n"))
        .and_then(|()| out.flush());
    match written {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e.kind() == ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        // Where the answer could not be written, an issue list would not be
        // either, and stdout is not the answer's place when it went to a file.
        Err(e) => {
            tracing::error!("the answer cannot be written: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The longest line a batch reads. The largest requests the operations
/// accept are a few MiB, and a stream with no newlines must not fill memory.
const MAX_LINE: u64 = 64 << 20;

/// How a batch ended, for its exit code: lines with issues are answered
/// lines, so only losing the streams themselves is a failure.
#[derive(Debug, PartialEq, Eq)]
pub enum Ended {
    /// Every line read was answered.
    Input,
    /// Whoever read the answers went away.
    Closed,
    Failed(String),
}

/// How far a batch has come: the lines answered and how many of them failed,
/// told on stderr at most once a second when asked, so a long batch shows it
/// is moving without a line on stdout.
pub struct Progress {
    shown: bool,
    answered: u64,
    failed: u64,
    told: Instant,
}

impl Progress {
    const EVERY: Duration = Duration::from_secs(1);

    pub fn new(shown: bool) -> Self {
        Progress {
            shown,
            answered: 0,
            failed: 0,
            told: Instant::now(),
        }
    }

    /// Counts one line answered; true when that told the count.
    fn count(&mut self, failed: bool) -> bool {
        self.answered += 1;
        self.failed += u64::from(failed);
        let due = self.shown && self.told.elapsed() >= Self::EVERY;
        if due {
            self.tell();
        }
        due
    }

    fn tell(&mut self) {
        let (n, f) = (self.answered, self.failed);
        let lines = if n == 1 { "line" } else { "lines" };
        tracing::info!("answered {n} {lines}, {f} with issues");
        self.told = Instant::now();
    }

    /// The count once the batch has ended.
    pub fn end(&mut self) {
        if self.shown {
            self.tell();
        }
    }
}

/// Answers each line of `input` on `output`, flushing each answer. A blank
/// line asks nothing and is answered with nothing.
pub fn batch(
    opener: &mut Opener,
    input: impl BufRead,
    output: impl Write,
    progress: &mut Progress,
) -> Ended {
    let (mut input, mut output) = (input, output);
    let mut text = Vec::new();
    let mut line = 0;
    loop {
        text.clear();
        match input.by_ref().take(MAX_LINE).read_until(b'\n', &mut text) {
            Ok(0) => return Ended::Input,
            Ok(_) => line += 1,
            Err(e) => return Ended::Failed(format!("the requests cannot be read: {e}")),
        }
        let whole = text.len() < MAX_LINE as usize || text.ends_with(b"\n");
        if !whole && let Err(e) = input.skip_until(b'\n') {
            return Ended::Failed(format!("the requests cannot be read: {e}"));
        }
        // Windows tools write a byte-order mark at the top of a file.
        let body = match (line, text.strip_prefix(b"\xEF\xBB\xBF")) {
            (1, Some(rest)) => rest,
            _ => text.as_slice(),
        };
        if whole && body.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let response = if whole {
            opener.line(line, body)
        } else {
            let message = format!("the line is over {MAX_LINE} bytes");
            Reply::Refused(BatchResponse {
                line,
                id: None,
                op: None,
                outcome: BatchOutcome::Issues(issue("too_large", &message)),
            })
        };
        let failed = matches!(response, Reply::Refused(_));
        let written = serde_json::to_writer(&mut output, &response)
            .map_err(std::io::Error::from)
            .and_then(|()| output.write_all(b"\n"))
            .and_then(|()| output.flush());
        match written {
            Ok(()) => {
                progress.count(failed);
            }
            Err(e) if e.kind() == ErrorKind::BrokenPipe => return Ended::Closed,
            Err(e) => return Ended::Failed(format!("the answers cannot be written: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opener() -> Opener {
        Opener {
            install: None,
            id: Some(synth::TERRAIN_ID.into()),
            synth: (
                synth::DEFAULT_SEED,
                synth::DEFAULT_SIZE_KM,
                synth::CELL_SIZE_M,
            ),
            allow_crs_residual: false,
            cache: CacheDir { dir: None },
            terrain: None,
            opens: 0,
        }
    }

    #[test]
    fn a_panic_is_one_lines_failure_and_the_terrain_reopens() {
        let mut o = opener();
        let hurt = o.contained(|_| panic!("boom")).unwrap_err();
        assert_eq!(hurt[0].code, "internal_error");
        assert_eq!(hurt[0].message, "boom");
        assert!(o.answer("describe", &json!({})).is_ok());
        assert_eq!(o.opens, 2);
        assert!(o.answer("describe", &json!({})).is_ok());
        assert_eq!(o.opens, 2);
    }

    #[test]
    fn a_line_over_the_limit_is_refused_and_the_next_is_read() {
        let mut input = vec![b'x'; MAX_LINE as usize + 9];
        input.extend_from_slice(b"\n{\"op\":\"describe\"}\n");
        let mut output = Vec::new();
        let ended = batch(
            &mut opener(),
            &input[..],
            &mut output,
            &mut Progress::new(false),
        );
        assert_eq!(ended, Ended::Input);
        let lines: Vec<Value> = output
            .split(|b| *b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_slice(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["issues"][0]["code"], "too_large");
        assert_eq!(lines[1]["line"], 2);
        assert!(lines[1]["result"].is_object());
    }

    #[test]
    fn progress_is_told_at_most_once_a_second_and_only_when_shown() {
        let mut p = Progress::new(true);
        assert!(!p.count(false), "told before a second has passed");
        p.told -= Progress::EVERY;
        assert!(p.count(true));
        assert!(!p.count(false), "told twice within a second");
        assert_eq!((p.answered, p.failed), (3, 1));

        let mut hidden = Progress::new(false);
        hidden.told -= Progress::EVERY;
        assert!(!hidden.count(false));
    }

    #[test]
    fn a_closed_reader_ends_the_batch_quietly() {
        struct Gone;
        impl Write for Gone {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let ended = batch(
            &mut opener(),
            &b"{\"op\":\"nope\"}\n"[..],
            Gone,
            &mut Progress::new(false),
        );
        assert_eq!(ended, Ended::Closed);
    }
}
