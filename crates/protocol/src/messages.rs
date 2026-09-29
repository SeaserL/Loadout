// SPDX-License-Identifier: Apache-2.0

//! Anthropic Messages codec.
//!
//! Usage: Anthropic reports `input_tokens` *excluding* cache reads/writes. Canonical input
//! is the total (see [`crate::chat`]), so decoding adds the cache fields and encoding
//! subtracts them.

use std::collections::HashMap;

use bytes::{Bytes, BytesMut};
use serde_json::{Map, Value, json};

use crate::chat::push_tool_result;
use crate::sse::{SseEvent, encode_json};
use crate::util::{
    arguments_to_object, bool_field, f64_field, fallback_id, require_str, str_field,
    string_or_array, u32_field, u64_field,
};
use crate::{
    CanonicalEvent, CanonicalRequest, CanonicalResponse, Image, Message, Part, ProtocolError,
    Reasoning, Role, StopReason, StreamDecoder, StreamEncoder, Tool, ToolChoice, ToolResultContent,
    Usage,
};

/// `max_tokens` is mandatory in Anthropic requests; used when the client gave none.
pub const DEFAULT_MAX_TOKENS: u32 = 8192;

// ---- request decoding ----------------------------------------------------------------------

pub fn decode_request(body: &Value) -> Result<CanonicalRequest, ProtocolError> {
    let mut req = CanonicalRequest {
        model: require_str(body, "model", "messages request")?.to_owned(),
        stream: bool_field(body, "stream").unwrap_or(false),
        max_output_tokens: u32_field(body, "max_tokens"),
        temperature: f64_field(body, "temperature"),
        top_p: f64_field(body, "top_p"),
        stop: string_or_array(body.get("stop_sequences")),
        user: body
            .get("metadata")
            .and_then(|m| str_field(m, "user_id"))
            .map(str::to_owned),
        ..CanonicalRequest::default()
    };

    match body.get("system") {
        Some(Value::String(s)) if !s.is_empty() => req.instructions.push(s.clone()),
        Some(Value::Array(blocks)) => {
            let text: Vec<&str> = blocks.iter().filter_map(|b| str_field(b, "text")).collect();
            if !text.is_empty() {
                req.instructions.push(text.join("\n"));
            }
        }
        _ => {}
    }

    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| ProtocolError::invalid("messages request: `messages` must be an array"))?;
    for msg in messages {
        let role = match require_str(msg, "role", "message")? {
            "user" => Role::User,
            "assistant" => Role::Assistant,
            other => return Err(ProtocolError::invalid(format!("role {other:?}"))),
        };
        let blocks = match msg.get("content") {
            Some(Value::String(s)) => vec![json!({ "type": "text", "text": s })],
            Some(Value::Array(a)) => a.clone(),
            _ => {
                return Err(ProtocolError::invalid(
                    "message content must be a string or array",
                ));
            }
        };
        let mut parts = Vec::new();
        for block in &blocks {
            match decode_block(block)? {
                Some(part @ Part::ToolResult { .. }) if role == Role::User => {
                    if !parts.is_empty() {
                        req.messages
                            .push(Message::new(role, std::mem::take(&mut parts)));
                    }
                    push_tool_result(&mut req, part);
                }
                Some(part) => parts.push(part),
                None => {}
            }
        }
        if !parts.is_empty() || blocks.is_empty() {
            req.messages.push(Message::new(role, parts));
        }
    }

    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        for tool in tools {
            match str_field(tool, "type") {
                None | Some("custom") => {}
                Some(other) => {
                    return Err(ProtocolError::unsupported(format!("server tool {other:?}")));
                }
            }
            req.tools.push(Tool {
                name: require_str(tool, "name", "tool")?.to_owned(),
                description: str_field(tool, "description").map(str::to_owned),
                parameters: tool
                    .get("input_schema")
                    .cloned()
                    .unwrap_or(json!({"type": "object"})),
            });
        }
    }
    if let Some(tc) = body.get("tool_choice") {
        req.tool_choice = match str_field(tc, "type") {
            Some("auto") => Some(ToolChoice::Auto),
            Some("any") => Some(ToolChoice::Required),
            Some("none") => Some(ToolChoice::None),
            Some("tool") => Some(ToolChoice::Named(
                require_str(tc, "name", "tool_choice")?.to_owned(),
            )),
            _ => None,
        };
        if bool_field(tc, "disable_parallel_tool_use") == Some(true) {
            req.parallel_tool_calls = Some(false);
        }
    }
    if let Some(thinking) = body.get("thinking") {
        match str_field(thinking, "type") {
            Some("enabled") => {
                req.reasoning = Some(Reasoning {
                    effort: None,
                    budget_tokens: u32_field(thinking, "budget_tokens"),
                });
            }
            Some("disabled") | None => {}
            Some(_) => {
                req.reasoning = Some(Reasoning {
                    effort: None,
                    budget_tokens: None,
                });
            }
        }
    }
    Ok(req)
}

