// SPDX-License-Identifier: Apache-2.0

//! Route table publication (Harness §20, ADR-003 §2): building a 100-target table with
//! virtual models, swapping it in, and the request-side `load()`, including under a
//! concurrent writer.

use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use criterion::{Criterion, criterion_group, criterion_main};
use loadout_protocol::Protocol;
use loadout_provider_sdk::{Capabilities, Endpoint, ProviderConfig, ProviderModule, TrustLevel};
use loadout_providers::OpenAiCompatible;
use loadout_router::{HealthRegistry, Prices, RouteTable, RouteTableHandle, RouteTarget, TargetId};
use serde_json::Value;

fn targets(reg: &HealthRegistry) -> Vec<Arc<RouteTarget>> {
    let module: Arc<dyn ProviderModule> = Arc::new(OpenAiCompatible);
    let config = Arc::new(ProviderConfig {
        slug: "p".into(),
        kind: "openai-compatible".into(),
        base_url: Some("https://x.test/v1".into()),
        settings: Value::Null,
    });
    let mut out = Vec::new();
    for account in 0..10 {
        for model in 0..10 {
            let id: TargetId = Arc::from(format!("a{account}-m{model}:chat"));
            out.push(Arc::new(RouteTarget {
                health: reg.get_or_create(&id),
                id,
                provider_slug: Arc::from(format!("p{}", account % 3).as_str()),
                account_id: Arc::from(format!("a{account}").as_str()),
                account_alias: Arc::from(format!("acct{account}").as_str()),
                account_position: account,
                upstream_model: Arc::from(format!("model-{model}").as_str()),
                protocol: Protocol::Chat,
                trust: TrustLevel::OfficialApi,
                opted_in: false,
                context_window: Some(128_000),
                max_output_tokens: None,
                capabilities: Capabilities::DEFAULT_MODEL,
                prices: Prices::default(),
                endpoint: Endpoint::new("https://x.test/v1/chat/completions".into()),
                module: module.clone(),
                config: config.clone(),
            }));
        }
    }
    out
}

fn build(version: u64, targets: &[Arc<RouteTarget>]) -> RouteTable {
    let ids: Vec<TargetId> = targets.iter().map(|t| t.id.clone()).collect();
    RouteTable::new(version, targets.to_vec())
        .with_virtual("default", &ids)
        .with_virtual("fast", &ids[..50])
        .with_virtual("realtime", &ids[..10])
}

fn snapshot_swap(c: &mut Criterion) {
    let reg = HealthRegistry::default();
    let targets = targets(&reg);
    let handle = Arc::new(RouteTableHandle::new(build(0, &targets)));

    c.bench_function("snapshot_swap/build_100_targets_3_virtuals", |b| {
        b.iter(|| build(black_box(1), &targets))
    });
    c.bench_function("snapshot_swap/store", |b| {
        let mut v = 0;
        b.iter_batched(
            || {
                v += 1;
                build(v, &targets)
            },
            |table| handle.store(table),
            criterion::BatchSize::SmallInput,
        )
    });
    c.bench_function("snapshot_swap/load", |b| {
        b.iter(|| black_box(handle.load()).version())
    });

    // Readers keep going while the control plane republishes continuously.
    let stop = Arc::new(AtomicBool::new(false));
    let writer = {
        let (handle, stop, targets) = (handle.clone(), stop.clone(), targets.clone());
        std::thread::spawn(move || {
            let mut v = 0;
            while !stop.load(Ordering::Relaxed) {
                v += 1;
                handle.store(build(v, &targets));
            }
        })
    };
    c.bench_function("snapshot_swap/load_during_swaps", |b| {
        b.iter(|| black_box(handle.load()).version())
    });
    stop.store(true, Ordering::Relaxed);
    let _ = writer.join();
}

criterion_group!(benches, snapshot_swap);
criterion_main!(benches);
