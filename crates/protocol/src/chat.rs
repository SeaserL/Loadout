// SPDX-License-Identifier: Apache-2.0

//! OpenAI Chat Completions codec.
//!
//! Usage semantics: canonical `input_tokens` includes cached tokens (OpenAI convention);
//! `cache_read_tokens` is the cached subset.

use std::collections::HashMap;

use bytes::{Bytes, BytesMut};
use serde_json::{Map, Value, json};

use crate::sse::{SseEvent, encode_json};
use crate::util::{
    bool_field, f64_field, fallback_id, require_str, str_field, string_or_array, u32_field,
    u64_field, unix_now,
};
use crate::{
    CanonicalEvent, CanonicalRequest, CanonicalResponse, Effort, Image, Message, Part,
    ProtocolError, Reasoning, ResponseFormat, Role, StopReason, StreamDecoder, StreamEncoder, Tool,
    ToolChoice, ToolResultContent, Usage,
};

// ---- request decoding ----------------------------------------------------------------------

pub fn decode_request(body: &Value) -> Result<CanonicalRequest, ProtocolError> {
    let model = require_str(body, "model", "chat request")?.to_owned();
    let mut req = CanonicalRequest {
        model,
        stream: bool_field(body, "stream").unwrap_or(false),
        ..CanonicalRequest::default()
    };

    if body.get("n").and_then(Value::as_u64).is_some_and(|n| n > 1) {
        return Err(ProtocolError::unsupported("`n` > 1"));
    }
    if bool_field(body, "logprobs") == Some(true) {
        return Err(ProtocolError::unsupported("`logprobs`"));
    }
    if body.get("audio").is_some_and(|v| !v.is_null()) {
        return Err(ProtocolError::unsupported("audio output"));
    }

    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| ProtocolError::invalid("chat request: `messages` must be an array"))?;
    for msg in messages {
        decode_message(msg, &mut req)?;
    }

    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        for tool in tools {
            if str_field(tool, "type") != Some("function") {
                return Err(ProtocolError::unsupported(format!(
                    "tool type {:?}",
                    str_field(tool, "type").unwrap_or("?")
                )));
            }
            let f = tool.get("function").unwrap_or(&Value::Null);
            req.tools.push(Tool {
                name: require_str(f, "name", "tool")?.to_owned(),
                description: str_field(f, "description").map(str::to_owned),
                parameters: f
                    .get("parameters")
                    .cloned()
                    .unwrap_or(json!({"type": "object"})),
            });
        }
    }
    req.tool_choice = match body.get("tool_choice") {
        Some(Value::String(s)) => match s.as_str() {
            "auto" => Some(ToolChoice::Auto),
            "none" => Some(ToolChoice::None),
            "required" => Some(ToolChoice::Required),
            other => return Err(ProtocolError::invalid(format!("tool_choice {other:?}"))),
        },
        Some(obj @ Value::Object(_)) => obj
            .get("function")
            .and_then(|f| str_field(f, "name"))
            .map(|n| ToolChoice::Named(n.to_owned())),
        _ => None,
    };

    req.max_output_tokens =
        u32_field(body, "max_completion_tokens").or_else(|| u32_field(body, "max_tokens"));
    req.temperature = f64_field(body, "temperature");
    req.top_p = f64_field(body, "top_p");
    req.stop = string_or_array(body.get("stop"));
    req.parallel_tool_calls = bool_field(body, "parallel_tool_calls");
    req.user = str_field(body, "user").map(str::to_owned);
    req.reasoning = str_field(body, "reasoning_effort")
        .and_then(Effort::parse)
        .map(|e| Reasoning {
            effort: Some(e),
            budget_tokens: None,
        });
    req.response_format = body.get("response_format").and_then(decode_response_format);
    Ok(req)
}

fn decode_response_format(v: &Value) -> Option<ResponseFormat> {
    match str_field(v, "type")? {
        "text" => Some(ResponseFormat::Text),
        "json_object" => Some(ResponseFormat::JsonObject),
        "json_schema" => {
            let js = v.get("json_schema")?;
            Some(ResponseFormat::JsonSchema {
                name: str_field(js, "name").unwrap_or("response").to_owned(),
                schema: js.get("schema").cloned().unwrap_or(json!({})),
                strict: bool_field(js, "strict"),
            })
        }
        _ => None,
    }
}