fn decode_image(source: &Value) -> Result<Image, ProtocolError> {
    match str_field(source, "type") {
        Some("base64") => Ok(Image::Base64 {
            media_type: require_str(source, "media_type", "image")?.to_owned(),
            data: require_str(source, "data", "image")?.to_owned(),
        }),
        Some("url") => Ok(Image::Url(require_str(source, "url", "image")?.to_owned())),
        other => Err(ProtocolError::unsupported(format!(
            "image source {:?}",
            other.unwrap_or("?")
        ))),
    }
}

fn decode_block(block: &Value) -> Result<Option<Part>, ProtocolError> {
    Ok(Some(match str_field(block, "type") {
        Some("text") => Part::Text(str_field(block, "text").unwrap_or("").to_owned()),
        Some("image") => Part::Image(decode_image(block.get("source").unwrap_or(&Value::Null))?),
        Some("tool_use") => Part::ToolCall {
            id: require_str(block, "id", "tool_use")?.to_owned(),
            name: require_str(block, "name", "tool_use")?.to_owned(),
            arguments: block
                .get("input")
                .map(Value::to_string)
                .unwrap_or_else(|| "{}".into()),
        },
        Some("tool_result") => {
            let content = match block.get("content") {
                Some(Value::String(s)) => vec![ToolResultContent::Text(s.clone())],
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|i| match str_field(i, "type") {
                        Some("text") => Ok(ToolResultContent::Text(
                            str_field(i, "text").unwrap_or("").to_owned(),
                        )),
                        Some("image") => Ok(ToolResultContent::Image(decode_image(
                            i.get("source").unwrap_or(&Value::Null),
                        )?)),
                        other => Err(ProtocolError::unsupported(format!(
                            "tool_result content {:?}",
                            other.unwrap_or("?")
                        ))),
                    })
                    .collect::<Result<_, _>>()?,
                _ => Vec::new(),
            };
            Part::ToolResult {
                call_id: require_str(block, "tool_use_id", "tool_result")?.to_owned(),
                content,
                is_error: bool_field(block, "is_error").unwrap_or(false),
            }
        }
        Some("thinking") => Part::Reasoning {
            text: str_field(block, "thinking").unwrap_or("").to_owned(),
            signature: str_field(block, "signature")
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            redacted: None,
        },
        Some("redacted_thinking") => Part::Reasoning {
            text: String::new(),
            signature: None,
            redacted: str_field(block, "data").map(str::to_owned),
        },
        other => {
            return Err(ProtocolError::unsupported(format!(
                "content block {:?}",
                other.unwrap_or("?")
            )));
        }
    }))
}

// ---- request encoding ----------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct EncodeOptions {
    pub default_max_tokens: u32,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            default_max_tokens: DEFAULT_MAX_TOKENS,
        }
    }
}

fn encode_image(img: &Image) -> Value {
    match img {
        Image::Base64 { media_type, data } => json!({
            "type": "image",
            "source": { "type": "base64", "media_type": media_type, "data": data },
        }),
        Image::Url(url) => json!({ "type": "image", "source": { "type": "url", "url": url } }),
    }
}

fn encode_parts(role: Role, parts: &[Part]) -> Vec<Value> {
    let mut leading = Vec::new(); // tool_result (user) or thinking (assistant) come first
    let mut rest = Vec::new();
    for p in parts {
        match p {
            Part::Text(t) if !t.is_empty() => rest.push(json!({ "type": "text", "text": t })),
            Part::Text(_) => {}
            Part::Image(img) => rest.push(encode_image(img)),
            Part::ToolCall {
                id,
                name,
                arguments,
            } if role == Role::Assistant => rest.push(json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": arguments_to_object(arguments),
            })),
            Part::ToolCall { .. } => {}
            Part::ToolResult {
                call_id,
                content,
                is_error,
            } => {
                let blocks: Vec<Value> = content
                    .iter()
                    .map(|c| match c {
                        ToolResultContent::Text(t) => json!({ "type": "text", "text": t }),
                        ToolResultContent::Image(img) => encode_image(img),
                    })
                    .collect();
                let mut b =
                    json!({ "type": "tool_result", "tool_use_id": call_id, "content": blocks });
                if *is_error {
                    b["is_error"] = json!(true);
                }
                leading.push(b);
            }
            // Thinking blocks are only valid with the signature the same provider issued.
            Part::Reasoning {
                text,
                signature: Some(sig),
                ..
            } if role == Role::Assistant => {
                leading.push(json!({ "type": "thinking", "thinking": text, "signature": sig }));
            }
            Part::Reasoning {
                redacted: Some(data),
                ..
            } if role == Role::Assistant => {
                leading.push(json!({ "type": "redacted_thinking", "data": data }));
            }
            Part::Reasoning { .. } => {}
        }
    }
    leading.extend(rest);
    leading
}

