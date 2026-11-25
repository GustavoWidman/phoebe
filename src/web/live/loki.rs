//! the node's Loki, which holds every machine's logs: pages of lines over
//! its HTTP API, and the lines that arrive over its tail websocket

use std::time::Duration;

use eyre::{Result, WrapErr, bail, eyre};
use futures_util::{Stream, StreamExt};
use serde::Deserialize;
use tokio_tungstenite::tungstenite::Message;

use super::prometheus::Labels;
use crate::web::wire::{Hostname, Level, LogLine, Ms};

/// one line as Loki stores it, with the labels of its stream
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// epoch nanoseconds
    pub ns: i64,
    pub labels: Labels,
    pub line: String,
}

impl Entry {
    /// the line as the web app reads it. the data plane labels a line from
    /// the journal with `machine`, `unit` and `level`, and one from a file on
    /// darwin with `machine` and `file`
    pub fn log_line(&self) -> LogLine {
        let label = |name: &str| self.labels.get(name).map(String::as_str);
        let text = plain(&self.line);
        // most programs write every line to stdout, which the journal files
        // at info whatever the line says, so the line's own word for its
        // level stands in for an info
        let level = match label("level").map(level) {
            Some(Level::Info) | None => written_level(&text).unwrap_or(Level::Info),
            Some(level) => level,
        };
        LogLine {
            at: Ms(self.ns.div_euclid(1_000_000)),
            machine: Hostname::from(label("machine").unwrap_or("unknown")),
            unit: label("unit")
                .or_else(|| label("file"))
                .unwrap_or("unknown")
                .to_owned(),
            level,
            text,
        }
    }
}

/// the level a line names near its start, as loggers write it: `level=warn`,
/// `"level":"WARN"`, `[warn]`, or a word of its own such as `ERROR`
fn written_level(text: &str) -> Option<Level> {
    let head: String = text.chars().take(160).collect();
    let lower = head.to_lowercase();
    for key in ["level=", "lvl=", "\"level\":\"", "[", "level: "] {
        let mut rest = lower.as_str();
        while let Some(at) = rest.find(key) {
            rest = &rest[at + key.len()..];
            let word: String = rest.chars().take_while(char::is_ascii_alphabetic).collect();
            if let Some(level) = named_level(&word) {
                return Some(level);
            }
        }
    }
    head.split(|c: char| !c.is_ascii_alphabetic())
        .filter(|word| word.len() >= 4 && word.chars().all(|c| c.is_ascii_uppercase()))
        .find_map(|word| named_level(&word.to_lowercase()))
}

fn named_level(word: &str) -> Option<Level> {
    Some(match word {
        "error" | "erro" | "err" | "fatal" | "panic" | "crit" | "critical" => Level::Error,
        "warn" | "warning" => Level::Warn,
        "info" => Level::Info,
        "debug" | "debu" | "trace" => Level::Debug,
        _ => return None,
    })
}

/// a syslog level, or the names Loki guesses
fn level(name: &str) -> Level {
    match name {
        "emerg" | "alert" | "crit" | "critical" | "err" | "error" | "fatal" => Level::Error,
        "warning" | "warn" => Level::Warn,
        "debug" | "trace" => Level::Debug,
        _ => Level::Info,
    }
}

/// the text without the terminal's color codes some programs write to
/// their log files
fn plain(line: &str) -> String {
    let mut text = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            // parameters and intermediates, up to the final byte
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
            continue;
        }
        text.push(c);
    }
    text
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Backward,
    Forward,
}

#[derive(Clone)]
pub struct Loki {
    http: reqwest::Client,
    /// `host:port`
    authority: String,
}