/// Text of a `content` field that may be a string or an array of text parts.
fn content_text(content: Option<&Value>) -> Result<String, ProtocolError> {
    match content {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(s)) => Ok(s.clone()),
        Some(Value::Array(parts)) => {
            let mut out = String::new();
            for p in parts {
                match str_field(p, "type") {
                    Some("text") => out.push_str(str_field(p, "text").unwrap_or("")),
                    Some("refusal") => out.push_str(str_field(p, "refusal").unwrap_or("")),
                    other => {
                        return Err(ProtocolError::unsupported(format!(
                            "content part {:?} here",
                            other.unwrap_or("?")
                        )));
                    }
                }
            }
            Ok(out)
        }
        Some(_) => Err(ProtocolError::invalid("content must be a string or array")),
    }
}

fn decode_user_parts(content: Option<&Value>) -> Result<Vec<Part>, ProtocolError> {
    match content {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(s)) => Ok(vec![Part::Text(s.clone())]),
        Some(Value::Array(parts)) => parts
            .iter()
            .map(|p| match str_field(p, "type") {
                Some("text") => Ok(Part::Text(str_field(p, "text").unwrap_or("").to_owned())),
                Some("image_url") => {
                    let url = p
                        .get("image_url")
                        .and_then(|i| i.as_str().or_else(|| str_field(i, "url")))
                        .ok_or_else(|| ProtocolError::invalid("image_url without url"))?;
                    Ok(Part::Image(Image::from_url(url)))
                }
                other => Err(ProtocolError::unsupported(format!(
                    "user content part {:?}",
                    other.unwrap_or("?")
                ))),
            })
            .collect(),
        Some(_) => Err(ProtocolError::invalid("content must be a string or array")),
    }
}

fn decode_message(msg: &Value, req: &mut CanonicalRequest) -> Result<(), ProtocolError> {
    let role = require_str(msg, "role", "message")?;
    match role {
        "system" | "developer" => {
            let text = content_text(msg.get("content"))?;
            if !text.is_empty() {
                req.instructions.push(text);
            }
        }
        "user" => {
            let parts = decode_user_parts(msg.get("content"))?;
            req.messages.push(Message::new(Role::User, parts));
        }
        "assistant" => {
            let mut parts = Vec::new();
            let reasoning = str_field(msg, "reasoning_content")
                .or_else(|| str_field(msg, "reasoning"))
                .unwrap_or("");
            if !reasoning.is_empty() {
                parts.push(Part::Reasoning {
                    text: reasoning.to_owned(),
                    signature: None,
                    redacted: None,
                });
            }
            let text = content_text(msg.get("content"))?;
            if !text.is_empty() {
                parts.push(Part::Text(text));
            }
            if let Some(calls) = msg.get("tool_calls").and_then(Value::as_array) {
                for c in calls {
                    let f = c.get("function").unwrap_or(&Value::Null);
                    parts.push(Part::ToolCall {
                        id: require_str(c, "id", "tool call")?.to_owned(),
                        name: require_str(f, "name", "tool call")?.to_owned(),
                        arguments: str_field(f, "arguments").unwrap_or("{}").to_owned(),
                    });
                }
            }
            req.messages.push(Message::new(Role::Assistant, parts));
        }
        "tool" => {
            let part = Part::ToolResult {
                call_id: require_str(msg, "tool_call_id", "tool message")?.to_owned(),
                content: vec![ToolResultContent::Text(content_text(msg.get("content"))?)],
                is_error: false,
            };
            push_tool_result(req, part);
        }
        other => {
            return Err(ProtocolError::unsupported(format!(
                "message role {other:?}"
            )));
        }
    }
    Ok(())
}

/// Consecutive tool results share one user message.
pub(crate) fn push_tool_result(req: &mut CanonicalRequest, part: Part) {
    if let Some(last) = req.messages.last_mut()
        && last.role == Role::User
        && !last.parts.is_empty()
        && last
            .parts
            .iter()
            .all(|p| matches!(p, Part::ToolResult { .. }))
    {
        last.parts.push(part);
        return;
    }
    req.messages.push(Message::new(Role::User, vec![part]));
}

