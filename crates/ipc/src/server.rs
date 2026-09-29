// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::broadcast;
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tokio_util::sync::CancellationToken;

use crate::api::{EventsSubscribe, Hello, HelloParams, HelloResult, Method};
use crate::protocol::codec;
use crate::security::PipeSecurity;
use crate::{ErrorBody, ErrorCode, Event, PROTOCOL_VERSION, RequestFrame, ServerFrame};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const EVENT_BUFFER: usize = 256;

/// Handles decoded requests. `hello` and `events.subscribe` are answered by the server itself.
pub trait Handler: Send + Sync + 'static {
    fn handle(
        &self,
        method: &str,
        params: Value,
    ) -> impl Future<Output = Result<Value, ErrorBody>> + Send;
}

pub fn decode_params<M: Method>(params: Value) -> Result<M::Params, ErrorBody> {
    serde_json::from_value(params)
        .map_err(|e| ErrorBody::invalid_params(format!("{}: {e}", M::NAME)))
}

pub fn encode_result<M: Method>(result: &M::Result) -> Result<Value, ErrorBody> {
    serde_json::to_value(result).map_err(|e| ErrorBody::internal(e.to_string()))
}

/// Named Pipe server. Creating it claims the pipe name (`FILE_FLAG_FIRST_PIPE_INSTANCE`), so a
/// second daemon — or another process squatting on the name — fails fast.
#[derive(Debug)]
pub struct IpcServer {
    pipe_name: String,
    security: Arc<PipeSecurity>,
    first: Option<NamedPipeServer>,
    events: broadcast::Sender<Event>,
    daemon_version: String,
}

impl IpcServer {
    /// Binds the pipe, readable/writable only by `user_sid` and LocalSystem.
    pub fn bind(pipe_name: &str, user_sid: &str, daemon_version: &str) -> io::Result<Self> {
        let security = Arc::new(PipeSecurity::for_user(user_sid)?);
        let first = create_instance(pipe_name, &security, true)?;
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        Ok(Self {
            pipe_name: pipe_name.to_owned(),
            security,
            first: Some(first),
            events,
            daemon_version: daemon_version.to_owned(),
        })
    }

    /// Sender for pushing events to subscribed connections.
    pub fn events(&self) -> broadcast::Sender<Event> {
        self.events.clone()
    }

    /// Accepts connections until `shutdown` is cancelled. Each connection runs on its own task
    /// and is closed when `shutdown` fires.
    pub async fn serve<H: Handler>(
        mut self,
        handler: Arc<H>,
        shutdown: CancellationToken,
    ) -> io::Result<()> {
        let mut next = match self.first.take() {
            Some(first) => first,
            None => create_instance(&self.pipe_name, &self.security, false)?,
        };
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => return Ok(()),
                connected = next.connect() => {
                    if let Err(e) = connected {
                        tracing::warn!(error = %e, "IPC connect failed");
                    }
                    // Create the next instance before handing this one off so clients never
                    // observe a moment with no listening instance.
                    let conn = std::mem::replace(
                        &mut next,
                        create_instance(&self.pipe_name, &self.security, false)?,
                    );
                    tokio::spawn(serve_connection(
                        conn,
                        handler.clone(),
                        self.events.clone(),
                        self.daemon_version.clone(),
                        shutdown.clone(),
                    ));
                }
            }
        }
    }
}

#[allow(unsafe_code)]
fn create_instance(
    name: &str,
    security: &PipeSecurity,
    first: bool,
) -> io::Result<NamedPipeServer> {
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first)
        .reject_remote_clients(true);
    // SAFETY: `security.as_raw()` points to SECURITY_ATTRIBUTES whose descriptor stays alive
    // for the duration of the call (the Arc is held by the caller).
    unsafe { options.create_with_security_attributes_raw(name, security.as_raw()) }
}

type Conn = Framed<NamedPipeServer, LengthDelimitedCodec>;

