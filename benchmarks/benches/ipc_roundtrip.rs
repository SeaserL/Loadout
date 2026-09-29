// SPDX-License-Identifier: Apache-2.0

//! Named Pipe request/response round trip (reference only; IPC is not on the hot path).

use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use loadout_ipc::{ErrorBody, Handler, IpcClient, IpcServer};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

struct Echo;

impl Handler for Echo {
    async fn handle(&self, _method: &str, params: Value) -> Result<Value, ErrorBody> {
        Ok(params)
    }
}

fn ipc_roundtrip(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let pipe = format!(r"\\.\pipe\loadout-bench-{}", uuid::Uuid::new_v4());
    let sid = loadout_platform::current_user_sid().expect("sid");
    let shutdown = CancellationToken::new();

    let mut client = runtime.block_on(async {
        let server = IpcServer::bind(&pipe, &sid, "bench").expect("bind");
        tokio::spawn(server.serve(Arc::new(Echo), shutdown.clone()));
        IpcClient::connect(&pipe, "bench").await.expect("connect")
    });

    let payload = json!({ "status": "ok", "n": 1 });
    c.bench_function("ipc_roundtrip/echo", |b| {
        b.iter(|| {
            runtime
                .block_on(client.call_raw("echo", payload.clone()))
                .expect("call")
        })
    });

    shutdown.cancel();
}

criterion_group!(benches, ipc_roundtrip);
criterion_main!(benches);