// ---- request encoding ----------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MaxTokensField {
    /// `max_tokens`: accepted by nearly every OpenAI-compatible server.
    #[default]
    MaxTokens,
    /// `max_completion_tokens`: required by OpenAI's reasoning models.
    MaxCompletionTokens,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EncodeOptions {
    pub max_tokens_field: MaxTokensField,
}

pub fn encode_request(req: &CanonicalRequest, upstream_model: &str, opts: EncodeOptions) -> Value {
    let mut messages = Vec::new();
    if !req.instructions.is_empty() {
        messages.push(json!({ "role": "system", "content": req.instructions.join("\n\n") }));
    }
    for m in &req.messages {
        match m.role {
            Role::User => encode_user(m, &mut messages),
            Role::Assistant => {
                if let Some(msg) = encode_assistant(m) {
                    messages.push(msg);
                }
            }
        }
    }

    let mut body = Map::new();
    body.insert("model".into(), json!(upstream_model));
    body.insert("messages".into(), Value::Array(messages));
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                let mut f = Map::new();
                f.insert("name".into(), json!(t.name));
                if let Some(d) = &t.description {
                    f.insert("description".into(), json!(d));
                }
                f.insert("parameters".into(), t.parameters.clone());
                json!({ "type": "function", "function": f })
            })
            .collect();
        body.insert("tools".into(), Value::Array(tools));
        if let Some(p) = req.parallel_tool_calls {
            body.insert("parallel_tool_calls".into(), json!(p));
        }
    }
    if let Some(choice) = &req.tool_choice {
        body.insert(
            "tool_choice".into(),
            match choice {
                ToolChoice::Auto => json!("auto"),
                ToolChoice::None => json!("none"),
                ToolChoice::Required => json!("required"),
                ToolChoice::Named(n) => json!({ "type": "function", "function": { "name": n } }),
            },
        );
    }
    if let Some(n) = req.max_output_tokens {
        let key = match opts.max_tokens_field {
            MaxTokensField::MaxTokens => "max_tokens",
            MaxTokensField::MaxCompletionTokens => "max_completion_tokens",
        };
        body.insert(key.into(), json!(n));
    }
    if let Some(t) = req.temperature {
        body.insert("temperature".into(), json!(t));
    }
    if let Some(p) = req.top_p {
        body.insert("top_p".into(), json!(p));
    }
    if !req.stop.is_empty() {
        body.insert("stop".into(), json!(req.stop));
    }
    if let Some(r) = &req.reasoning {
        body.insert("reasoning_effort".into(), json!(r.effort().as_str()));
    }
    if let Some(f) = &req.response_format {
        body.insert("response_format".into(), encode_response_format(f));
    }
    if let Some(u) = &req.user {
        body.insert("user".into(), json!(u));
    }
    body.insert("stream".into(), json!(true));
    body.insert("stream_options".into(), json!({ "include_usage": true }));
    Value::Object(body)
}

fn encode_response_format(f: &ResponseFormat) -> Value {
    match f {
        ResponseFormat::Text => json!({ "type": "text" }),
        ResponseFormat::JsonObject => json!({ "type": "json_object" }),
        ResponseFormat::JsonSchema {
            name,
            schema,
            strict,
        } => {
            let mut js = Map::new();
            js.insert("name".into(), json!(name));
            js.insert("schema".into(), schema.clone());
            if let Some(s) = strict {
                js.insert("strict".into(), json!(s));
            }
            json!({ "type": "json_schema", "json_schema": js })
        }
    }
}

