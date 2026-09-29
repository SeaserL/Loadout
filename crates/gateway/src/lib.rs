// SPDX-License-Identifier: Apache-2.0

//! The data-plane HTTP gateway bound to loopback (ADR-011).
//!
//! The request path must never touch the database or the network before route selection
//! (Harness §2.2); all lookups here are in-memory.

mod auth;
mod error;
mod plane;
mod proxy;
mod routes;
mod security;
mod server;

pub use auth::{
    AuthError, ClientKeyIndex, GeneratedKey, KEY_PREFIX, MAX_KEY_LEN, generate_client_key,
    hash_client_key,
};
pub use error::{ApiError, ApiFlavor, ErrorCategory};
pub use plane::{
    DataPlane, NullSink, RequestRecord, ResponseRouteStore, TelemetrySink, UpstreamLimits,
};
pub use server::{Gateway, GatewayConfig};

/// Identifier of the authenticated client, attached to each request as an extension.
pub type ClientId = std::sync::Arc<str>;
