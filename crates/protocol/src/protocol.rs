// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use bytes::Bytes;

use crate::sse::SseEvent;
use crate::{CanonicalEvent, ProtocolError, chat, messages, responses};

/// The three wire protocols Loadout speaks, both to clients and to upstreams.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Protocol {
    /// OpenAI Chat Completions (`/v1/chat/completions`).
    Chat,
    /// OpenAI Responses (`/v1/responses`).
    Responses,
    /// Anthropic Messages (`/v1/messages`).
    Messages,
}

impl Protocol {
    pub const ALL: [Protocol; 3] = [Protocol::Chat, Protocol::Responses, Protocol::Messages];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Responses => "responses",
            Self::Messages => "messages",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "chat" => Some(Self::Chat),
            "responses" => Some(Self::Responses),
            "messages" => Some(Self::Messages),
            _ => None,
        }
    }

    /// Ingress protocol for a gateway path.
    pub fn from_path(path: &str) -> Option<Self> {
        match path {
            "/v1/chat/completions" => Some(Self::Chat),
            "/v1/responses" => Some(Self::Responses),
            "/v1/messages" => Some(Self::Messages),
            _ => None,
        }
    }

    /// Path appended to an upstream base URL (which ends in `/v1` or similar).
    pub fn upstream_path(self) -> &'static str {
        match self {
            Self::Chat => "/chat/completions",
            Self::Responses => "/responses",
            Self::Messages => "/messages",
        }
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Turns upstream SSE events into canonical events.
pub trait StreamDecoder: Send {
    fn feed(&mut self, event: &SseEvent) -> Result<Vec<CanonicalEvent>, ProtocolError>;

    /// True once the upstream signalled the end of the stream.
    fn is_done(&self) -> bool;
}

/// Turns canonical events into client SSE bytes.
pub trait StreamEncoder: Send {
    fn encode(&mut self, event: &CanonicalEvent) -> Bytes;

    /// Emits everything still pending (closing blocks, final usage, terminators).
    fn finish(&mut self) -> Bytes;
}

pub fn decoder_for(protocol: Protocol) -> Box<dyn StreamDecoder> {
    match protocol {
        Protocol::Chat => Box::new(chat::ChatStreamDecoder::default()),
        Protocol::Responses => Box::new(responses::ResponsesStreamDecoder::default()),
        Protocol::Messages => Box::new(messages::MessagesStreamDecoder::default()),
    }
}

/// `model` is the name reported back to the client; `include_usage` applies to Chat (which
/// only sends a usage chunk when the client asked for it).
pub fn encoder_for(protocol: Protocol, model: &str, include_usage: bool) -> Box<dyn StreamEncoder> {
    match protocol {
        Protocol::Chat => Box::new(chat::ChatStreamEncoder::new(model, include_usage)),
        Protocol::Responses => Box::new(responses::ResponsesStreamEncoder::new(model)),
        Protocol::Messages => Box::new(messages::MessagesStreamEncoder::new(model)),
    }
}