fn tool_result_text(content: &[ToolResultContent]) -> String {
    content
        .iter()
        .map(|c| match c {
            ToolResultContent::Text(t) => t.as_str(),
            ToolResultContent::Image(_) => "[image omitted]",
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn encode_user(m: &Message, out: &mut Vec<Value>) {
    // Tool results must directly follow the assistant message that made the calls.
    for p in &m.parts {
        if let Part::ToolResult {
            call_id, content, ..
        } = p
        {
            out.push(json!({
                "role": "tool",
                "tool_call_id": call_id,
                "content": tool_result_text(content),
            }));
        }
    }
    let content: Vec<Value> = m
        .parts
        .iter()
        .filter_map(|p| match p {
            Part::Text(t) => Some(json!({ "type": "text", "text": t })),
            Part::Image(img) => {
                Some(json!({ "type": "image_url", "image_url": { "url": img.to_url() } }))
            }
            _ => None,
        })
        .collect();
    match content.as_slice() {
        [] => {}
        [single] if single["type"] == "text" => {
            out.push(json!({ "role": "user", "content": single["text"] }));
        }
        _ => out.push(json!({ "role": "user", "content": content })),
    }
}

fn encode_assistant(m: &Message) -> Option<Value> {
    let text: String = m
        .parts
        .iter()
        .filter_map(|p| match p {
            Part::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect();
    let calls: Vec<Value> = m
        .parts
        .iter()
        .filter_map(|p| match p {
            Part::ToolCall {
                id,
                name,
                arguments,
            } => Some(json!({
                "id": id,
                "type": "function",
                "function": { "name": name, "arguments": arguments },
            })),
            _ => None,
        })
        .collect();
    if text.is_empty() && calls.is_empty() {
        return None;
    }
    let mut msg = Map::new();
    msg.insert("role".into(), json!("assistant"));
    msg.insert(
        "content".into(),
        if text.is_empty() {
            Value::Null
        } else {
            json!(text)
        },
    );
    if !calls.is_empty() {
        msg.insert("tool_calls".into(), Value::Array(calls));
    }
    Some(Value::Object(msg))
}

// ---- usage / stop reason -------------------------------------------------------------------

pub(crate) fn decode_usage(u: &Value) -> Usage {
    Usage {
        input_tokens: u64_field(u, "prompt_tokens"),
        output_tokens: u64_field(u, "completion_tokens"),
        cache_read_tokens: u
            .get("prompt_tokens_details")
            .and_then(|d| u64_field(d, "cached_tokens")),
        cache_write_tokens: None,
        reasoning_tokens: u
            .get("completion_tokens_details")
            .and_then(|d| u64_field(d, "reasoning_tokens")),
    }
}

fn encode_usage(u: &Usage) -> Value {
    let input = u.input_tokens.unwrap_or(0);
    let output = u.output_tokens.unwrap_or(0);
    let mut v = json!({
        "prompt_tokens": input,
        "completion_tokens": output,
        "total_tokens": input + output,
    });
    if let Some(c) = u.cache_read_tokens {
        v["prompt_tokens_details"] = json!({ "cached_tokens": c });
    }
    if let Some(r) = u.reasoning_tokens {
        v["completion_tokens_details"] = json!({ "reasoning_tokens": r });
    }
    v
}

fn decode_finish(s: &str) -> StopReason {
    match s {
        "stop" => StopReason::EndTurn,
        "length" => StopReason::MaxTokens,
        "tool_calls" | "function_call" => StopReason::ToolUse,
        "content_filter" => StopReason::ContentFilter,
        other => StopReason::Other(other.to_owned()),
    }
}

fn encode_finish(r: &StopReason) -> &'static str {
    match r {
        StopReason::EndTurn | StopReason::StopSequence | StopReason::Other(_) => "stop",
        StopReason::MaxTokens => "length",
        StopReason::ToolUse => "tool_calls",
        StopReason::ContentFilter => "content_filter",
    }
}

// ---- stream decoding (upstream → canonical) ------------------------------------------------

#[derive(Debug, Default)]
pub struct ChatStreamDecoder {
    started: bool,
    done: bool,
    /// upstream tool-call index → canonical index
    tools: HashMap<u64, u32>,
}

impl StreamDecoder for ChatStreamDecoder {
    fn feed(&mut self, event: &SseEvent) -> Result<Vec<CanonicalEvent>, ProtocolError> {
        let data = event.data.trim();
        if data.is_empty() {
            return Ok(Vec::new());
        }
        if data == "[DONE]" {
            self.done = true;
            return Ok(Vec::new());
        }
        let chunk: Value = serde_json::from_str(data)
            .map_err(|e| ProtocolError::InvalidUpstream(format!("chat chunk: {e}")))?;
        let mut out = Vec::new();

        if let Some(err) = chunk.get("error").filter(|e| !e.is_null()) {
            out.push(CanonicalEvent::Error {
                message: str_field(err, "message")
                    .unwrap_or("upstream error")
                    .to_owned(),
                kind: str_field(err, "type")
                    .or_else(|| str_field(err, "code"))
                    .map(str::to_owned),
            });
            return Ok(out);
        }
        if !self.started {
            self.started = true;
            out.push(CanonicalEvent::Start {
                id: str_field(&chunk, "id").map(str::to_owned),
                model: str_field(&chunk, "model").map(str::to_owned),
            });
        }
        if let Some(choice) = chunk
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|c| c.first())
        {
            let delta = choice.get("delta").unwrap_or(&Value::Null);
            for key in ["reasoning_content", "reasoning"] {
                if let Some(r) = str_field(delta, key).filter(|s| !s.is_empty()) {
                    out.push(CanonicalEvent::ReasoningDelta(r.to_owned()));
                    break;
                }
            }
            if let Some(t) = str_field(delta, "content").filter(|s| !s.is_empty()) {
                out.push(CanonicalEvent::TextDelta(t.to_owned()));
            }
            if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for (pos, call) in calls.iter().enumerate() {
                    let upstream_index = u64_field(call, "index").unwrap_or(pos as u64);
                    let f = call.get("function").unwrap_or(&Value::Null);
                    let next = self.tools.len() as u32;
                    let index = match self.tools.get(&upstream_index) {
                        Some(&i) => i,
                        None => {
                            self.tools.insert(upstream_index, next);
                            out.push(CanonicalEvent::ToolCallStart {
                                index: next,
                                id: str_field(call, "id")
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| fallback_id("call_")),
                                name: str_field(f, "name").unwrap_or("").to_owned(),
                            });
                            next
                        }
                    };
                    if let Some(args) = str_field(f, "arguments").filter(|s| !s.is_empty()) {
                        out.push(CanonicalEvent::ToolCallArgs {
                            index,
                            delta: args.to_owned(),
                        });
                    }
                }
            }
            if let Some(reason) = str_field(choice, "finish_reason") {
                out.push(CanonicalEvent::Finish(decode_finish(reason)));
            }
        }
        if let Some(u) = chunk.get("usage").filter(|u| u.is_object()) {
            out.push(CanonicalEvent::Usage(decode_usage(u)));
        }
        Ok(out)
    }

    fn is_done(&self) -> bool {
        self.done
    }
}

// ---- stream encoding (canonical → client) --------------------------------------------------

#[derive(Debug)]
pub struct ChatStreamEncoder {
    id: String,
    model: String,
    created: u64,
    include_usage: bool,
    started: bool,
    finished: bool,
    stop: Option<StopReason>,
    usage: Usage,
    saw_tool_call: bool,
}

impl ChatStreamEncoder {
    pub fn new(model: &str, include_usage: bool) -> Self {
        Self {
            id: fallback_id("chatcmpl-"),
            model: model.to_owned(),
            created: unix_now(),
            include_usage,
            started: false,
            finished: false,
            stop: None,
            usage: Usage::default(),
            saw_tool_call: false,
        }
    }

    fn chunk(&self, delta: Value, finish: Option<&str>) -> Bytes {
        encode_json(
            None,
            &json!({
                "id": self.id,
                "object": "chat.completion.chunk",
                "created": self.created,
                "model": self.model,
                "choices": [{ "index": 0, "delta": delta, "finish_reason": finish }],
            }),
        )
    }

    fn start(&mut self, out: &mut BytesMut) {
        if !self.started {
            self.started = true;
            out.extend_from_slice(&self.chunk(json!({ "role": "assistant", "content": "" }), None));
        }
    }
}

impl StreamEncoder for ChatStreamEncoder {
    fn encode(&mut self, event: &CanonicalEvent) -> Bytes {
        let mut out = BytesMut::new();
        match event {
            CanonicalEvent::Start { id, .. } => {
                if !self.started
                    && let Some(id) = id
                {
                    self.id = if id.starts_with("chatcmpl") {
                        id.clone()
                    } else {
                        format!("chatcmpl-{id}")
                    };
                }
                self.start(&mut out);
            }
            CanonicalEvent::TextDelta(t) => {
                self.start(&mut out);
                out.extend_from_slice(&self.chunk(json!({ "content": t }), None));
            }
            CanonicalEvent::ReasoningDelta(t) => {
                self.start(&mut out);
                out.extend_from_slice(&self.chunk(json!({ "reasoning_content": t }), None));
            }
            CanonicalEvent::ReasoningSignature(_) => {}
            CanonicalEvent::ToolCallStart { index, id, name } => {
                self.start(&mut out);
                self.saw_tool_call = true;
                out.extend_from_slice(&self.chunk(
                    json!({ "tool_calls": [{
                        "index": index,
                        "id": id,
                        "type": "function",
                        "function": { "name": name, "arguments": "" },
                    }] }),
                    None,
                ));
            }
            CanonicalEvent::ToolCallArgs { index, delta } => {
                out.extend_from_slice(&self.chunk(
                    json!({ "tool_calls": [{ "index": index, "function": { "arguments": delta } }] }),
                    None,
                ));
            }
            CanonicalEvent::Usage(u) => self.usage.merge(u),
            CanonicalEvent::Finish(r) => self.stop = Some(r.clone()),
            CanonicalEvent::Error { message, kind } => {
                self.start(&mut out);
                out.extend_from_slice(&encode_json(
                    None,
                    &json!({ "error": {
                        "message": message,
                        "type": kind.as_deref().unwrap_or("upstream_error"),
                    } }),
                ));
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
        let stop = self.stop.clone().unwrap_or(if self.saw_tool_call {
            StopReason::ToolUse
        } else {
            StopReason::EndTurn
        });
        out.extend_from_slice(&self.chunk(json!({}), Some(encode_finish(&stop))));
        if self.include_usage {
            out.extend_from_slice(&encode_json(
                None,
                &json!({
                    "id": self.id,
                    "object": "chat.completion.chunk",
                    "created": self.created,
                    "model": self.model,
                    "choices": [],
                    "usage": encode_usage(&self.usage),
                }),
            ));
        }
        out.extend_from_slice(b"data: [DONE]\n\n");
        out.freeze()
    }
}

// ---- non-streaming response -----------------------------------------------------------------

pub fn encode_response(r: &CanonicalResponse, model: &str) -> Value {
    let mut message = Map::new();
    message.insert("role".into(), json!("assistant"));
    message.insert(
        "content".into(),
        if r.text.is_empty() && !r.tool_calls.is_empty() {
            Value::Null
        } else {
            json!(r.text)
        },
    );
    if !r.reasoning.is_empty() {
        message.insert("reasoning_content".into(), json!(r.reasoning));
    }
    if !r.tool_calls.is_empty() {
        message.insert(
            "tool_calls".into(),
            Value::Array(
                r.tool_calls
                    .iter()
                    .map(|c| {
                        json!({
                            "id": c.id,
                            "type": "function",
                            "function": { "name": c.name, "arguments": c.arguments },
                        })
                    })
                    .collect(),
            ),
        );
    }
    let stop = r.stop_reason.clone().unwrap_or(StopReason::EndTurn);
    let id =
        r.id.clone()
            .map(|id| {
                if id.starts_with("chatcmpl") {
                    id
                } else {
                    format!("chatcmpl-{id}")
                }
            })
            .unwrap_or_else(|| fallback_id("chatcmpl-"));
    json!({
        "id": id,
        "object": "chat.completion",
        "created": unix_now(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": encode_finish(&stop),
        }],
        "usage": encode_usage(&r.usage),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sse::SseParser;
    use crate::{Aggregator, ToolCall};

    fn parse_sse(bytes: &[u8]) -> Vec<SseEvent> {
        let mut p = SseParser::new();
        let mut v = p.feed(bytes);
        v.extend(p.finish());
        v
    }

    #[test]
    fn decodes_full_request() {
        let body = json!({
            "model": "gpt-x",
            "messages": [
                {"role": "system", "content": "be brief"},
                {"role": "developer", "content": [{"type": "text", "text": "use tools"}]},
                {"role": "user", "content": [
                    {"type": "text", "text": "what is this?"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAA"}}
                ]},
                {"role": "assistant", "content": null, "tool_calls": [
                    {"id": "c1", "type": "function", "function": {"name": "look", "arguments": "{\"q\":1}"}}
                ]},
                {"role": "tool", "tool_call_id": "c1", "content": "a cat"},
                {"role": "user", "content": "thanks"}
            ],
            "tools": [{"type": "function", "function": {"name": "look", "parameters": {"type": "object"}}}],
            "tool_choice": {"type": "function", "function": {"name": "look"}},
            "max_completion_tokens": 100,
            "stop": "END",
            "reasoning_effort": "high",
            "response_format": {"type": "json_schema", "json_schema": {"name": "x", "schema": {"type": "object"}, "strict": true}},
            "stream": true
        });
        let req = decode_request(&body).expect("decode");
        assert_eq!(req.instructions, vec!["be brief", "use tools"]);
        assert_eq!(req.messages.len(), 4);
        assert_eq!(
            req.messages[0].parts[1],
            Part::Image(Image::Base64 {
                media_type: "image/png".into(),
                data: "AAA".into()
            })
        );
        assert!(matches!(&req.messages[1].parts[0], Part::ToolCall { name, .. } if name == "look"));
        assert!(
            matches!(&req.messages[2].parts[0], Part::ToolResult { call_id, .. } if call_id == "c1")
        );
        assert_eq!(req.tool_choice, Some(ToolChoice::Named("look".into())));
        assert_eq!(req.max_output_tokens, Some(100));
        assert_eq!(req.stop, vec!["END"]);
        assert_eq!(req.reasoning.map(|r| r.effort()), Some(Effort::High));
        assert!(req.stream);
    }

    #[test]
    fn rejects_unsupported_features() {
        for body in [
            json!({"model": "m", "messages": [], "n": 2}),
            json!({"model": "m", "messages": [], "logprobs": true}),
            json!({"model": "m", "messages": [], "tools": [{"type": "web_search"}]}),
            json!({"model": "m", "messages": [{"role": "user", "content": [{"type": "input_audio"}]}]}),
        ] {
            assert!(matches!(
                decode_request(&body),
                Err(ProtocolError::Unsupported(_))
            ));
        }
        assert!(matches!(
            decode_request(&json!({"messages": []})),
            Err(ProtocolError::InvalidRequest(_))
        ));
    }

    #[test]
    fn request_round_trips_through_canonical() {
        let body = json!({
            "model": "m",
            "messages": [
                {"role": "system", "content": "sys"},
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": "calling", "tool_calls": [
                    {"id": "c1", "type": "function", "function": {"name": "f", "arguments": "{}"}}
                ]},
                {"role": "tool", "tool_call_id": "c1", "content": "ok"}
            ],
            "max_tokens": 5
        });
        let req = decode_request(&body).expect("decode");
        let encoded = encode_request(&req, "upstream-m", EncodeOptions::default());
        assert_eq!(encoded["model"], "upstream-m");
        assert_eq!(encoded["stream"], true);
        assert_eq!(encoded["stream_options"]["include_usage"], true);
        assert_eq!(encoded["max_tokens"], 5);
        let again = decode_request(&encoded).expect("decode again");
        assert_eq!(again.messages, req.messages);
        assert_eq!(again.instructions, req.instructions);
    }

    #[test]
    fn max_completion_tokens_option() {
        let req = CanonicalRequest {
            model: "m".into(),
            max_output_tokens: Some(7),
            ..CanonicalRequest::default()
        };
        let v = encode_request(
            &req,
            "m",
            EncodeOptions {
                max_tokens_field: MaxTokensField::MaxCompletionTokens,
            },
        );
        assert_eq!(v["max_completion_tokens"], 7);
        assert!(v.get("max_tokens").is_none());
    }

    #[test]
    fn decodes_stream_with_tools_reasoning_and_usage() {
        let stream = concat!(
            "data: {\"id\":\"x1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"\"}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"reasoning_content\":\"think\"}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hi\"}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"type\":\"function\",\"function\":{\"name\":\"f\",\"arguments\":\"\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"a\\\":1}\"}}]}}]}\n\n",
            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"prompt_tokens_details\":{\"cached_tokens\":4}}}\n\n",
            "data: [DONE]\n\n",
        );
        let mut d = ChatStreamDecoder::default();
        let mut agg = Aggregator::new();
        let mut first_content = None;
        for (i, e) in parse_sse(stream.as_bytes()).iter().enumerate() {
            for ev in d.feed(e).expect("feed") {
                if ev.is_content() && first_content.is_none() {
                    first_content = Some(i);
                }
                agg.push(&ev);
            }
        }
        assert!(d.is_done());
        assert_eq!(first_content, Some(1), "role-only chunk is not content");
        let r = agg.finish();
        assert_eq!(r.id.as_deref(), Some("x1"));
        assert_eq!(r.reasoning, "think");
        assert_eq!(r.text, "Hi");
        assert_eq!(
            r.tool_calls,
            vec![ToolCall {
                id: "c1".into(),
                name: "f".into(),
                arguments: "{\"a\":1}".into()
            }]
        );
        assert_eq!(r.stop_reason, Some(StopReason::ToolUse));
        assert_eq!(r.usage.input_tokens, Some(10));
        assert_eq!(r.usage.cache_read_tokens, Some(4));
    }

    #[test]
    fn decodes_stream_error_object() {
        let mut d = ChatStreamDecoder::default();
        let ev = parse_sse(
            b"data: {\"error\":{\"message\":\"overloaded\",\"type\":\"server_error\"}}\n\n",
        );
        let out = d.feed(&ev[0]).expect("feed");
        assert_eq!(
            out,
            vec![CanonicalEvent::Error {
                message: "overloaded".into(),
                kind: Some("server_error".into())
            }]
        );
    }

    #[test]
    fn encoder_emits_valid_chunk_sequence() {
        let mut e = ChatStreamEncoder::new("loadout/test", true);
        let mut out = Vec::new();
        for ev in [
            CanonicalEvent::Start {
                id: Some("abc".into()),
                model: None,
            },
            CanonicalEvent::TextDelta("Hel".into()),
            CanonicalEvent::ToolCallStart {
                index: 0,
                id: "c1".into(),
                name: "f".into(),
            },
            CanonicalEvent::ToolCallArgs {
                index: 0,
                delta: "{}".into(),
            },
            CanonicalEvent::Finish(StopReason::ToolUse),
            CanonicalEvent::Usage(Usage {
                input_tokens: Some(2),
                output_tokens: Some(3),
                ..Usage::default()
            }),
        ] {
            out.extend_from_slice(&e.encode(&ev));
        }
        out.extend_from_slice(&e.finish());
        assert!(e.finish().is_empty(), "finish is idempotent");

        let events = parse_sse(&out);
        assert_eq!(events.last().map(|e| e.data.as_str()), Some("[DONE]"));
        let chunks: Vec<Value> = events[..events.len() - 1]
            .iter()
            .map(|e| serde_json::from_str(&e.data).expect("json"))
            .collect();
        assert_eq!(chunks[0]["choices"][0]["delta"]["role"], "assistant");
        assert_eq!(chunks[0]["id"], "chatcmpl-abc");
        assert_eq!(chunks[0]["model"], "loadout/test");
        let finish = &chunks[chunks.len() - 2];
        assert_eq!(finish["choices"][0]["finish_reason"], "tool_calls");
        let usage = chunks.last().expect("usage chunk");
        assert_eq!(usage["choices"], json!([]));
        assert_eq!(usage["usage"]["total_tokens"], 5);

        // Decoding our own output yields the same content.
        let mut d = ChatStreamDecoder::default();
        let mut agg = Aggregator::new();
        for ev in &events {
            for c in d.feed(ev).expect("feed") {
                agg.push(&c);
            }
        }
        let r = agg.finish();
        assert_eq!(r.text, "Hel");
        assert_eq!(r.tool_calls[0].name, "f");
        assert_eq!(r.usage.output_tokens, Some(3));
    }

    #[test]
    fn usage_chunk_only_when_requested() {
        let mut e = ChatStreamEncoder::new("m", false);
        let mut out = e.encode(&CanonicalEvent::TextDelta("x".into())).to_vec();
        out.extend_from_slice(&e.finish());
        assert!(!String::from_utf8_lossy(&out).contains("\"usage\""));
    }

    #[test]
    fn response_encoding() {
        let r = CanonicalResponse {
            text: String::new(),
            tool_calls: vec![ToolCall {
                id: "c".into(),
                name: "f".into(),
                arguments: "{}".into(),
            }],
            stop_reason: Some(StopReason::ToolUse),
            usage: Usage {
                input_tokens: Some(1),
                output_tokens: Some(2),
                ..Usage::default()
            },
            ..CanonicalResponse::default()
        };
        let v = encode_response(&r, "m");
        assert_eq!(v["object"], "chat.completion");
        assert_eq!(v["choices"][0]["message"]["content"], Value::Null);
        assert_eq!(v["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(v["usage"]["total_tokens"], 3);
    }
}
