// SPDX-License-Identifier: Apache-2.0

//! End-to-end tests over real Named Pipes with unique names.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use loadout_ipc::api::EventsSubscribe;
use loadout_ipc::{
    ErrorBody, ErrorCode, Event, Handler, IpcClient, IpcError, IpcServer, RequestFrame, ServerFrame,
};
use serde_json::{Value, json};
use tokio::net::windows::named_pipe::ClientOptions;
use tokio::task::JoinHandle;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tokio_util::sync::CancellationToken;

struct TestHandler;

impl Handler for TestHandler {
    async fn handle(&self, method: &str, params: Value) -> Result<Value, ErrorBody> {
        match method {
            "test.echo" => Ok(params),
            "test.missing" => Err(ErrorBody::not_found("no such thing")),
            other => Err(ErrorBody::new(
                ErrorCode::MethodNotFound,
                format!("unknown method {other}"),
            )),
        }
    }
}

struct Running {
    pipe: String,
    events: tokio::sync::broadcast::Sender<Event>,
    shutdown: CancellationToken,
    task: JoinHandle<std::io::Result<()>>,
}

fn unique_pipe() -> String {
    format!(r"\\.\pipe\loadout-test-{}", uuid::Uuid::new_v4())
}

fn start() -> Running {
    let pipe = unique_pipe();
    let sid = loadout_platform::current_user_sid().expect("sid");
    let server = IpcServer::bind(&pipe, &sid, "9.9.9-test").expect("bind");
    let events = server.events();
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(server.serve(Arc::new(TestHandler), shutdown.clone()));
    Running {
        pipe,
        events,
        shutdown,
        task,
    }
}

#[tokio::test]
async fn handshake_and_roundtrip() {
    let server = start();
    let mut client = IpcClient::connect(&server.pipe, "test")
        .await
        .expect("connect");
    assert_eq!(client.daemon_version(), "9.9.9-test");

    let payload = json!({"hello": "世界", "n": [1, 2, 3]});
    let echoed = client
        .call_raw("test.echo", payload.clone())
        .await
        .expect("echo");
    assert_eq!(echoed, payload);
    server.shutdown.cancel();
}

#[tokio::test]
async fn remote_errors_are_typed() {
    let server = start();
    let mut client = IpcClient::connect(&server.pipe, "test")
        .await
        .expect("connect");

    match client.call_raw("test.missing", Value::Null).await {
        Err(IpcError::Remote(body)) => assert_eq!(body.code, ErrorCode::NotFound),
        other => panic!("unexpected {other:?}"),
    }
    match client.call_raw("nope", Value::Null).await {
        Err(IpcError::Remote(body)) => assert_eq!(body.code, ErrorCode::MethodNotFound),
        other => panic!("unexpected {other:?}"),
    }
    // The connection stays usable after errors.
    assert_eq!(
        client.call_raw("test.echo", json!(1)).await.expect("echo"),
        json!(1)
    );
    server.shutdown.cancel();
}

#[tokio::test]
async fn events_reach_only_subscribers() {
    let server = start();
    let mut subscriber = IpcClient::connect(&server.pipe, "sub")
        .await
        .expect("connect");
    let mut other = IpcClient::connect(&server.pipe, "other")
        .await
        .expect("connect");
    subscriber
        .call::<EventsSubscribe>(&())
        .await
        .expect("subscribe");

    server
        .events
        .send(Event::new("clients.changed", json!({"n": 1})))
        .expect("send event");

    let event = tokio::time::timeout(Duration::from_secs(5), subscriber.next_event())
        .await
        .expect("event in time")
        .expect("event ok")
        .expect("some event");
    assert_eq!(event.name, "clients.changed");
    assert_eq!(event.data, json!({"n": 1}));

    // The unsubscribed connection gets a plain response with no queued event.
    assert_eq!(
        other.call_raw("test.echo", json!("x")).await.expect("echo"),
        json!("x")
    );
    let pending = tokio::time::timeout(Duration::from_millis(200), other.next_event()).await;
    assert!(
        pending.is_err(),
        "unsubscribed client must not receive events"
    );
    server.shutdown.cancel();
}

#[tokio::test]
async fn second_server_on_same_name_is_rejected() {
    let server = start();
    let sid = loadout_platform::current_user_sid().expect("sid");
    assert!(IpcServer::bind(&server.pipe, &sid, "dup").is_err());
    server.shutdown.cancel();
}

#[tokio::test]
async fn missing_daemon_is_not_running() {
    match IpcClient::connect(&unique_pipe(), "test").await {
        Err(IpcError::NotRunning) => {}
        other => panic!("expected NotRunning, got {other:?}"),
    }
}

async fn raw_hello(pipe: &str, v: u32, method: &str) -> ServerFrame {
    let conn = ClientOptions::new().open(pipe).expect("open");
    let mut framed = Framed::new(conn, LengthDelimitedCodec::new());
    let hello = RequestFrame {
        v,
        id: 1,
        method: method.to_owned(),
        params: json!({"client_name": "raw", "client_version": "0"}),
    };
    framed
        .send(Bytes::from(serde_json::to_vec(&hello).expect("json")))
        .await
        .expect("send");
    let bytes = framed.next().await.expect("frame").expect("bytes");
    serde_json::from_slice(&bytes).expect("server frame")
}

#[tokio::test]
async fn protocol_version_mismatch_is_reported() {
    let server = start();
    let frame = raw_hello(&server.pipe, 99, "hello").await;
    assert_eq!(
        frame.error.map(|e| e.code),
        Some(ErrorCode::VersionMismatch)
    );
    server.shutdown.cancel();
}

#[tokio::test]
async fn first_request_must_be_hello() {
    let server = start();
    let frame = raw_hello(&server.pipe, 1, "test.echo").await;
    assert_eq!(frame.error.map(|e| e.code), Some(ErrorCode::InvalidRequest));
    server.shutdown.cancel();
}

#[tokio::test]
async fn many_concurrent_clients() {
    let server = start();
    let mut tasks = Vec::new();
    for c in 0..16 {
        let pipe = server.pipe.clone();
        tasks.push(tokio::spawn(async move {
            let mut client = IpcClient::connect(&pipe, "load").await.expect("connect");
            for i in 0..25 {
                let v = json!([c, i]);
                assert_eq!(
                    client.call_raw("test.echo", v.clone()).await.expect("echo"),
                    v
                );
            }
        }));
    }
    for t in tasks {
        t.await.expect("task");
    }
    server.shutdown.cancel();
}

#[tokio::test]
async fn shutdown_stops_server_and_closes_connections() {
    let server = start();
    let mut client = IpcClient::connect(&server.pipe, "test")
        .await
        .expect("connect");
    client
        .call::<EventsSubscribe>(&())
        .await
        .expect("subscribe");

    server.shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(5), server.task)
        .await
        .expect("server stops in time")
        .expect("join")
        .expect("serve ok");

    let next = tokio::time::timeout(Duration::from_secs(5), client.next_event())
        .await
        .expect("closed in time");
    assert!(matches!(next, Ok(None) | Err(IpcError::Io(_))));
}

#[test]
fn method_names_are_unique() {
    use loadout_ipc::api::*;
    let names = [
        Hello::NAME,
        EventsSubscribe::NAME,
        Status::NAME,
        Shutdown::NAME,
        ClientCreate::NAME,
        ClientList::NAME,
        ClientRevoke::NAME,
        SettingsList::NAME,
        SettingsSet::NAME,
        SettingsReset::NAME,
    ];
    let unique: std::collections::HashSet<_> = names.iter().collect();
    assert_eq!(unique.len(), names.len());
}