pub fn encode_request(req: &CanonicalRequest, upstream_model: &str, opts: EncodeOptions) -> Value {
    // Anthropic requires alternating roles; merge consecutive messages of the same role.
    let mut messages: Vec<(Role, Vec<Value>)> = Vec::new();
    for m in &req.messages {
        let blocks = encode_parts(m.role, &m.parts);
        if blocks.is_empty() {
            continue;
        }
        match messages.last_mut() {
            Some((role, existing)) if *role == m.role => existing.extend(blocks),
            _ => messages.push((m.role, blocks)),
        }
    }
    let messages: Vec<Value> = messages
        .into_iter()
        .map(|(role, content)| {
            json!({
                "role": if role == Role::User { "user" } else { "assistant" },
                "content": content,
            })
        })
        .collect();

    let mut body = Map::new();
    body.insert("model".into(), json!(upstream_model));
    let mut max_tokens = req.max_output_tokens.unwrap_or(opts.default_max_tokens);
    if let Some(r) = &req.reasoning {
        let budget = r.budget().max(1024);
        // budget_tokens must be below max_tokens.
        max_tokens = max_tokens.max(budget + 1024);
        body.insert(
            "thinking".into(),
            json!({ "type": "enabled", "budget_tokens": budget }),
        );
    }
    body.insert("max_tokens".into(), json!(max_tokens));
    if !req.instructions.is_empty() {
        body.insert("system".into(), json!(req.instructions.join("\n\n")));
    }
    body.insert("messages".into(), Value::Array(messages));
    if !req.tools.is_empty() {
        body.insert(
            "tools".into(),
            Value::Array(
                req.tools
                    .iter()
                    .map(|t| {
                        let mut tool = Map::new();
                        tool.insert("name".into(), json!(t.name));
                        if let Some(d) = &t.description {
                            tool.insert("description".into(), json!(d));
                        }
                        let schema = if t.parameters.is_object() {
                            t.parameters.clone()
                        } else {
                            json!({ "type": "object", "properties": {} })
                        };
                        tool.insert("input_schema".into(), schema);
                        Value::Object(tool)
                    })
                    .collect(),
            ),
        );
    }
    let mut choice = match &req.tool_choice {
        Some(ToolChoice::Auto) => Some(json!({ "type": "auto" })),
        Some(ToolChoice::Required) => Some(json!({ "type": "any" })),
        Some(ToolChoice::None) => Some(json!({ "type": "none" })),
        Some(ToolChoice::Named(n)) => Some(json!({ "type": "tool", "name": n })),
        None => None,
    };
    if req.parallel_tool_calls == Some(false) && !req.tools.is_empty() {
        let c = choice.get_or_insert_with(|| json!({ "type": "auto" }));
        c["disable_parallel_tool_use"] = json!(true);
    }
    if let Some(c) = choice {
        body.insert("tool_choice".into(), c);
    }
    // Extended thinking only allows the default sampling parameters.
    if req.reasoning.is_none() {
        if let Some(t) = req.temperature {
            body.insert("temperature".into(), json!(t));
        }
        if let Some(p) = req.top_p {
            body.insert("top_p".into(), json!(p));
        }
    }
    if !req.stop.is_empty() {
        body.insert("stop_sequences".into(), json!(req.stop));
    }
    if let Some(u) = &req.user {
        body.insert("metadata".into(), json!({ "user_id": u }));
    }
    body.insert("stream".into(), json!(true));
    Value::Object(body)
}

// ---- usage / stop ----------------------------------------------------------------------------

pub(crate) fn decode_usage(u: &Value) -> Usage {
    let cache_read = u64_field(u, "cache_read_input_tokens");
    let cache_write = u64_field(u, "cache_creation_input_tokens");
    Usage {
        input_tokens: u64_field(u, "input_tokens")
            .map(|i| i + cache_read.unwrap_or(0) + cache_write.unwrap_or(0)),
        output_tokens: u64_field(u, "output_tokens"),
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        reasoning_tokens: None,
    }
}

fn encode_usage(u: &Usage) -> Value {
    let cache_read = u.cache_read_tokens.unwrap_or(0);
    let cache_write = u.cache_write_tokens.unwrap_or(0);
    let mut v = json!({
        "input_tokens": u.input_tokens.unwrap_or(0).saturating_sub(cache_read + cache_write),
        "output_tokens": u.output_tokens.unwrap_or(0),
    });
    if u.cache_read_tokens.is_some() {
        v["cache_read_input_tokens"] = json!(cache_read);
    }
    if u.cache_write_tokens.is_some() {
        v["cache_creation_input_tokens"] = json!(cache_write);
    }
    v
}

