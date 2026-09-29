// SPDX-License-Identifier: Apache-2.0

use std::collections::VecDeque;
use std::time::Duration;

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY};

use crate::api::{Hello, HelloParams, HelloResult, Method};
use crate::protocol::codec;
use crate::{ErrorCode, Event, IpcError, PROTOCOL_VERSION, RequestFrame, ServerFrame};

const BUSY_RETRY: Duration = Duration::from_millis(50);
const BUSY_TIMEOUT: Duration = Duration::from_secs(3);

/// Connection to the daemon. Requests are sequential; events received while waiting for a
/// response are queued for [`IpcClient::next_event`].
#[derive(Debug)]
pub struct IpcClient {
    conn: Framed<NamedPipeClient, LengthDelimitedCodec>,
    next_id: u64,
    events: VecDeque<Event>,
    daemon_version: String,
}

impl IpcClient {
    /// Connects and performs the version handshake. Returns [`IpcError::NotRunning`] if no
    /// daemon owns the pipe.
    pub async fn connect(pipe_name: &str, client_name: &str) -> Result<Self, IpcError> {
        let pipe = open_pipe(pipe_name).await?;
        let mut client = Self {
            conn: Framed::new(pipe, codec()),
            next_id: 1,
            events: VecDeque::new(),
            daemon_version: String::new(),
        };
        let params = HelloParams {
            client_name: client_name.to_owned(),
            client_version: env!("CARGO_PKG_VERSION").to_owned(),
        };
        let hello: HelloResult = match client.call::<Hello>(&params).await {
            Ok(h) => h,
            Err(IpcError::Remote(body)) if body.code == ErrorCode::VersionMismatch => {
                return Err(IpcError::VersionMismatch {
                    daemon: body.message,
                    client: PROTOCOL_VERSION,
                });
            }
            Err(e) => return Err(e),
        };
        client.daemon_version = hello.daemon_version;
        Ok(client)
    }

    pub fn daemon_version(&self) -> &str {
        &self.daemon_version
    }

    pub async fn call<M: Method>(&mut self, params: &M::Params) -> Result<M::Result, IpcError> {
        let value = self
            .call_raw(M::NAME, serde_json::to_value(params).map_err(protocol)?)
            .await?;
        serde_json::from_value(value).map_err(protocol)
    }

    /// Sends a request with a raw method name. Prefer [`IpcClient::call`].
    pub async fn call_raw(&mut self, method: &str, params: Value) -> Result<Value, IpcError> {
        let id = self.next_id;
        self.next_id += 1;
        let frame = RequestFrame {
            v: PROTOCOL_VERSION,
            id,
            method: method.to_owned(),
            params,
        };
        let bytes = serde_json::to_vec(&frame).map_err(protocol)?;
        self.conn.send(Bytes::from(bytes)).await?;

        loop {
            let frame = self.read_frame().await?;
            if let Some(event) = frame.event {
                self.events
                    .push_back(Event::new(event, frame.data.unwrap_or(Value::Null)));
                continue;
            }
            match frame.id {
                Some(frame_id) if frame_id == id => {}
                // A response to an unparseable request carries id 0.
                Some(0) => {}
                _ => continue,
            }
            return match (frame.result, frame.error) {
                (_, Some(error)) => Err(IpcError::Remote(error)),
                (Some(result), None) => Ok(result),
                (None, None) => Ok(Value::Null),
            };
        }
    }

    /// Waits for the next event (after `events.subscribe`). `Ok(None)` when the daemon closes
    /// the connection.
    pub async fn next_event(&mut self) -> Result<Option<Event>, IpcError> {
        if let Some(event) = self.events.pop_front() {
            return Ok(Some(event));
        }
        loop {
            let frame = match self.read_frame().await {
                Ok(frame) => frame,
                Err(IpcError::Closed) => return Ok(None),
                Err(e) => return Err(e),
            };
            if let Some(event) = frame.event {
                return Ok(Some(Event::new(event, frame.data.unwrap_or(Value::Null))));
            }
        }
    }

    async fn read_frame(&mut self) -> Result<ServerFrame, IpcError> {
        match self.conn.next().await {
            Some(Ok(bytes)) => serde_json::from_slice(&bytes).map_err(protocol),
            Some(Err(e)) => Err(IpcError::Io(e)),
            None => Err(IpcError::Closed),
        }
    }
}

async fn open_pipe(pipe_name: &str) -> Result<NamedPipeClient, IpcError> {
    let deadline = tokio::time::Instant::now() + BUSY_TIMEOUT;
    loop {
        match ClientOptions::new().open(pipe_name) {
            Ok(pipe) => return Ok(pipe),
            Err(e) if e.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32) => {
                return Err(IpcError::NotRunning);
            }
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                if tokio::time::Instant::now() >= deadline {
                    return Err(IpcError::Io(e));
                }
                tokio::time::sleep(BUSY_RETRY).await;
            }
            Err(e) => return Err(IpcError::Io(e)),
        }
    }
}

fn protocol(e: serde_json::Error) -> IpcError {
    IpcError::Protocol(e.to_string())
}
