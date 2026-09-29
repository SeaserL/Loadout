// SPDX-License-Identifier: Apache-2.0

//! Telemetry write throughput through the async writer (target ≥ 5,000 events/s).

use std::time::{Duration, Instant};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use loadout_storage::telemetry::{TelemetryEvent, TelemetryWriter};
use loadout_storage::{Database, RequestEvent};

const EVENTS: u64 = 5_000;

fn event(i: i64) -> TelemetryEvent {
    TelemetryEvent::Request(Box::new(RequestEvent {
        ts: i,
        client_id: Some("bench".into()),
        requested_model: "p/a/model".into(),
        route_target: Some("m1:chat".into()),
        provider: Some("p".into()),
        account: Some("a".into()),
        upstream_model: Some("model".into()),
        protocol_in: "chat".into(),
        protocol_out: Some("chat".into()),
        mode: Some("passthrough".into()),
        stream: true,
        status: "ok".into(),
        attempts: 1,
        input_tokens: Some(1_000),
        output_tokens: Some(200),
        ttft_ms: Some(300),
        duration_ms: 2_000,
        output_tps: Some(100.0),
        estimated_cost_usd: Some(0.001),
        usage_confidence: Some("EXACT".into()),
        ..RequestEvent::default()
    }))
}

fn sqlite_usage_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("sqlite_usage_write");
    group.throughput(Throughput::Elements(EVENTS));
    group.sample_size(10);
    group.bench_function("writer_5000_events", |b| {
        b.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let dir = tempfile::tempdir().expect("tempdir");
                let path = dir.path().join("bench.db");
                drop(Database::open(&path).expect("db"));
                let (writer, sender) = TelemetryWriter::spawn(&path, 20_000).expect("spawn");
                let started = Instant::now();
                for i in 0..EVENTS as i64 {
                    sender.send(event(i));
                }
                drop(sender);
                writer.shutdown();
                total += started.elapsed();
            }
            total
        })
    });
    group.finish();
}

criterion_group!(benches, sqlite_usage_write);
criterion_main!(benches);
