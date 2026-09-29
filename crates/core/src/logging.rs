// SPDX-License-Identifier: Apache-2.0

//! Structured logging to `<data>/logs/loadoutd.YYYY-MM-DD.log` (ADR-008 §6).
//!
//! Secrets must never be logged in the first place (ADR-004 §5). As defense in depth, every
//! formatted line passes through [`redact`], which masks anything shaped like a client key,
//! a provider API key or a bearer token.

use std::borrow::Cow;
use std::io::{self, Write};
use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const MAX_LOG_FILES: usize = 14;

/// Keeps the background log writer alive; drop it last to flush.
#[derive(Debug)]
pub struct LogGuard {
    _file: WorkerGuard,
}

/// Initializes the global subscriber. `filter` uses `RUST_LOG` syntax (e.g. `info`,
/// `loadout_gateway=debug`). With `stderr`, logs are mirrored to standard error.
pub fn init(log_dir: &Path, filter: &str, stderr: bool) -> io::Result<LogGuard> {
    std::fs::create_dir_all(log_dir)?;
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("loadoutd")
        .filename_suffix("log")
        .max_log_files(MAX_LOG_FILES)
        .build(log_dir)
        .map_err(io::Error::other)?;
    let (file_writer, guard) = tracing_appender::non_blocking(appender);

    let filter = EnvFilter::try_new(filter).unwrap_or_else(|_| EnvFilter::new("info"));
    let file_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_writer(Redacting(file_writer));
    let stderr_layer = stderr.then(|| {
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(Redacting(io::stderr))
    });

    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(stderr_layer)
        .try_init()
        .map_err(io::Error::other)?;
    Ok(LogGuard { _file: guard })
}

/// `MakeWriter` wrapper that redacts each formatted event before it is written.
#[derive(Clone, Debug)]
pub struct Redacting<M>(pub M);

impl<'a, M: MakeWriter<'a>> MakeWriter<'a> for Redacting<M> {
    type Writer = RedactingWriter<M::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter(self.0.make_writer())
    }
}

#[derive(Debug)]
pub struct RedactingWriter<W>(W);

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write_all(&redact(buf))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

const MASK: &[u8] = b"[REDACTED]";

/// Masks credential-shaped substrings:
/// - `lo-` + ≥16 hex chars (Loadout client keys)
/// - `sk-` + ≥16 key chars (common provider API keys)
/// - `Bearer ` + ≥8 token chars
pub fn redact(input: &[u8]) -> Cow<'_, [u8]> {
    let mut out: Option<Vec<u8>> = None;
    let mut i = 0;
    let mut copied = 0;
    while i < input.len() {
        let rest = &input[i..];
        let hit = if rest.starts_with(b"lo-") {
            run(&rest[3..], |b| b.is_ascii_hexdigit())
                .filter(|&n| n >= 16)
                .map(|n| (3, n))
        } else if rest.starts_with(b"sk-") {
            run(&rest[3..], is_key_char)
                .filter(|&n| n >= 16)
                .map(|n| (3, n))
        } else if rest.len() >= 7 && rest[..7].eq_ignore_ascii_case(b"bearer ") {
            run(&rest[7..], is_token_char)
                .filter(|&n| n >= 8)
                .map(|n| (7, n))
        } else {
            None
        };
        match hit {
            Some((keep, secret_len)) => {
                let buf = out.get_or_insert_with(|| Vec::with_capacity(input.len()));
                buf.extend_from_slice(&input[copied..i + keep]);
                buf.extend_from_slice(MASK);
                i += keep + secret_len;
                copied = i;
            }
            None => i += 1,
        }
    }
    match out {
        None => Cow::Borrowed(input),
        Some(mut buf) => {
            buf.extend_from_slice(&input[copied..]);
            Cow::Owned(buf)
        }
    }
}

fn run(bytes: &[u8], accept: impl Fn(u8) -> bool) -> Option<usize> {
    Some(bytes.iter().take_while(|&&b| accept(b)).count())
}

fn is_key_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
}

fn is_token_char(b: u8) -> bool {
    !b.is_ascii_whitespace() && b != b'"' && b != b'\'' && b != b','
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(s: &str) -> String {
        String::from_utf8(redact(s.as_bytes()).into_owned()).expect("utf8")
    }

    #[test]
    fn masks_credentials() {
        let client = format!("lo-{}", "ab12".repeat(16));
        assert_eq!(r(&format!("key={client} ok")), "key=lo-[REDACTED] ok");
        assert_eq!(
            r("upstream key sk-proj-AbCdEf0123456789xyz rejected"),
            "upstream key sk-[REDACTED] rejected"
        );
        assert_eq!(
            r(r#"header="Bearer abc.def-123456" next"#),
            r#"header="Bearer [REDACTED]" next"#
        );
        assert_eq!(
            r("authorization: bearer tok_1234567890"),
            "authorization: bearer [REDACTED]"
        );
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        for s in [
            "loadout-core started",
            "lo-abc too short",
            "risk-free",
            "sk-short",
            "Bearer x",
            "gateway listening on 127.0.0.1:8317",
            "",
        ] {
            assert!(matches!(redact(s.as_bytes()), Cow::Borrowed(_)), "{s}");
        }
    }

    #[test]
    fn masks_multiple_occurrences() {
        let k = format!("lo-{}", "f".repeat(64));
        assert_eq!(
            r(&format!("{k} and {k}")),
            "lo-[REDACTED] and lo-[REDACTED]"
        );
    }

    #[test]
    fn writer_redacts() {
        let mut buf = Vec::new();
        {
            let mut w = RedactingWriter(&mut buf);
            w.write_all(format!("x lo-{} y\n", "0".repeat(20)).as_bytes())
                .expect("write");
        }
        assert_eq!(String::from_utf8(buf).expect("utf8"), "x lo-[REDACTED] y\n");
    }
}
