// SPDX-License-Identifier: Apache-2.0

//! Protocol layer (ADR-006). Pure functions, no I/O.
//!
//! - [`canonical`]: the protocol-neutral request / event / response model.
//! - [`chat`], [`messages`], [`responses`]: codecs for OpenAI Chat Completions, Anthropic
//!   Messages and OpenAI Responses. Each provides `decode_request`, `encode_request`,
//!   a `StreamDecoder` (upstream SSE → canonical events), a `StreamEncoder` (canonical
//!   events → client SSE) and `encode_response` (non-streaming).
//! - [`sse`]: incremental Server-Sent Events parsing and serialization.
//! - [`hints`]: the few fields routing needs, extracted without a full decode.
//!
//! Conversion always streams from upstream; non-streaming clients get an aggregated
//! response built with [`Aggregator`].

pub mod aggregate;
pub mod canonical;
pub mod chat;
mod error;
pub mod hints;
pub mod messages;
mod protocol;
pub mod responses;
pub mod sse;
mod util;

pub use aggregate::Aggregator;
pub use canonical::*;
pub use error::ProtocolError;
pub use protocol::{Protocol, StreamDecoder, StreamEncoder, decoder_for, encoder_for};