fn decode_stop(s: &str) -> StopReason {
    match s {
        "end_turn" => StopReason::EndTurn,
        "max_tokens" => StopReason::MaxTokens,
        "tool_use" => StopReason::ToolUse,
        "stop_sequence" => StopReason::StopSequence,
        "refusal" => StopReason::ContentFilter,
        other => StopReason::Other(other.to_owned()),
    }
}

fn encode_stop(r: &StopReason) -> &'static str {
    match r {
        StopReason::EndTurn | StopReason::Other(_) => "end_turn",
        StopReason::MaxTokens => "max_tokens",
        StopReason::ToolUse => "tool_use",
        StopReason::StopSequence => "stop_sequence",
        StopReason::ContentFilter => "refusal",
    }
}

// ---- stream decoding -------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct MessagesStreamDecoder {
    done: bool,
    /// content block index → canonical tool index (tool_use blocks only)
    tool_blocks: HashMap<u64, u32>,
}

impl StreamDecoder for MessagesStreamDecoder {
    fn feed(&mut self, event: &SseEvent) -> Result<Vec<CanonicalEvent>, ProtocolError> {
        if event.data.trim().is_empty() {
            return Ok(Vec::new());
        }
        let v: Value = serde_json::from_str(&event.data)
            .map_err(|e| ProtocolError::InvalidUpstream(format!("messages event: {e}")))?;
        let kind = str_field(&v, "type")
            .or(event.event.as_deref())
            .unwrap_or("");
        let mut out = Vec::new();
        match kind {
            "message_start" => {
                let m = v.get("message").unwrap_or(&Value::Null);
                out.push(CanonicalEvent::Start {
                    id: str_field(m, "id").map(str::to_owned),
                    model: str_field(m, "model").map(str::to_owned),
                });
                if let Some(u) = m.get("usage") {
                    let mut usage = decode_usage(u);
                    // The output count at message_start is a placeholder.
                    usage.output_tokens = None;
                    out.push(CanonicalEvent::Usage(usage));
                }
            }
            "content_block_start" => {
                let block = v.get("content_block").unwrap_or(&Value::Null);
                let index = u64_field(&v, "index").unwrap_or(0);
                match str_field(block, "type") {
                    Some("tool_use") => {
                        let tool_index = self.tool_blocks.len() as u32;
                        self.tool_blocks.insert(index, tool_index);
                        out.push(CanonicalEvent::ToolCallStart {
                            index: tool_index,
                            id: str_field(block, "id")
                                .map(str::to_owned)
                                .unwrap_or_else(|| fallback_id("toolu_")),
                            name: str_field(block, "name").unwrap_or("").to_owned(),
                        });
                    }
                    Some("text") => {
                        if let Some(t) = str_field(block, "text").filter(|t| !t.is_empty()) {
                            out.push(CanonicalEvent::TextDelta(t.to_owned()));
                        }
                    }
                    Some("thinking") => {
                        if let Some(t) = str_field(block, "thinking").filter(|t| !t.is_empty()) {
                            out.push(CanonicalEvent::ReasoningDelta(t.to_owned()));
                        }
                    }
                    _ => {}
                }
            }
            "content_block_delta" => {
                let delta = v.get("delta").unwrap_or(&Value::Null);
                let index = u64_field(&v, "index").unwrap_or(0);
                match str_field(delta, "type") {
                    Some("text_delta") => {
                        if let Some(t) = str_field(delta, "text").filter(|t| !t.is_empty()) {
                            out.push(CanonicalEvent::TextDelta(t.to_owned()));
                        }
                    }
                    Some("input_json_delta") => {
                        if let (Some(&tool), Some(p)) = (
                            self.tool_blocks.get(&index),
                            str_field(delta, "partial_json").filter(|p| !p.is_empty()),
                        ) {
                            out.push(CanonicalEvent::ToolCallArgs {
                                index: tool,
                                delta: p.to_owned(),
                            });
                        }
                    }
                    Some("thinking_delta") => {
                        if let Some(t) = str_field(delta, "thinking").filter(|t| !t.is_empty()) {
                            out.push(CanonicalEvent::ReasoningDelta(t.to_owned()));
                        }
                    }
                    Some("signature_delta") => {
                        if let Some(s) = str_field(delta, "signature") {
                            out.push(CanonicalEvent::ReasoningSignature(s.to_owned()));
                        }
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(u) = v.get("usage") {
                    out.push(CanonicalEvent::Usage(decode_usage(u)));
                }
                if let Some(s) = v.get("delta").and_then(|d| str_field(d, "stop_reason")) {
                    out.push(CanonicalEvent::Finish(decode_stop(s)));
                }
            }
            "message_stop" => self.done = true,
            "error" => {
                let err = v.get("error").unwrap_or(&Value::Null);
                out.push(CanonicalEvent::Error {
                    message: str_field(err, "message")
                        .unwrap_or("upstream error")
                        .to_owned(),
                    kind: str_field(err, "type").map(str::to_owned),
                });
            }
            _ => {} // ping, content_block_stop, unknown future events
        }
        Ok(out)
    }

    fn is_done(&self) -> bool {
        self.done
    }
}

// ---- stream encoding -------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Block {
    Text,
    Thinking,
}

#[derive(Debug)]
pub struct MessagesStreamEncoder {
    id: String,
    model: String,
    started: bool,
    finished: bool,
    next_block: u32,
    open: Option<(Block, u32)>,
    /// Tool calls are emitted as whole blocks at the end: Anthropic blocks cannot
    /// interleave, while OpenAI upstreams may interleave parallel tool calls.
    tools: Vec<(String, String, String)>,
    usage: Usage,
    stop: Option<StopReason>,
}

impl MessagesStreamEncoder {
    pub fn new(model: &str) -> Self {
        Self {
            id: fallback_id("msg_"),
            model: model.to_owned(),
            started: false,
            finished: false,
            next_block: 0,
            open: None,
            tools: Vec::new(),
            usage: Usage::default(),
            stop: None,
        }
    }

