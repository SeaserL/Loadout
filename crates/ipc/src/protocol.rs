// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::codec::LengthDelimitedCodec;

use crate::ErrorBody;

/// Bumped on any incompatible change to frames or method semantics.
pub const PROTOCOL_VERSION: u32 = 1;

/// Upper bound for one frame; larger frames close the connection.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

pub(crate) fn codec() -> LengthDelimitedCodec {
    LengthDelimitedCodec::builder()
        .length_field_length(4)
        .max_frame_length(MAX_FRAME_BYTES)
        .new_codec()
}

/// Client → daemon.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RequestFrame {
    pub v: u32,
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// Daemon → client: either a response (`id` + `result`/`error`) or an event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ServerFrame {
    pub v: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl ServerFrame {
    pub fn response(id: u64, outcome: Result<Value, ErrorBody>) -> Self {
        let (result, error) = match outcome {
            Ok(v) => (Some(v), None),
            Err(e) => (None, Some(e)),
        };
        Self {
            v: PROTOCOL_VERSION,
            id: Some(id),
            result,
            error,
            event: None,
            data: None,
        }
    }

    pub fn event(event: &Event) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            id: None,
            result: None,
            error: None,
            event: Some(event.name.clone()),
            data: Some(event.data.clone()),
        }
    }
}

/// Daemon-pushed notification, delivered to connections that called `events.subscribe`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub name: String,
    pub data: Value,
}

impl Event {
    pub fn new(name: impl Into<String>, data: Value) -> Self {
        Self {
            name: name.into(),
            data,
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::ErrorCode;

    #[test]
    fn response_frames_omit_unused_fields() {
        let ok = serde_json::to_value(ServerFrame::response(7, Ok(json!({"a": 1})))).expect("json");
        assert_eq!(ok, json!({"v": 1, "id": 7, "result": {"a": 1}}));

        let err = serde_json::to_value(ServerFrame::response(
            8,
            Err(ErrorBody::new(ErrorCode::NotFound, "nope")),
        ))
        .expect("json");
        assert_eq!(
            err,
            json!({"v": 1, "id": 8, "error": {"code": "not_found", "message": "nope"}})
        );

        let ev =
            serde_json::to_value(ServerFrame::event(&Event::new("x", json!(null)))).expect("json");
        assert_eq!(ev, json!({"v": 1, "event": "x", "data": null}));
    }

    #[test]
    fn unknown_error_codes_are_tolerated() {
        let body: ErrorBody =
            serde_json::from_value(json!({"code": "from_the_future", "message": "m"}))
                .expect("parse");
        assert_eq!(body.code, ErrorCode::Unknown);
    }

    #[test]
    fn request_params_default_to_null() {
        let req: RequestFrame =
            serde_json::from_value(json!({"v": 1, "id": 1, "method": "daemon.status"}))
                .expect("parse");
        assert_eq!(req.params, Value::Null);
    }
}
