// SPDX-License-Identifier: Apache-2.0

//! Structured logging to `<data>/logs/loadoutd.YYYY-MM-DD.log` (ADR-008 §6).
//!
//! Secrets must never be logged in the first place (ADR-004 §5). As defense in depth, every
//! formatted line passes through [`redact`], which masks anything shaped like a client key,
//! a provider API key or a bearer token.

use std::io::{self, Write};
use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub use loadout_secrets::redact;

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

#[cfg(test)]
mod tests {
    use super::*;

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
