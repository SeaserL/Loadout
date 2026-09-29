// SPDX-License-Identifier: Apache-2.0

//! Real socket tests: bind, serve, port conflicts, graceful shutdown.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use loadout_gateway::{ClientId, ClientKeyIndex, Gateway, GatewayConfig, generate_client_key};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

fn loopback_any_port() -> GatewayConfig {
    GatewayConfig {
        bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        shutdown_grace: Duration::from_secs(2),
        ..GatewayConfig::default()
    }
}

async fn raw_get(addr: SocketAddr, path: &str, key: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.expect("connect");
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {key}\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).await.expect("write");
    let mut response = String::new();
    stream.read_to_string(&mut response).await.expect("read");
    response
}

#[tokio::test]
async fn serves_over_tcp_and_stops_on_shutdown() {
    let key = generate_client_key();
    let keys = Arc::new(ClientKeyIndex::new([(key.hash, ClientId::from("c"))]));
    let gateway = Gateway::bind(loopback_any_port(), keys)
        .await
        .expect("bind");
    let addr = gateway.local_addr();

    let shutdown = CancellationToken::new();
    let task = tokio::spawn(gateway.serve(shutdown.clone()));

    let response = raw_get(addr, "/v1/models", &key.key).await;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains(r#""object":"list""#));

    let started = std::time::Instant::now();
    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("stops in time")
        .expect("join")
        .expect("serve ok");
    // With no in-flight requests shutdown must not wait for the grace period.
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "idle shutdown took {:?}",
        started.elapsed()
    );

    // Rebinding the exact address proves the port was released. (Connecting to a closed
    // loopback port on Windows takes ~2 s to fail, so that check would be slow.)
    let rebound = Gateway::bind(
        GatewayConfig {
            bind: addr,
            ..loopback_any_port()
        },
        Arc::default(),
    )
    .await;
    assert!(rebound.is_ok(), "port must be released: {rebound:?}");
}

#[tokio::test]
async fn port_conflict_is_reported_not_worked_around() {
    let first = Gateway::bind(loopback_any_port(), Arc::default())
        .await
        .expect("bind");
    let taken = GatewayConfig {
        bind: first.local_addr(),
        ..loopback_any_port()
    };
    let err = Gateway::bind(taken, Arc::default())
        .await
        .expect_err("port is taken");
    assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
}