impl Loki {
    pub fn new(authority: String, timeout: Duration) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(timeout)
                .build()
                .expect("a plain HTTP client always builds"),
            authority,
        }
    }

    /// lines of `logql` from `start` up to, not including, `end`, epoch
    /// nanoseconds: the newest `limit` of them going backward, the oldest
    /// going forward
    pub async fn range_body(
        &self,
        logql: &str,
        start: i64,
        end: i64,
        limit: usize,
        direction: Direction,
    ) -> Result<String> {
        let response = self
            .http
            .get(format!("http://{}/loki/api/v1/query_range", self.authority))
            .query(&[
                ("query", logql.to_owned()),
                ("start", start.to_string()),
                ("end", end.to_string()),
                ("limit", limit.to_string()),
                (
                    "direction",
                    match direction {
                        Direction::Backward => "backward",
                        Direction::Forward => "forward",
                    }
                    .to_owned(),
                ),
            ])
            .send()
            .await
            .wrap_err_with(|| format!("{} did not answer", self.authority))?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            bail!("{} answered {status}: {}", self.authority, body.trim());
        }
        Ok(body)
    }

    pub async fn range(
        &self,
        logql: &str,
        start: i64,
        end: i64,
        limit: usize,
        direction: Direction,
    ) -> Result<Vec<Entry>> {
        streams(&self.range_body(logql, start, end, limit, direction).await?)
            .wrap_err_with(|| format!("asking {logql}"))
    }

    /// the lines of `logql` that arrive from `start`, epoch nanoseconds,
    /// a batch at a time, until Loki closes the tail
    pub async fn tail(
        &self,
        logql: &str,
        start: i64,
    ) -> Result<impl Stream<Item = Vec<Entry>> + Send + use<>> {
        let (socket, _) = tokio_tungstenite::connect_async(self.tail_url(logql, start))
            .await
            .wrap_err_with(|| format!("{} refused a tail", self.authority))?;
        Ok(socket.filter_map(|message| async move {
            match message {
                Ok(Message::Text(text)) => match tailed(text.as_str()) {
                    Ok(entries) if !entries.is_empty() => Some(entries),
                    Ok(_) => None,
                    Err(error) => {
                        log::warn!("a tail sent what is not a batch of lines: {error:#}");
                        None
                    }
                },
                _ => None,
            }
        }))
    }
}

impl Loki {
    pub fn tail_url(&self, logql: &str, start: i64) -> String {
        format!(
            "ws://{}/loki/api/v1/tail?query={}&start={start}&delay_for=0&limit=1000",
            self.authority,
            url_encode(logql)
        )
    }
}

/// percent-encodes everything but the unreserved characters
fn url_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[derive(Deserialize)]
struct RawStream {
    stream: Labels,
    values: Vec<(String, String)>,
}

fn entries(streams: Vec<RawStream>) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for stream in streams {
        for (ns, line) in stream.values {
            entries.push(Entry {
                ns: ns
                    .parse()
                    .map_err(|_| eyre!("{ns} is not a time in nanoseconds"))?,
                labels: stream.stream.clone(),
                line,
            });
        }
    }
    Ok(entries)
}

/// a `query_range` answer's lines, in no particular order
pub fn streams(body: &str) -> Result<Vec<Entry>> {
    #[derive(Deserialize)]
    struct Answer {
        status: String,
        data: Data,
    }
    #[derive(Deserialize)]
    struct Data {
        #[serde(rename = "resultType")]
        kind: String,
        result: serde_json::Value,
    }

    let answer: Answer = serde_json::from_str(body).wrap_err("not an answer of Loki's API")?;
    if answer.status != "success" {
        bail!("Loki refused: {}", answer.status);
    }
    if answer.data.kind != "streams" {
        bail!("expected streams of lines, not a {}", answer.data.kind);
    }
    entries(serde_json::from_value(answer.data.result).wrap_err("not streams of lines")?)
}

/// one message of a tail, oldest line first
pub fn tailed(message: &str) -> Result<Vec<Entry>> {
    #[derive(Deserialize)]
    struct Tailed {
        #[serde(default)]
        streams: Vec<RawStream>,
    }
    let tailed: Tailed = serde_json::from_str(message).wrap_err("not a message of Loki's tail")?;
    let mut entries = entries(tailed.streams)?;
    entries.sort_by_key(|entry| entry.ns);
    Ok(entries)
}
