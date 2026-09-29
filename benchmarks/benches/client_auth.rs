// SPDX-License-Identifier: Apache-2.0

//! Header parsing + SHA-256 + index lookup for one request (target P95 < 20 µs).

use std::hint::black_box;

use axum::http::{HeaderMap, HeaderValue};
use criterion::{Criterion, criterion_group, criterion_main};
use loadout_gateway::{ClientId, ClientKeyIndex, generate_client_key};

fn client_auth(c: &mut Criterion) {
    let keys: Vec<_> = (0..100).map(|_| generate_client_key()).collect();
    let index = ClientKeyIndex::new(
        keys.iter()
            .enumerate()
            .map(|(i, k)| (k.hash, ClientId::from(format!("client-{i}")))),
    );

    let mut bearer = HeaderMap::new();
    bearer.insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {}", keys[42].key)).expect("header"),
    );
    c.bench_function("client_auth/valid_bearer", |b| {
        b.iter(|| index.authenticate(black_box(&bearer)))
    });

    let mut wrong = HeaderMap::new();
    wrong.insert(
        "x-api-key",
        HeaderValue::from_str(&generate_client_key().key).expect("header"),
    );
    c.bench_function("client_auth/unknown_key", |b| {
        b.iter(|| index.authenticate(black_box(&wrong)))
    });
}

criterion_group!(benches, client_auth);
criterion_main!(benches);
