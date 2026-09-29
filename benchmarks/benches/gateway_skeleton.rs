// SPDX-License-Identifier: Apache-2.0

//! Loopback HTTP round trip of an authenticated `GET /v1/models` over a keep-alive
//! connection (target P95 < 1 ms). Measures the full gateway middleware stack.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use loadout_gateway::{
    ClientId, ClientKeyIndex, DataPlane, Gateway, GatewayConfig, generate_client_key,
};
use tokio_util::sync::CancellationToken;

fn read_response(reader: &mut BufReader<TcpStream>) -> usize {
    let mut content_length = 0usize;
    let mut line = String::new();
    loop {
        line.clear();
        reader.read_line(&mut line).expect("header line");
        if line == "\r\n" {
            break;
        }
        if let Some(v) = line
            .to_ascii_lowercase()
            .strip_prefix("content-length:")
            .map(str::trim)
            .map(str::to_owned)
        {
            content_length = v.parse().expect("length");
        }
    }
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body).expect("body");
    body.len()
}

fn gateway_skeleton(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime");
    let key = generate_client_key();
    let keys = Arc::new(ClientKeyIndex::new([(key.hash, ClientId::from("bench"))]));
    let shutdown = CancellationToken::new();
    let addr: SocketAddr = runtime.block_on(async {
        let gateway = Gateway::bind(
            GatewayConfig {
                bind: SocketAddr::from(([127, 0, 0, 1], 0)),
                ..GatewayConfig::default()
            },
            keys,
            Arc::new(DataPlane::empty()),
        )
        .await
        .expect("bind");
        let addr = gateway.local_addr();
        tokio::spawn(gateway.serve(shutdown.clone()));
        addr
    });

    let stream = TcpStream::connect(addr).expect("connect");
    stream.set_nodelay(true).expect("nodelay");
    let mut writer = stream.try_clone().expect("clone");
    let mut reader = BufReader::new(stream);
    let request = format!(
        "GET /v1/models HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {}\r\n\r\n",
        key.key
    );

    c.bench_function("gateway_skeleton/get_models_keepalive", |b| {
        b.iter(|| {
            writer.write_all(request.as_bytes()).expect("write");
            read_response(&mut reader)
        })
    });

    shutdown.cancel();
}

criterion_group!(benches, gateway_skeleton);
criterion_main!(benches);