    fn emit(out: &mut BytesMut, name: &str, data: Value) {
        out.extend_from_slice(&encode_json(Some(name), &data));
    }

    fn start(&mut self, out: &mut BytesMut) {
        if self.started {
            return;
        }
        self.started = true;
        Self::emit(
            out,
            "message_start",
            json!({
                "type": "message_start",
                "message": {
                    "id": self.id,
                    "type": "message",
                    "role": "assistant",
                    "model": self.model,
                    "content": [],
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": { "input_tokens": 0, "output_tokens": 0 },
                },
            }),
        );
    }

    fn close(&mut self, out: &mut BytesMut) {
        if let Some((_, index)) = self.open.take() {
            Self::emit(
                out,
                "content_block_stop",
                json!({ "type": "content_block_stop", "index": index }),
            );
        }
    }

    fn ensure_block(&mut self, out: &mut BytesMut, kind: Block) -> u32 {
        if let Some((open, index)) = self.open
            && open == kind
        {
            return index;
        }
        self.close(out);
        let index = self.next_block;
        self.next_block += 1;
        let block = match kind {
            Block::Text => json!({ "type": "text", "text": "" }),
            Block::Thinking => json!({ "type": "thinking", "thinking": "" }),
        };
        Self::emit(
            out,
            "content_block_start",
            json!({ "type": "content_block_start", "index": index, "content_block": block }),
        );
        self.open = Some((kind, index));
        index
    }

    fn delta(out: &mut BytesMut, index: u32, delta: Value) {
        Self::emit(
            out,
            "content_block_delta",
            json!({ "type": "content_block_delta", "index": index, "delta": delta }),
        );
    }
}

impl StreamEncoder for MessagesStreamEncoder {
    fn encode(&mut self, event: &CanonicalEvent) -> Bytes {
        let mut out = BytesMut::new();
        match event {
            CanonicalEvent::Start { id, .. } => {
                if !self.started
                    && let Some(id) = id
                {
                    self.id = if id.starts_with("msg_") {
                        id.clone()
                    } else {
                        format!("msg_{id}")
                    };
                }
                self.start(&mut out);
            }
            CanonicalEvent::TextDelta(t) => {
                self.start(&mut out);
                let i = self.ensure_block(&mut out, Block::Text);
                Self::delta(&mut out, i, json!({ "type": "text_delta", "text": t }));
            }
            CanonicalEvent::ReasoningDelta(t) => {
                self.start(&mut out);
                let i = self.ensure_block(&mut out, Block::Thinking);
                Self::delta(
                    &mut out,
                    i,
                    json!({ "type": "thinking_delta", "thinking": t }),
                );
            }
            CanonicalEvent::ReasoningSignature(s) => {
                if let Some((Block::Thinking, i)) = self.open {
                    Self::delta(
                        &mut out,
                        i,
                        json!({ "type": "signature_delta", "signature": s }),
                    );
                }
            }
            CanonicalEvent::ToolCallStart { index, id, name } => {
                self.start(&mut out);
                let i = *index as usize;
                if self.tools.len() <= i {
                    self.tools.resize_with(i + 1, Default::default);
                }
                self.tools[i].0.clone_from(id);
                self.tools[i].1.clone_from(name);
            }
            CanonicalEvent::ToolCallArgs { index, delta } => {
                if let Some(t) = self.tools.get_mut(*index as usize) {
                    t.2.push_str(delta);
                }
            }
            CanonicalEvent::Usage(u) => self.usage.merge(u),
            CanonicalEvent::Finish(r) => self.stop = Some(r.clone()),
            CanonicalEvent::Error { message, kind } => {
                self.start(&mut out);
                Self::emit(
                    &mut out,
                    "error",
                    json!({
                        "type": "error",
                        "error": {
                            "type": kind.as_deref().filter(|k| k.ends_with("_error")).unwrap_or("api_error"),
                            "message": message,
                        },
                    }),
                );
            }
        }
        out.freeze()
    }

