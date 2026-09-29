// SPDX-License-Identifier: Apache-2.0

//! Asynchronous telemetry writer (ADR-008 §5).
//!
//! The gateway calls [`TelemetrySender::send`], which never blocks: events go into a bounded
//! channel read by a dedicated thread with its own SQLite connection, which writes them in
//! batched transactions. When the channel is full, events are dropped and counted.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rusqlite::Connection;

use crate::request_events::{RequestEvent, insert_batch};
use crate::{StorageError, response_routes};

pub const DEFAULT_CAPACITY: usize = 10_000;
const MAX_BATCH: usize = 512;
const FLUSH_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Clone, Debug)]
pub enum TelemetryEvent {
    Request(Box<RequestEvent>),
    ResponseRoute {
        response_id: String,
        route_target: String,
        at_ms: i64,
    },
}

#[derive(Debug, Default)]
pub struct TelemetryStats {
    pub written: AtomicU64,
    pub dropped: AtomicU64,
    pub failed_batches: AtomicU64,
}

#[derive(Clone, Debug)]
pub struct TelemetrySender {
    tx: SyncSender<TelemetryEvent>,
    stats: Arc<TelemetryStats>,
}

impl TelemetrySender {
    /// Enqueues without blocking; drops (and counts) the event if the queue is full.
    pub fn send(&self, event: TelemetryEvent) {
        match self.tx.try_send(event) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.stats.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub fn stats(&self) -> &Arc<TelemetryStats> {
        &self.stats
    }
}

/// Owns the writer thread. Dropping every [`TelemetrySender`] and then calling
/// [`TelemetryWriter::shutdown`] flushes everything queued.
#[derive(Debug)]
pub struct TelemetryWriter {
    thread: Option<JoinHandle<()>>,
}

impl TelemetryWriter {
    pub fn spawn(db_path: &Path, capacity: usize) -> Result<(Self, TelemetrySender), StorageError> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )?;
        let (tx, rx) = sync_channel(capacity);
        let stats = Arc::new(TelemetryStats::default());
        let thread_stats = stats.clone();
        let thread = std::thread::Builder::new()
            .name("loadout-telemetry".into())
            .spawn(move || run(conn, rx, &thread_stats))
            .map_err(StorageError::Io)?;
        Ok((
            Self {
                thread: Some(thread),
            },
            TelemetrySender { tx, stats },
        ))
    }

    /// Waits for the thread to drain the queue. All senders must be dropped first, otherwise
    /// this waits until they are.
    pub fn shutdown(mut self) {
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(mut conn: Connection, rx: Receiver<TelemetryEvent>, stats: &TelemetryStats) {
    let mut requests: Vec<RequestEvent> = Vec::with_capacity(MAX_BATCH);
    let mut routes: Vec<(String, String, i64)> = Vec::new();
    let mut last_flush = Instant::now();
    loop {
        let disconnected = match rx.recv_timeout(FLUSH_INTERVAL) {
            Ok(event) => {
                push(event, &mut requests, &mut routes);
                // Drain whatever else is already queued, up to a batch.
                while requests.len() < MAX_BATCH {
                    match rx.try_recv() {
                        Ok(event) => push(event, &mut requests, &mut routes),
                        Err(_) => break,
                    }
                }
                false
            }
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => true,
        };
        let due = requests.len() >= MAX_BATCH || last_flush.elapsed() >= FLUSH_INTERVAL;
        if (due || disconnected) && (!requests.is_empty() || !routes.is_empty()) {
            flush(&mut conn, &mut requests, &mut routes, stats);
            last_flush = Instant::now();
        }
        if disconnected {
            return;
        }
    }
}

fn push(
    event: TelemetryEvent,
    requests: &mut Vec<RequestEvent>,
    routes: &mut Vec<(String, String, i64)>,
) {
    match event {
        TelemetryEvent::Request(e) => requests.push(*e),
        TelemetryEvent::ResponseRoute {
            response_id,
            route_target,
            at_ms,
        } => routes.push((response_id, route_target, at_ms)),
    }
}

fn flush(
    conn: &mut Connection,
    requests: &mut Vec<RequestEvent>,
    routes: &mut Vec<(String, String, i64)>,
    stats: &TelemetryStats,
) {
    if !requests.is_empty() {
        match insert_batch(conn, requests) {
            Ok(()) => {
                stats
                    .written
                    .fetch_add(requests.len() as u64, Ordering::Relaxed);
            }
            Err(e) => {
                stats.failed_batches.fetch_add(1, Ordering::Relaxed);
                tracing::error!(error = %e, count = requests.len(), "telemetry batch failed");
            }
        }
        requests.clear();
    }
    for (id, target, at) in routes.drain(..) {
        if let Err(e) = response_routes::upsert(conn, &id, &target, at) {
            tracing::error!(error = %e, "response route write failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    fn event(i: i64) -> TelemetryEvent {
        TelemetryEvent::Request(Box::new(RequestEvent {
            ts: i,
            requested_model: format!("p/a/m{i}"),
            protocol_in: "chat".into(),
            status: "ok".into(),
            attempts: 1,
            duration_ms: 5,
            ..RequestEvent::default()
        }))
    }

    #[test]
    fn writes_batches_and_flushes_on_shutdown() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.db");
        let db = Database::open(&path).expect("db");
        let (writer, sender) = TelemetryWriter::spawn(&path, 10_000).expect("spawn");
        for i in 0..1_500 {
            sender.send(event(i));
        }
        sender.send(TelemetryEvent::ResponseRoute {
            response_id: "resp_1".into(),
            route_target: "t1:responses".into(),
            at_ms: 1,
        });
        let stats = sender.stats().clone();
        drop(sender);
        writer.shutdown();

        assert_eq!(stats.written.load(Ordering::Relaxed), 1_500);
        assert_eq!(stats.dropped.load(Ordering::Relaxed), 0);
        let recent = db.recent_requests(3).expect("recent");
        assert_eq!(recent[0].ts, 1_499);
        assert_eq!(
            db.response_route("resp_1").expect("route").as_deref(),
            Some("t1:responses")
        );
    }

    #[test]
    fn full_queue_drops_instead_of_blocking() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("t.db");
        drop(Database::open(&path).expect("db"));
        let (writer, sender) = TelemetryWriter::spawn(&path, 4).expect("spawn");
        let started = Instant::now();
        for i in 0..10_000 {
            sender.send(event(i));
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "send must not block"
        );
        let stats = sender.stats().clone();
        drop(sender);
        writer.shutdown();
        let written = stats.written.load(Ordering::Relaxed);
        let dropped = stats.dropped.load(Ordering::Relaxed);
        assert_eq!(written + dropped, 10_000);
        assert!(dropped > 0);
    }
}
