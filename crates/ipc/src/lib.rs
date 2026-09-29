// SPDX-License-Identifier: Apache-2.0

//! Control-plane IPC between `loadoutd` and its clients (ADR-002).
//!
//! Transport: per-user Named Pipe restricted to the current user's SID.
//! Framing: 4-byte length prefix + UTF-8 JSON envelope with a protocol version.
