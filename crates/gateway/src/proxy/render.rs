// SPDX-License-Identifier: Apache-2.0

//! Turns upstream SSE events into bytes for the client: raw passthrough (minus the usage
//! chunk Loadout injected) or re-encoded canonical events.

use bytes::{Bytes, BytesMut};
use loadout_protocol::hints::is_injected_usage_chunk;
use loadout_protocol::observe::error_event;
use loadout_protocol::sse::SseEvent;
use loadout_protocol::{CanonicalEvent, Protocol, StreamEncoder, encoder_for};

pub(crate) enum Renderer {
    Passthrough {
        ingress: Protocol,
        drop_injected_usage: bool,
    },
    Converted {
        encoder: Box<dyn StreamEncoder>,
    },
}

impl Renderer {
    pub fn passthrough(ingress: Protocol, drop_injected_usage: bool) -> Self {
        Self::Passthrough {
            ingress,
            drop_injected_usage,
        }
    }

    pub fn converted(ingress: Protocol, client_model: &str, include_usage: bool) -> Self {
        Self::Converted {
            encoder: encoder_for(ingress, client_model, include_usage),
        }
    }

    pub fn render(&mut self, event: &SseEvent, canonical: &[CanonicalEvent]) -> Bytes {
        match self {
            Self::Passthrough {
                ingress,
                drop_injected_usage,
            } => {
                if *drop_injected_usage
                    && *ingress == Protocol::Chat
                    && is_injected_usage_chunk(&event.data)
                {
                    Bytes::new()
                } else {
                    event.raw.clone()
                }
            }
            Self::Converted { encoder } => {
                let mut out = BytesMut::new();
                for e in canonical {
                    out.extend_from_slice(&encoder.encode(e));
                }
                out.freeze()
            }
        }
    }

    /// Canonical events from a non-SSE upstream (browser sessions); converted only.
    pub fn render_events(&mut self, canonical: &[CanonicalEvent]) -> Bytes {
        match self {
            Self::Passthrough { .. } => Bytes::new(),
            Self::Converted { encoder } => {
                let mut out = BytesMut::new();
                for e in canonical {
                    out.extend_from_slice(&encoder.encode(e));
                }
                out.freeze()
            }
        }
    }

    /// Bytes to send after the upstream stream ended normally.
    pub fn finish(&mut self) -> Bytes {
        match self {
            Self::Passthrough { .. } => Bytes::new(),
            Self::Converted { encoder } => encoder.finish(),
        }
    }

    /// Bytes reporting an upstream failure after the response was committed.
    pub fn error(&mut self, message: &str) -> Bytes {
        match self {
            Self::Passthrough { ingress, .. } => {
                let mut out = BytesMut::from(&error_event(*ingress, message)[..]);
                if *ingress == Protocol::Chat {
                    out.extend_from_slice(&loadout_protocol::observe::chat_done());
                }
                out.freeze()
            }
            Self::Converted { encoder } => {
                let mut out = BytesMut::from(
                    &encoder.encode(&CanonicalEvent::Error {
                        message: message.to_owned(),
                        kind: Some("upstream_error".into()),
                    })[..],
                );
                out.extend_from_slice(&encoder.finish());
                out.freeze()
            }
        }
    }
}