    fn finish(&mut self) -> Bytes {
        if self.finished {
            return Bytes::new();
        }
        self.finished = true;
        let mut out = BytesMut::new();
        self.start(&mut out);
        self.close(&mut out);
        for (id, name, args) in std::mem::take(&mut self.tools) {
            let index = self.next_block;
            self.next_block += 1;
            Self::emit(
                &mut out,
                "content_block_start",
                json!({
                    "type": "content_block_start",
                    "index": index,
                    "content_block": { "type": "tool_use", "id": id, "name": name, "input": {} },
                }),
            );
            let json_args = if args.trim().is_empty() {
                "{}".to_owned()
            } else {
                args
            };
            Self::delta(
                &mut out,
                index,
                json!({ "type": "input_json_delta", "partial_json": json_args }),
            );
            Self::emit(
                &mut out,
                "content_block_stop",
                json!({ "type": "content_block_stop", "index": index }),
            );
            self.stop.get_or_insert(StopReason::ToolUse);
        }
        let stop = self.stop.clone().unwrap_or(StopReason::EndTurn);
        Self::emit(
            &mut out,
            "message_delta",
            json!({
                "type": "message_delta",
                "delta": { "stop_reason": encode_stop(&stop), "stop_sequence": null },
                "usage": encode_usage(&self.usage),
            }),
        );
        Self::emit(&mut out, "message_stop", json!({ "type": "message_stop" }));
        out.freeze()
    }
}

// ---- non-streaming response -------------------------------------------------------------------

pub fn encode_response(r: &CanonicalResponse, model: &str) -> Value {
    let mut content = Vec::new();
    if !r.reasoning.is_empty() {
        content.push(json!({
            "type": "thinking",
            "thinking": r.reasoning,
            "signature": r.reasoning_signature.clone().unwrap_or_default(),
        }));
    }
    if !r.text.is_empty() {
        content.push(json!({ "type": "text", "text": r.text }));
    }
    for c in &r.tool_calls {
        content.push(json!({
            "type": "tool_use",
            "id": c.id,
            "name": c.name,
            "input": arguments_to_object(&c.arguments),
        }));
    }
    let stop = r.stop_reason.clone().unwrap_or(StopReason::EndTurn);
    let id =
        r.id.clone()
            .map(|id| {
                if id.starts_with("msg_") {
                    id
                } else {
                    format!("msg_{id}")
                }
            })
            .unwrap_or_else(|| fallback_id("msg_"));
    json!({
        "id": id,
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": content,
        "stop_reason": encode_stop(&stop),
        "stop_sequence": null,
        "usage": encode_usage(&r.usage),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sse::SseParser;
    use crate::{Aggregator, Effort, chat};

    fn events(bytes: &[u8]) -> Vec<SseEvent> {
        let mut p = SseParser::new();
        let mut v = p.feed(bytes);
        v.extend(p.finish());
        v
    }

    #[test]
    fn decodes_request_with_all_block_types() {
        let body = json!({
            "model": "claude-x",
            "max_tokens": 1000,
            "system": [{"type": "text", "text": "be kind", "cache_control": {"type": "ephemeral"}}],
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "look"},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": "QQ=="}}
                ]},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "hmm", "signature": "sig"},
                    {"type": "tool_use", "id": "t1", "name": "f", "input": {"a": 1}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t1", "content": "done", "is_error": true},
                    {"type": "text", "text": "and?"}
                ]}
            ],
            "tools": [{"name": "f", "input_schema": {"type": "object"}}],
            "tool_choice": {"type": "any", "disable_parallel_tool_use": true},
            "thinking": {"type": "enabled", "budget_tokens": 5000},
            "stop_sequences": ["X"],
            "metadata": {"user_id": "u1"}
        });
        let req = decode_request(&body).expect("decode");
        assert_eq!(req.instructions, vec!["be kind"]);
        assert_eq!(req.tool_choice, Some(ToolChoice::Required));
        assert_eq!(req.parallel_tool_calls, Some(false));
        assert_eq!(req.reasoning.map(|r| r.budget()), Some(5000));
        assert_eq!(req.user.as_deref(), Some("u1"));
        // tool_result is split into its own message before the trailing text.
        assert_eq!(req.messages.len(), 4);
        assert!(matches!(
            &req.messages[1].parts[0],
            Part::Reasoning { signature: Some(s), .. } if s == "sig"
        ));
        assert!(matches!(
            &req.messages[1].parts[1],
            Part::ToolCall { arguments, .. } if arguments == "{\"a\":1}"
        ));
        assert!(matches!(
            &req.messages[2].parts[0],
            Part::ToolResult { is_error: true, .. }
        ));
        assert_eq!(req.messages[3].parts, vec![Part::Text("and?".into())]);
    }

    #[test]
    fn rejects_server_tools_and_documents() {
        let server_tool = json!({"model": "m", "max_tokens": 1, "messages": [],
            "tools": [{"type": "web_search_20250305", "name": "web_search"}]});
        assert!(matches!(
            decode_request(&server_tool),
            Err(ProtocolError::Unsupported(_))
        ));
        let doc = json!({"model": "m", "max_tokens": 1, "messages": [
            {"role": "user", "content": [{"type": "document", "source": {}}]}]});
        assert!(matches!(
            decode_request(&doc),
            Err(ProtocolError::Unsupported(_))
        ));
    }

    #[test]
    fn encoding_merges_roles_orders_blocks_and_drops_unsigned_reasoning() {
        let req = CanonicalRequest {
            model: "m".into(),
            instructions: vec!["a".into(), "b".into()],
            messages: vec![
                Message::text(Role::User, "hi"),
                Message::text(Role::User, "again"),
                Message::new(
                    Role::Assistant,
                    vec![
                        Part::Text("sure".into()),
                        Part::Reasoning {
                            text: "unsigned".into(),
                            signature: None,
                            redacted: None,
                        },
                        Part::ToolCall {
                            id: "c1".into(),
                            name: "f".into(),
                            arguments: "not json".into(),
                        },
                    ],
                ),
                Message::new(
                    Role::User,
                    vec![
                        Part::Text("after".into()),
                        Part::ToolResult {
                            call_id: "c1".into(),
                            content: vec![ToolResultContent::Text("r".into())],
                            is_error: false,
                        },
                    ],
                ),
            ],
            reasoning: Some(Reasoning {
                effort: Some(Effort::High),
                budget_tokens: None,
            }),
            max_output_tokens: Some(100),
            temperature: Some(0.2),
            ..CanonicalRequest::default()
        };
        let v = encode_request(&req, "claude-up", EncodeOptions::default());
        assert_eq!(v["system"], "a\n\nb");
        let msgs = v["messages"].as_array().expect("messages");
        assert_eq!(msgs.len(), 3, "two user messages merged");
        assert_eq!(msgs[0]["content"].as_array().map(Vec::len), Some(2));
        let assistant = msgs[1]["content"].as_array().expect("assistant");
        assert_eq!(assistant.len(), 2, "unsigned reasoning dropped");
        assert_eq!(assistant[1]["input"]["_raw_arguments"], "not json");
        assert_eq!(
            msgs[2]["content"][0]["type"], "tool_result",
            "tool_result first"
        );
        assert_eq!(v["thinking"]["budget_tokens"], 24576);
        assert!(v["max_tokens"].as_u64().expect("max") > 24576);
        assert!(
            v.get("temperature").is_none(),
            "no temperature with thinking"
        );
        assert_eq!(v["stream"], true);
    }

    #[test]
    fn default_max_tokens_is_applied() {
        let req = CanonicalRequest {
            model: "m".into(),
            messages: vec![Message::text(Role::User, "x")],
            ..CanonicalRequest::default()
        };
        assert_eq!(
            encode_request(&req, "m", EncodeOptions::default())["max_tokens"],
            DEFAULT_MAX_TOKENS
        );
    }

    const STREAM: &str = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude\",\"usage\":{\"input_tokens\":10,\"cache_read_input_tokens\":5,\"output_tokens\":1}}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"hm\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"S\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: ping\n",
        "data: {\"type\":\"ping\"}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"f\",\"input\":{}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"x\\\":\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"2}\"}}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":7}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    #[test]
    fn decodes_stream() {
        let mut d = MessagesStreamDecoder::default();
        let mut agg = Aggregator::new();
        for e in events(STREAM.as_bytes()) {
            for c in d.feed(&e).expect("feed") {
                agg.push(&c);
            }
        }
        assert!(d.is_done());
        let r = agg.finish();
        assert_eq!(r.id.as_deref(), Some("msg_1"));
        assert_eq!(r.reasoning, "hm");
        assert_eq!(r.reasoning_signature.as_deref(), Some("S"));
        assert_eq!(r.text, "Hi");
        assert_eq!(r.tool_calls[0].arguments, "{\"x\":2}");
        assert_eq!(r.stop_reason, Some(StopReason::ToolUse));
        assert_eq!(r.usage.input_tokens, Some(15), "input includes cache reads");
        assert_eq!(r.usage.cache_read_tokens, Some(5));
        assert_eq!(r.usage.output_tokens, Some(7));
    }

    #[test]
    fn encoder_output_has_valid_block_structure_and_round_trips() {
        let mut e = MessagesStreamEncoder::new("loadout/x");
        let mut out = BytesMut::new();
        for ev in [
            CanonicalEvent::Start {
                id: Some("abc".into()),
                model: None,
            },
            CanonicalEvent::ReasoningDelta("r".into()),
            CanonicalEvent::TextDelta("A".into()),
            CanonicalEvent::TextDelta("B".into()),
            // Interleaved parallel tool calls, as OpenAI upstreams may send them.
            CanonicalEvent::ToolCallStart {
                index: 0,
                id: "c0".into(),
                name: "f".into(),
            },
            CanonicalEvent::ToolCallStart {
                index: 1,
                id: "c1".into(),
                name: "g".into(),
            },
            CanonicalEvent::ToolCallArgs {
                index: 1,
                delta: "{\"b\":".into(),
            },
            CanonicalEvent::ToolCallArgs {
                index: 0,
                delta: "{}".into(),
            },
            CanonicalEvent::ToolCallArgs {
                index: 1,
                delta: "2}".into(),
            },
            CanonicalEvent::Usage(Usage {
                input_tokens: Some(9),
                output_tokens: Some(4),
                cache_read_tokens: Some(3),
                ..Usage::default()
            }),
        ] {
            out.extend_from_slice(&e.encode(&ev));
        }
        out.extend_from_slice(&e.finish());

        let evs = events(&out);
        let names: Vec<&str> = evs.iter().filter_map(|e| e.event.as_deref()).collect();
        assert_eq!(names.first(), Some(&"message_start"));
        assert_eq!(names.last(), Some(&"message_stop"));
        // Every started block is stopped, and indices are unique and increasing.
        let starts: Vec<u64> = evs
            .iter()
            .filter(|e| e.event.as_deref() == Some("content_block_start"))
            .map(|e| {
                serde_json::from_str::<Value>(&e.data).expect("json")["index"]
                    .as_u64()
                    .expect("i")
            })
            .collect();
        let stops = names.iter().filter(|n| **n == "content_block_stop").count();
        assert_eq!(starts, vec![0, 1, 2, 3]);
        assert_eq!(stops, 4);
        let delta: Value = serde_json::from_str(
            &evs.iter()
                .find(|e| e.event.as_deref() == Some("message_delta"))
                .expect("delta")
                .data,
        )
        .expect("json");
        assert_eq!(delta["delta"]["stop_reason"], "tool_use");
        assert_eq!(delta["usage"]["input_tokens"], 6, "cache reads excluded");
        assert_eq!(delta["usage"]["cache_read_input_tokens"], 3);

        let mut d = MessagesStreamDecoder::default();
        let mut agg = Aggregator::new();
        for ev in &evs {
            for c in d.feed(ev).expect("feed") {
                agg.push(&c);
            }
        }
        let r = agg.finish();
        assert_eq!(r.text, "AB");
        assert_eq!(r.reasoning, "r");
        assert_eq!(r.tool_calls.len(), 2);
        assert_eq!(r.tool_calls[1].arguments, "{\"b\":2}");
        assert_eq!(r.usage.input_tokens, Some(9));
    }

    #[test]
    fn chat_request_converts_to_messages_and_back() {
        let chat_body = json!({
            "model": "m",
            "messages": [
                {"role": "system", "content": "sys"},
                {"role": "user", "content": "q"},
                {"role": "assistant", "content": null, "tool_calls": [
                    {"id": "c1", "type": "function", "function": {"name": "f", "arguments": "{\"k\":\"v\"}"}}
                ]},
                {"role": "tool", "tool_call_id": "c1", "content": "res"}
            ],
            "tools": [{"type": "function", "function": {"name": "f", "parameters": {"type": "object"}}}],
            "max_tokens": 50
        });
        let canonical = chat::decode_request(&chat_body).expect("chat decode");
        let anthropic = encode_request(&canonical, "claude", EncodeOptions::default());
        let back = decode_request(&anthropic).expect("messages decode");
        assert_eq!(back.instructions, canonical.instructions);
        assert_eq!(back.messages, canonical.messages);
        assert_eq!(back.tools, canonical.tools);
        assert_eq!(back.max_output_tokens, Some(50));
    }
}