async fn send(conn: &mut Conn, frame: &ServerFrame) -> bool {
    match serde_json::to_vec(frame) {
        Ok(bytes) => conn.send(Bytes::from(bytes)).await.is_ok(),
        Err(e) => {
            tracing::error!(error = %e, "failed to encode IPC frame");
            false
        }
    }
}

async fn serve_connection<H: Handler>(
    pipe: NamedPipeServer,
    handler: Arc<H>,
    events: broadcast::Sender<Event>,
    daemon_version: String,
    shutdown: CancellationToken,
) {
    let mut conn = Framed::new(pipe, codec());

    // Handshake: the first frame must be a `hello` with our protocol version.
    let first = match tokio::time::timeout(HANDSHAKE_TIMEOUT, conn.next()).await {
        Ok(Some(Ok(bytes))) => bytes,
        _ => return,
    };
    let Ok(hello) = serde_json::from_slice::<RequestFrame>(&first) else {
        let err = ErrorBody::new(ErrorCode::InvalidRequest, "malformed hello frame");
        send(&mut conn, &ServerFrame::response(0, Err(err))).await;
        return;
    };
    if hello.method != Hello::NAME {
        let err = ErrorBody::new(ErrorCode::InvalidRequest, "first request must be hello");
        send(&mut conn, &ServerFrame::response(hello.id, Err(err))).await;
        return;
    }
    if hello.v != PROTOCOL_VERSION {
        let err = ErrorBody::new(
            ErrorCode::VersionMismatch,
            format!("daemon {daemon_version} speaks protocol {PROTOCOL_VERSION}"),
        );
        send(&mut conn, &ServerFrame::response(hello.id, Err(err))).await;
        return;
    }
    if let Ok(params) = serde_json::from_value::<HelloParams>(hello.params) {
        tracing::debug!(client = %params.client_name, version = %params.client_version, "IPC client connected");
    }
    let result = HelloResult {
        protocol: PROTOCOL_VERSION,
        daemon_version,
    };
    let reply = ServerFrame::response(hello.id, encode_result::<Hello>(&result));
    if !send(&mut conn, &reply).await {
        return;
    }

    let mut subscription: Option<broadcast::Receiver<Event>> = None;
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            frame = conn.next() => {
                let bytes = match frame {
                    Some(Ok(bytes)) => bytes,
                    Some(Err(e)) => {
                        tracing::debug!(error = %e, "IPC connection error");
                        return;
                    }
                    None => return,
                };
                let request = match serde_json::from_slice::<RequestFrame>(&bytes) {
                    Ok(r) => r,
                    Err(e) => {
                        let err = ErrorBody::new(ErrorCode::InvalidRequest, e.to_string());
                        if !send(&mut conn, &ServerFrame::response(0, Err(err))).await {
                            return;
                        }
                        continue;
                    }
                };
                let outcome = if request.method == EventsSubscribe::NAME {
                    subscription = Some(events.subscribe());
                    Ok(Value::Null)
                } else if request.method == Hello::NAME {
                    Err(ErrorBody::new(ErrorCode::InvalidRequest, "hello already completed"))
                } else {
                    handler.handle(&request.method, request.params).await
                };
                if !send(&mut conn, &ServerFrame::response(request.id, outcome)).await {
                    return;
                }
            }
            event = next_event(&mut subscription) => {
                if let Some(event) = event
                    && !send(&mut conn, &ServerFrame::event(&event)).await
                {
                    return;
                }
            }
        }
    }
}

/// Next event for a subscribed connection; pending forever when not subscribed.
async fn next_event(subscription: &mut Option<broadcast::Receiver<Event>>) -> Option<Event> {
    let Some(rx) = subscription else {
        return std::future::pending().await;
    };
    loop {
        match rx.recv().await {
            Ok(event) => return Some(event),
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                tracing::warn!(skipped, "IPC subscriber lagged; events dropped");
            }
            Err(broadcast::error::RecvError::Closed) => {
                *subscription = None;
                return None;
            }
        }
    }
}
