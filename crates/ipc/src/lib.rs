// SPDX-License-Identifier: Apache-2.0

//! Control-plane IPC between `loadoutd` and its clients (ADR-002).
//!
//! Transport: per-user Named Pipe restricted to the current user's SID.
//! Framing: 4-byte length prefix + UTF-8 JSON envelope with a protocol version.
//!
//! [`api`] defines every method with typed params/results so the daemon, the CLI and the
//! desktop app share one definition.

pub mod api;
mod client;
mod error;
mod protocol;
mod security;
mod server;

pub use client::IpcClient;
pub use error::{ErrorBody, ErrorCode, IpcError};
pub use protocol::{Event, MAX_FRAME_BYTES, PROTOCOL_VERSION, RequestFrame, ServerFrame};
pub use server::{Handler, IpcServer, decode_params, encode_result};

/// Pipe name for a user: `\\.\pipe\loadout-<sid>`.
pub fn pipe_name_for(sid: &str) -> String {
    format!(r"\\.\pipe\loadout-{sid}")
}
