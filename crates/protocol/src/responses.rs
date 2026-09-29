// SPDX-License-Identifier: Apache-2.0

//! OpenAI Responses codec (stateless subset, ADR-006 §3).
//!
//! Conversion covers messages, images, function tools and reasoning summaries. Built-in
//! tools (web search, file search, code interpreter…) and stateful requests
//! (`previous_response_id`, `conversation`) only work in passthrough to a native Responses
//! upstream; converting them is rejected as unsupported.

use std::collections::HashMap;

use bytes::{Bytes, BytesMut};
use serde_json::{Map, Value, json};

use crate::chat::push_tool_result;
use crate::sse::{SseEvent, encode_json};
use crate::util::{
    bool_field, f64_field, fallback_id, require_str, str_field, u32_field, u64_field, unix_now,
};
use crate::{
    CanonicalEvent, CanonicalRequest, CanonicalResponse, Effort, Image, Message, Part,
    ProtocolError, Reasoning, ResponseFormat, Role, StopReason, StreamDecoder, StreamEncoder, Tool,
    ToolChoice, ToolResultContent, Usage,
};

// ---- request decoding ----------------------------------------------------------------------

pub fn decode_request(body: &Value) -> Result<CanonicalRequest, ProtocolError> {
    if body
        .get("previous_response_id")
        .is_some_and(|v| !v.is_null())
        || body.get("conversation").is_some_and(|v| !v.is_null())
    {
        return Err(ProtocolError::unsupported(
            "stateful Responses requests need a native Responses upstream",
        ));
    }
    let mut req = CanonicalRequest {
        model: require_str(body, "model", "responses request")?.to_owned(),
        stream: bool_field(body, "stream").unwrap_or(false),
        max_output_tokens: u32_field(body, "max_output_tokens"),
        temperature: f64_field(body, "temperature"),
        top_p: f64_field(body, "top_p"),
        parallel_tool_calls: bool_field(body, "parallel_tool_calls"),
        user: str_field(body, "user").map(str::to_owned),
        ..CanonicalRequest::default()
    };
    if let Some(i) = str_field(body, "instructions").filter(|s| !s.is_empty()) {
        req.instructions.push(i.to_owned());
    }

    match body.get("input") {
        Some(Value::String(s)) => req.messages.push(Message::text(Role::User, s.clone())),
        Some(Value::Array(items)) => {
            for item in items {
                decode_item(item, &mut req)?;
            }
        }
        None | Some(Value::Null) => {}
        Some(_) => return Err(ProtocolError::invalid("`input` must be a string or array")),
    }

    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        for tool in tools {
            match str_field(tool, "type") {
                Some("function") => req.tools.push(Tool {
                    name: require_str(tool, "name", "tool")?.to_owned(),
                    description: str_field(tool, "description").map(str::to_owned),
                    parameters: tool
                        .get("parameters")
                        .cloned()
                        .unwrap_or(json!({"type": "object"})),
                }),
                other => {
                    return Err(ProtocolError::unsupported(format!(
                        "built-in tool {:?}",
                        other.unwrap_or("?")
                    )));
                }
            }
        }
    }
    req.tool_choice = match body.get("tool_choice") {
        Some(Value::String(s)) => match s.as_str() {
            "auto" => Some(ToolChoice::Auto),
            "none" => Some(ToolChoice::None),
            "required" => Some(ToolChoice::Required),
            other => return Err(ProtocolError::invalid(format!("tool_choice {other:?}"))),
        },
        Some(obj @ Value::Object(_)) => match str_field(obj, "type") {
            Some("function") => Some(ToolChoice::Named(
                require_str(obj, "name", "tool_choice")?.to_owned(),
            )),
            other => {
                return Err(ProtocolError::unsupported(format!(
                    "tool_choice type {:?}",
                    other.unwrap_or("?")
                )));
            }
        },
        _ => None,
    };
    req.reasoning = body
        .get("reasoning")
        .and_then(|r| str_field(r, "effort"))
        .and_then(Effort::parse)
        .map(|e| Reasoning {
            effort: Some(e),
            budget_tokens: None,
        });
    req.response_format = body
        .get("text")
        .and_then(|t| t.get("format"))
        .and_then(|f| match str_field(f, "type")? {
            "text" => Some(ResponseFormat::Text),
            "json_object" => Some(ResponseFormat::JsonObject),
            "json_schema" => Some(ResponseFormat::JsonSchema {
                name: str_field(f, "name").unwrap_or("response").to_owned(),
                schema: f.get("schema").cloned().unwrap_or(json!({})),
                strict: bool_field(f, "strict"),
            }),
            _ => None,
        });
    Ok(req)
}

fn decode_content(content: Option<&Value>, role: Role) -> Result<Vec<Part>, ProtocolError> {
    match content {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(s)) => Ok(vec![Part::Text(s.clone())]),
        Some(Value::Array(parts)) => parts
            .iter()
            .map(|p| match str_field(p, "type") {
                Some("input_text" | "output_text" | "text") => {
                    Ok(Part::Text(str_field(p, "text").unwrap_or("").to_owned()))
                }
                Some("refusal") => Ok(Part::Text(str_field(p, "refusal").unwrap_or("").to_owned())),
                Some("input_image") if role == Role::User => {
                    let url = str_field(p, "image_url").ok_or_else(|| {
                        ProtocolError::unsupported("input_image without image_url (file ids)")
                    })?;
                    Ok(Part::Image(Image::from_url(url)))
                }
                other => Err(ProtocolError::unsupported(format!(
                    "content part {:?}",
                    other.unwrap_or("?")
                ))),
            })
            .collect(),
        Some(_) => Err(ProtocolError::invalid("content must be a string or array")),
    }
}

fn push_assistant_part(req: &mut CanonicalRequest, part: Part) {
    match req.messages.last_mut() {
        Some(last) if last.role == Role::Assistant => last.parts.push(part),
        _ => req.messages.push(Message::new(Role::Assistant, vec![part])),
    }
}

fn decode_item(item: &Value, req: &mut CanonicalRequest) -> Result<(), ProtocolError> {
    let item_type = str_field(item, "type");
    if item_type.is_none() || item_type == Some("message") {
        match require_str(item, "role", "input message")? {
            "system" | "developer" => {
                let text: String = decode_content(item.get("content"), Role::User)?
                    .into_iter()
                    .filter_map(|p| match p {
                        Part::Text(t) => Some(t),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.is_empty() {
                    req.instructions.push(text);
                }
            }
            "user" => {
                let parts = decode_content(item.get("content"), Role::User)?;
                req.messages.push(Message::new(Role::User, parts));
            }
            "assistant" => {
                for part in decode_content(item.get("content"), Role::Assistant)? {
                    push_assistant_part(req, part);
                }
            }
            other => return Err(ProtocolError::invalid(format!("role {other:?}"))),
        }
        return Ok(());
    }
    match item_type {
        Some("function_call") => push_assistant_part(
            req,
            Part::ToolCall {
                id: require_str(item, "call_id", "function_call")?.to_owned(),
                name: require_str(item, "name", "function_call")?.to_owned(),
                arguments: str_field(item, "arguments").unwrap_or("{}").to_owned(),
            },
        ),
        Some("function_call_output") => {
            let content = match item.get("output") {
                Some(Value::String(s)) => vec![ToolResultContent::Text(s.clone())],
                Some(Value::Array(parts)) => parts
                    .iter()
                    .filter_map(|p| match str_field(p, "type") {
                        Some("input_text" | "output_text") => Some(ToolResultContent::Text(
                            str_field(p, "text").unwrap_or("").to_owned(),
                        )),
                        Some("input_image") => str_field(p, "image_url")
                            .map(|u| ToolResultContent::Image(Image::from_url(u))),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            push_tool_result(
                req,
                Part::ToolResult {
                    call_id: require_str(item, "call_id", "function_call_output")?.to_owned(),
                    content,
                    is_error: false,
                },
            );
        }
        Some("reasoning") => {
            let text = item
                .get("summary")
                .and_then(Value::as_array)
                .map(|s| {
                    s.iter()
                        .filter_map(|p| str_field(p, "text"))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            push_assistant_part(
                req,
                Part::Reasoning {
                    text,
                    signature: None,
                    redacted: str_field(item, "encrypted_content").map(str::to_owned),
                },
            );
        }
        other => {
            return Err(ProtocolError::unsupported(format!(
                "input item {:?}",
                other.unwrap_or("?")
            )));
        }
    }
    Ok(())
}

// ---- request encoding ----------------------------------------------------------------------

pub fn encode_request(req: &CanonicalRequest, upstream_model: &str) -> Value {
    let mut input = Vec::new();
    for m in &req.messages {
        match m.role {
            Role::User => {
                let mut content = Vec::new();
                for p in &m.parts {
                    match p {
                        Part::Text(t) => content.push(json!({ "type": "input_text", "text": t })),
                        Part::Image(img) => content.push(json!({
                            "type": "input_image",
                            "image_url": img.to_url(),
                        })),
                        Part::ToolResult {
                            call_id, content: c, ..
                        } => input.push(json!({
                            "type": "function_call_output",
                            "call_id": call_id,
                            "output": c.iter().map(|x| match x {
                                ToolResultContent::Text(t) => json!({ "type": "input_text", "text": t }),
                                ToolResultContent::Image(img) => json!({ "type": "input_image", "image_url": img.to_url() }),
                            }).collect::<Vec<_>>(),
                        })),
                        _ => {}
                    }
                }
                if !content.is_empty() {
                    input.push(json!({ "role": "user", "content": content }));
                }
            }
            Role::Assistant => {
                for p in &m.parts {
                    match p {
                        Part::Text(t) if !t.is_empty() => {
                            input.push(json!({ "role": "assistant", "content": t }));
                        }
                        Part::ToolCall {
                            id,
                            name,
                            arguments,
                        } => input.push(json!({
                            "type": "function_call",
                            "call_id": id,
                            "name": name,
                            "arguments": arguments,
                        })),
                        _ => {}
                    }
                }
            }
        }
    }

    let mut body = Map::new();
    body.insert("model".into(), json!(upstream_model));
    body.insert("input".into(), Value::Array(input));
    if !req.instructions.is_empty() {
        body.insert("instructions".into(), json!(req.instructions.join("\n\n")));
    }
    if !req.tools.is_empty() {
        body.insert(
            "tools".into(),
            Value::Array(
                req.tools
                    .iter()
                    .map(|t| {
                        let mut tool = Map::new();
                        tool.insert("type".into(), json!("function"));
                        tool.insert("name".into(), json!(t.name));
                        if let Some(d) = &t.description {
                            tool.insert("description".into(), json!(d));
                        }
                        tool.insert("parameters".into(), t.parameters.clone());
                        Value::Object(tool)
                    })
                    .collect(),
            ),
        );
        if let Some(p) = req.parallel_tool_calls {
            body.insert("parallel_tool_calls".into(), json!(p));
        }
    }
    if let Some(c) = &req.tool_choice {
        body.insert(
            "tool_choice".into(),
            match c {
                ToolChoice::Auto => json!("auto"),
                ToolChoice::None => json!("none"),
                ToolChoice::Required => json!("required"),
                ToolChoice::Named(n) => json!({ "type": "function", "name": n }),
            },
        );
    }
    if let Some(n) = req.max_output_tokens {
        body.insert("max_output_tokens".into(), json!(n));
    }
    if let Some(t) = req.temperature {
        body.insert("temperature".into(), json!(t));
    }
    if let Some(p) = req.top_p {
        body.insert("top_p".into(), json!(p));
    }
    if let Some(r) = &req.reasoning {
        body.insert(
            "reasoning".into(),
            json!({ "effort": r.effort().as_str(), "summary": "auto" }),
        );
    }
    if let Some(f) = &req.response_format {
        body.insert(
            "text".into(),
            json!({ "format": match f {
                ResponseFormat::Text => json!({ "type": "text" }),
                ResponseFormat::JsonObject => json!({ "type": "json_object" }),
                ResponseFormat::JsonSchema { name, schema, strict } => {
                    let mut o = json!({ "type": "json_schema", "name": name, "schema": schema });
                    if let Some(s) = strict {
                        o["strict"] = json!(s);
                    }
                    o
                }
            } }),
        );
    }
    if let Some(u) = &req.user {
        body.insert("user".into(), json!(u));
    }
    body.insert("stream".into(), json!(true));
    body.insert("store".into(), json!(false));
    Value::Object(body)
}

// ---- usage -----------------------------------------------------------------------------------

pub(crate) fn decode_usage(u: &Value) -> Usage {
    Usage {
        input_tokens: u64_field(u, "input_tokens"),
        output_tokens: u64_field(u, "output_tokens"),
        cache_read_tokens: u
            .get("input_tokens_details")
            .and_then(|d| u64_field(d, "cached_tokens")),
        cache_write_tokens: None,
        reasoning_tokens: u
            .get("output_tokens_details")
            .and_then(|d| u64_field(d, "reasoning_tokens")),
    }
}

fn encode_usage(u: &Usage) -> Value {
    let input = u.input_tokens.unwrap_or(0);
    let output = u.output_tokens.unwrap_or(0);
    json!({
        "input_tokens": input,
        "input_tokens_details": { "cached_tokens": u.cache_read_tokens.unwrap_or(0) },
        "output_tokens": output,
        "output_tokens_details": { "reasoning_tokens": u.reasoning_tokens.unwrap_or(0) },
        "total_tokens": input + output,
    })
}

// ---- stream decoding -------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct ResponsesStreamDecoder {
    done: bool,
    /// function_call item id → (canonical tool index, whether argument deltas were seen)
    calls: HashMap<String, (u32, bool)>,
}

impl ResponsesStreamDecoder {
    fn call_index(&self, v: &Value) -> Option<u32> {
        str_field(v, "item_id")
            .and_then(|id| self.calls.get(id))
            .map(|(i, _)| *i)
    }
}

impl StreamDecoder for ResponsesStreamDecoder {
    fn feed(&mut self, event: &SseEvent) -> Result<Vec<CanonicalEvent>, ProtocolError> {
        let data = event.data.trim();
        if data.is_empty() || data == "[DONE]" {
            return Ok(Vec::new());
        }
        let v: Value = serde_json::from_str(data)
            .map_err(|e| ProtocolError::InvalidUpstream(format!("responses event: {e}")))?;
        let kind = str_field(&v, "type")
            .or(event.event.as_deref())
            .unwrap_or("");
        let mut out = Vec::new();
        match kind {
            "response.created" => {
                let r = v.get("response").unwrap_or(&Value::Null);
                out.push(CanonicalEvent::Start {
                    id: str_field(r, "id").map(str::to_owned),
                    model: str_field(r, "model").map(str::to_owned),
                });
            }
            "response.output_text.delta" | "response.refusal.delta" => {
                if let Some(d) = str_field(&v, "delta").filter(|d| !d.is_empty()) {
                    out.push(CanonicalEvent::TextDelta(d.to_owned()));
                }
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                if let Some(d) = str_field(&v, "delta").filter(|d| !d.is_empty()) {
                    out.push(CanonicalEvent::ReasoningDelta(d.to_owned()));
                }
            }
            "response.output_item.added" => {
                let item = v.get("item").unwrap_or(&Value::Null);
                if str_field(item, "type") == Some("function_call") {
                    let item_id = str_field(item, "id")
                        .map(str::to_owned)
                        .unwrap_or_else(|| fallback_id("fc_"));
                    let index = self.calls.len() as u32;
                    self.calls.insert(item_id, (index, false));
                    out.push(CanonicalEvent::ToolCallStart {
                        index,
                        id: str_field(item, "call_id")
                            .map(str::to_owned)
                            .unwrap_or_else(|| fallback_id("call_")),
                        name: str_field(item, "name").unwrap_or("").to_owned(),
                    });
                    if let Some(args) = str_field(item, "arguments").filter(|a| !a.is_empty())
                        && let Some(entry) =
                            str_field(item, "id").and_then(|id| self.calls.get_mut(id))
                    {
                        entry.1 = true;
                        out.push(CanonicalEvent::ToolCallArgs {
                            index,
                            delta: args.to_owned(),
                        });
                    }
                }
            }
            "response.function_call_arguments.delta" => {
                if let (Some(index), Some(d)) = (
                    self.call_index(&v),
                    str_field(&v, "delta").filter(|d| !d.is_empty()),
                ) {
                    if let Some(entry) =
                        str_field(&v, "item_id").and_then(|id| self.calls.get_mut(id))
                    {
                        entry.1 = true;
                    }
                    out.push(CanonicalEvent::ToolCallArgs {
                        index,
                        delta: d.to_owned(),
                    });
                }
            }
            "response.output_item.done" => {
                // Some upstreams only send complete arguments here.
                let item = v.get("item").unwrap_or(&Value::Null);
                if str_field(item, "type") == Some("function_call")
                    && let Some(id) = str_field(item, "id")
                    && let Some((index, seen)) = self.calls.get(id).copied()
                    && !seen
                    && let Some(args) = str_field(item, "arguments").filter(|a| !a.is_empty())
                {
                    out.push(CanonicalEvent::ToolCallArgs {
                        index,
                        delta: args.to_owned(),
                    });
                }
            }
            "response.completed" | "response.incomplete" => {
                self.done = true;
                let r = v.get("response").unwrap_or(&Value::Null);
                if let Some(u) = r.get("usage").filter(|u| u.is_object()) {
                    out.push(CanonicalEvent::Usage(decode_usage(u)));
                }
                let reason = if kind == "response.incomplete" {
                    match r
                        .get("incomplete_details")
                        .and_then(|d| str_field(d, "reason"))
                    {
                        Some("content_filter") => StopReason::ContentFilter,
                        _ => StopReason::MaxTokens,
                    }
                } else if self.calls.is_empty() {
                    StopReason::EndTurn
                } else {
                    StopReason::ToolUse
                };
                out.push(CanonicalEvent::Finish(reason));
            }
            "response.failed" => {
                self.done = true;
                let err = v
                    .get("response")
                    .and_then(|r| r.get("error"))
                    .unwrap_or(&Value::Null);
                out.push(CanonicalEvent::Error {
                    message: str_field(err, "message")
                        .unwrap_or("response failed")
                        .to_owned(),
                    kind: str_field(err, "code").map(str::to_owned),
                });
            }
            "error" => {
                out.push(CanonicalEvent::Error {
                    message: str_field(&v, "message")
                        .or_else(|| v.get("error").and_then(|e| str_field(e, "message")))
                        .unwrap_or("upstream error")
                        .to_owned(),
                    kind: str_field(&v, "code").map(str::to_owned),
                });
            }
            _ => {}
        }
        Ok(out)
    }

    fn is_done(&self) -> bool {
        self.done
    }
}

// ---- stream encoding -------------------------------------------------------------------------

#[derive(Debug)]
struct OpenItem {
    id: String,
    output_index: u32,
    text: String,
}

#[derive(Debug)]
pub struct ResponsesStreamEncoder {
    id: String,
    model: String,
    created_at: u64,
    seq: u64,
    started: bool,
    finished: bool,
    next_output: u32,
    reasoning: Option<OpenItem>,
    message: Option<OpenItem>,
    /// Completed output items, for `response.completed`.
    output: Vec<Value>,
    tools: Vec<(String, String, String)>,
    usage: Usage,
    stop: Option<StopReason>,
    error: Option<String>,
}

impl ResponsesStreamEncoder {
    pub fn new(model: &str) -> Self {
        Self {
            id: fallback_id("resp_"),
            model: model.to_owned(),
            created_at: unix_now(),
            seq: 0,
            started: false,
            finished: false,
            next_output: 0,
            reasoning: None,
            message: None,
            output: Vec::new(),
            tools: Vec::new(),
            usage: Usage::default(),
            stop: None,
            error: None,
        }
    }

    fn emit(&mut self, out: &mut BytesMut, kind: &str, mut data: Value) {
        data["type"] = json!(kind);
        data["sequence_number"] = json!(self.seq);
        self.seq += 1;
        out.extend_from_slice(&encode_json(Some(kind), &data));
    }

    fn response(&self, status: &str, with_output: bool) -> Value {
        json!({
            "id": self.id,
            "object": "response",
            "created_at": self.created_at,
            "status": status,
            "model": self.model,
            "output": if with_output { Value::Array(self.output.clone()) } else { json!([]) },
            "usage": if with_output { encode_usage(&self.usage) } else { Value::Null },
            "error": null,
            "incomplete_details": null,
        })
    }

    fn start(&mut self, out: &mut BytesMut) {
        if self.started {
            return;
        }
        self.started = true;
        let r = self.response("in_progress", false);
        self.emit(out, "response.created", json!({ "response": r }));
        let r = self.response("in_progress", false);
        self.emit(out, "response.in_progress", json!({ "response": r }));
    }

    fn open_item(&mut self, out: &mut BytesMut, prefix: &str, item: Value) -> OpenItem {
        let open = OpenItem {
            id: fallback_id(prefix),
            output_index: self.next_output,
            text: String::new(),
        };
        self.next_output += 1;
        let mut item = item;
        item["id"] = json!(open.id);
        self.emit(
            out,
            "response.output_item.added",
            json!({ "output_index": open.output_index, "item": item }),
        );
        open
    }

    fn close_reasoning(&mut self, out: &mut BytesMut) {
        let Some(r) = self.reasoning.take() else {
            return;
        };
        let base = json!({ "item_id": r.id, "output_index": r.output_index, "summary_index": 0 });
        let mut done = base.clone();
        done["text"] = json!(r.text);
        self.emit(out, "response.reasoning_summary_text.done", done);
        let mut part_done = base;
        part_done["part"] = json!({ "type": "summary_text", "text": r.text });
        self.emit(out, "response.reasoning_summary_part.done", part_done);
        let item = json!({
            "type": "reasoning",
            "id": r.id,
            "summary": [{ "type": "summary_text", "text": r.text }],
        });
        self.emit(
            out,
            "response.output_item.done",
            json!({ "output_index": r.output_index, "item": item }),
        );
        self.output.push(item);
    }

    fn close_message(&mut self, out: &mut BytesMut) {
        let Some(m) = self.message.take() else { return };
        let part = json!({ "type": "output_text", "text": m.text, "annotations": [] });
        self.emit(
            out,
            "response.output_text.done",
            json!({ "item_id": m.id, "output_index": m.output_index, "content_index": 0, "text": m.text }),
        );
        self.emit(
            out,
            "response.content_part.done",
            json!({ "item_id": m.id, "output_index": m.output_index, "content_index": 0, "part": part }),
        );
        let item = json!({
            "type": "message",
            "id": m.id,
            "status": "completed",
            "role": "assistant",
            "content": [part],
        });
        self.emit(
            out,
            "response.output_item.done",
            json!({ "output_index": m.output_index, "item": item }),
        );
        self.output.push(item);
    }
}

impl StreamEncoder for ResponsesStreamEncoder {
    fn encode(&mut self, event: &CanonicalEvent) -> Bytes {
        let mut out = BytesMut::new();
        match event {
            CanonicalEvent::Start { id, .. } => {
                if !self.started
                    && let Some(id) = id
                {
                    self.id = if id.starts_with("resp_") {
                        id.clone()
                    } else {
                        format!("resp_{id}")
                    };
                }
                self.start(&mut out);
            }
            CanonicalEvent::ReasoningDelta(t) => {
                self.start(&mut out);
                self.close_message(&mut out);
                if self.reasoning.is_none() {
                    let item = self.open_item(
                        &mut out,
                        "rs_",
                        json!({ "type": "reasoning", "summary": [] }),
                    );
                    self.emit(
                        &mut out,
                        "response.reasoning_summary_part.added",
                        json!({
                            "item_id": item.id,
                            "output_index": item.output_index,
                            "summary_index": 0,
                            "part": { "type": "summary_text", "text": "" },
                        }),
                    );
                    self.reasoning = Some(item);
                }
                let (id, idx) = {
                    let r = self.reasoning.as_mut().expect("opened above");
                    r.text.push_str(t);
                    (r.id.clone(), r.output_index)
                };
                self.emit(
                    &mut out,
                    "response.reasoning_summary_text.delta",
                    json!({ "item_id": id, "output_index": idx, "summary_index": 0, "delta": t }),
                );
            }
            CanonicalEvent::TextDelta(t) => {
                self.start(&mut out);
                self.close_reasoning(&mut out);
                if self.message.is_none() {
                    let item = self.open_item(
                        &mut out,
                        "msg_",
                        json!({ "type": "message", "status": "in_progress", "role": "assistant", "content": [] }),
                    );
                    self.emit(
                        &mut out,
                        "response.content_part.added",
                        json!({
                            "item_id": item.id,
                            "output_index": item.output_index,
                            "content_index": 0,
                            "part": { "type": "output_text", "text": "", "annotations": [] },
                        }),
                    );
                    self.message = Some(item);
                }
                let (id, idx) = {
                    let m = self.message.as_mut().expect("opened above");
                    m.text.push_str(t);
                    (m.id.clone(), m.output_index)
                };
                self.emit(
                    &mut out,
                    "response.output_text.delta",
                    json!({ "item_id": id, "output_index": idx, "content_index": 0, "delta": t }),
                );
            }
            CanonicalEvent::ReasoningSignature(_) => {}
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
                self.error = Some(message.clone());
                self.emit(
                    &mut out,
                    "error",
                    json!({
                        "code": kind.as_deref().unwrap_or("upstream_error"),
                        "message": message,
                        "param": null,
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
        self.close_reasoning(&mut out);
        self.close_message(&mut out);
        for (call_id, name, args) in std::mem::take(&mut self.tools) {
            let open = self.open_item(
                &mut out,
                "fc_",
                json!({
                    "type": "function_call",
                    "status": "in_progress",
                    "call_id": call_id,
                    "name": name,
                    "arguments": "",
                }),
            );
            self.emit(
                &mut out,
                "response.function_call_arguments.delta",
                json!({ "item_id": open.id, "output_index": open.output_index, "delta": args }),
            );
            self.emit(
                &mut out,
                "response.function_call_arguments.done",
                json!({ "item_id": open.id, "output_index": open.output_index, "arguments": args }),
            );
            let item = json!({
                "type": "function_call",
                "id": open.id,
                "status": "completed",
                "call_id": call_id,
                "name": name,
                "arguments": args,
            });
            self.emit(
                &mut out,
                "response.output_item.done",
                json!({ "output_index": open.output_index, "item": item }),
            );
            self.output.push(item);
        }
        if let Some(message) = self.error.clone() {
            let mut r = self.response("failed", true);
            r["error"] = json!({ "code": "server_error", "message": message });
            self.emit(&mut out, "response.failed", json!({ "response": r }));
            return out.freeze();
        }
        let (kind, status, details) = match self.stop {
            Some(StopReason::MaxTokens) => (
                "response.incomplete",
                "incomplete",
                json!({ "reason": "max_output_tokens" }),
            ),
            Some(StopReason::ContentFilter) => (
                "response.incomplete",
                "incomplete",
                json!({ "reason": "content_filter" }),
            ),
            _ => ("response.completed", "completed", Value::Null),
        };
        let mut r = self.response(status, true);
        r["incomplete_details"] = details;
        self.emit(&mut out, kind, json!({ "response": r }));
        out.freeze()
    }
}

// ---- non-streaming response -------------------------------------------------------------------

pub fn encode_response(r: &CanonicalResponse, model: &str) -> Value {
    let mut output = Vec::new();
    if !r.reasoning.is_empty() {
        output.push(json!({
            "type": "reasoning",
            "id": fallback_id("rs_"),
            "summary": [{ "type": "summary_text", "text": r.reasoning }],
        }));
    }
    if !r.text.is_empty() {
        output.push(json!({
            "type": "message",
            "id": fallback_id("msg_"),
            "status": "completed",
            "role": "assistant",
            "content": [{ "type": "output_text", "text": r.text, "annotations": [] }],
        }));
    }
    for c in &r.tool_calls {
        output.push(json!({
            "type": "function_call",
            "id": fallback_id("fc_"),
            "status": "completed",
            "call_id": c.id,
            "name": c.name,
            "arguments": c.arguments,
        }));
    }
    let (status, details) = match r.stop_reason {
        Some(StopReason::MaxTokens) => ("incomplete", json!({ "reason": "max_output_tokens" })),
        Some(StopReason::ContentFilter) => ("incomplete", json!({ "reason": "content_filter" })),
        _ => ("completed", Value::Null),
    };
    let id =
        r.id.clone()
            .map(|id| {
                if id.starts_with("resp_") {
                    id
                } else {
                    format!("resp_{id}")
                }
            })
            .unwrap_or_else(|| fallback_id("resp_"));
    json!({
        "id": id,
        "object": "response",
        "created_at": unix_now(),
        "status": status,
        "incomplete_details": details,
        "model": model,
        "output": output,
        "usage": encode_usage(&r.usage),
        "error": null,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sse::SseParser;
    use crate::{Aggregator, chat, messages};

    fn events(bytes: &[u8]) -> Vec<SseEvent> {
        let mut p = SseParser::new();
        let mut v = p.feed(bytes);
        v.extend(p.finish());
        v
    }

    #[test]
    fn decodes_items() {
        let body = json!({
            "model": "gpt",
            "instructions": "top",
            "input": [
                {"role": "developer", "content": "dev"},
                {"role": "user", "content": [
                    {"type": "input_text", "text": "see"},
                    {"type": "input_image", "image_url": "https://x.test/i.png"}
                ]},
                {"type": "reasoning", "id": "rs_1", "summary": [{"type": "summary_text", "text": "plan"}], "encrypted_content": "ENC"},
                {"type": "function_call", "call_id": "c1", "name": "f", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "c1", "output": "42"},
                {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "It is 42"}]}
            ],
            "tools": [{"type": "function", "name": "f", "parameters": {"type": "object"}}],
            "tool_choice": {"type": "function", "name": "f"},
            "reasoning": {"effort": "low"},
            "text": {"format": {"type": "json_object"}},
            "max_output_tokens": 64,
            "stream": true
        });
        let req = decode_request(&body).expect("decode");
        assert_eq!(req.instructions, vec!["top", "dev"]);
        assert_eq!(req.messages.len(), 4);
        assert_eq!(req.messages[0].parts.len(), 2);
        assert!(matches!(
            &req.messages[1].parts[0],
            Part::Reasoning { redacted: Some(e), .. } if e == "ENC"
        ));
        assert!(matches!(&req.messages[1].parts[1], Part::ToolCall { .. }));
        assert!(matches!(&req.messages[2].parts[0], Part::ToolResult { .. }));
        assert_eq!(req.tool_choice, Some(ToolChoice::Named("f".into())));
        assert_eq!(req.response_format, Some(ResponseFormat::JsonObject));
        assert_eq!(req.max_output_tokens, Some(64));
    }

    #[test]
    fn string_input_and_stateful_rejection() {
        let req = decode_request(&json!({"model": "m", "input": "hello"})).expect("decode");
        assert_eq!(req.messages, vec![Message::text(Role::User, "hello")]);
        for body in [
            json!({"model": "m", "input": "x", "previous_response_id": "resp_1"}),
            json!({"model": "m", "input": "x", "conversation": "conv_1"}),
            json!({"model": "m", "input": "x", "tools": [{"type": "web_search"}]}),
            json!({"model": "m", "input": [{"type": "item_reference", "id": "x"}]}),
        ] {
            assert!(
                matches!(decode_request(&body), Err(ProtocolError::Unsupported(_))),
                "{body}"
            );
        }
    }

    #[test]
    fn encodes_stateless_request() {
        let chat_body = json!({
            "model": "m",
            "messages": [
                {"role": "system", "content": "s"},
                {"role": "user", "content": "q"},
                {"role": "assistant", "content": null, "tool_calls": [{"id": "c", "type": "function", "function": {"name": "f", "arguments": "{}"}}]},
                {"role": "tool", "tool_call_id": "c", "content": "r"}
            ],
            "reasoning_effort": "medium"
        });
        let canonical = chat::decode_request(&chat_body).expect("decode");
        let v = encode_request(&canonical, "gpt-up");
        assert_eq!(v["store"], false);
        assert_eq!(v["stream"], true);
        assert_eq!(v["instructions"], "s");
        assert_eq!(v["reasoning"]["effort"], "medium");
        let input = v["input"].as_array().expect("input");
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[1]["type"], "function_call");
        assert_eq!(input[2]["type"], "function_call_output");
        let back = decode_request(&v).expect("decode back");
        assert_eq!(back.messages.len(), canonical.messages.len());
    }

    const STREAM: &str = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_9\",\"model\":\"gpt\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"reasoning\",\"id\":\"rs\"}}\n\n",
        "event: response.reasoning_summary_text.delta\n",
        "data: {\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"rs\",\"delta\":\"why\"}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"m\",\"delta\":\"Yo\"}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":2,\"item\":{\"type\":\"function_call\",\"id\":\"fc1\",\"call_id\":\"call1\",\"name\":\"f\",\"arguments\":\"\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc1\",\"delta\":\"{}\"}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":3,\"item\":{\"type\":\"function_call\",\"id\":\"fc2\",\"call_id\":\"call2\",\"name\":\"g\"}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":3,\"item\":{\"type\":\"function_call\",\"id\":\"fc2\",\"call_id\":\"call2\",\"name\":\"g\",\"arguments\":\"{\\\"z\\\":1}\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_9\",\"status\":\"completed\",\"usage\":{\"input_tokens\":8,\"output_tokens\":3,\"input_tokens_details\":{\"cached_tokens\":2},\"output_tokens_details\":{\"reasoning_tokens\":1}}}}\n\n",
    );

    #[test]
    fn decodes_stream_including_args_only_in_done() {
        let mut d = ResponsesStreamDecoder::default();
        let mut agg = Aggregator::new();
        for e in events(STREAM.as_bytes()) {
            for c in d.feed(&e).expect("feed") {
                agg.push(&c);
            }
        }
        assert!(d.is_done());
        let r = agg.finish();
        assert_eq!(r.id.as_deref(), Some("resp_9"));
        assert_eq!(r.reasoning, "why");
        assert_eq!(r.text, "Yo");
        assert_eq!(r.tool_calls.len(), 2);
        assert_eq!(r.tool_calls[0].id, "call1");
        assert_eq!(r.tool_calls[1].arguments, "{\"z\":1}");
        assert_eq!(r.stop_reason, Some(StopReason::ToolUse));
        assert_eq!(r.usage.cache_read_tokens, Some(2));
        assert_eq!(r.usage.reasoning_tokens, Some(1));
    }

    #[test]
    fn encoder_sequence_is_well_formed_and_round_trips() {
        let mut e = ResponsesStreamEncoder::new("loadout/r");
        let mut out = BytesMut::new();
        for ev in [
            CanonicalEvent::Start {
                id: Some("x".into()),
                model: None,
            },
            CanonicalEvent::ReasoningDelta("a".into()),
            CanonicalEvent::TextDelta("b".into()),
            CanonicalEvent::TextDelta("c".into()),
            CanonicalEvent::ToolCallStart {
                index: 0,
                id: "call_1".into(),
                name: "f".into(),
            },
            CanonicalEvent::ToolCallArgs {
                index: 0,
                delta: "{}".into(),
            },
            CanonicalEvent::Usage(Usage {
                input_tokens: Some(5),
                output_tokens: Some(6),
                ..Usage::default()
            }),
            CanonicalEvent::Finish(StopReason::ToolUse),
        ] {
            out.extend_from_slice(&e.encode(&ev));
        }
        out.extend_from_slice(&e.finish());

        let evs = events(&out);
        let parsed: Vec<Value> = evs
            .iter()
            .map(|e| serde_json::from_str(&e.data).expect("json"))
            .collect();
        for (i, p) in parsed.iter().enumerate() {
            assert_eq!(
                p["sequence_number"], i as u64,
                "sequence numbers are contiguous"
            );
            assert_eq!(
                Some(p["type"].as_str().expect("type")),
                evs[i].event.as_deref()
            );
        }
        assert_eq!(parsed[0]["type"], "response.created");
        assert_eq!(parsed[0]["response"]["id"], "resp_x");
        let last = parsed.last().expect("last");
        assert_eq!(last["type"], "response.completed");
        let output = last["response"]["output"].as_array().expect("output");
        let types: Vec<&str> = output
            .iter()
            .map(|o| o["type"].as_str().expect("t"))
            .collect();
        assert_eq!(types, vec!["reasoning", "message", "function_call"]);
        let indices: Vec<u64> = parsed
            .iter()
            .filter(|p| p["type"] == "response.output_item.added")
            .map(|p| p["output_index"].as_u64().expect("i"))
            .collect();
        assert_eq!(indices, vec![0, 1, 2]);
        assert_eq!(last["response"]["usage"]["total_tokens"], 11);

        let mut d = ResponsesStreamDecoder::default();
        let mut agg = Aggregator::new();
        for ev in &evs {
            for c in d.feed(ev).expect("feed") {
                agg.push(&c);
            }
        }
        let r = agg.finish();
        assert_eq!(r.text, "bc");
        assert_eq!(r.reasoning, "a");
        assert_eq!(r.tool_calls[0].id, "call_1");
        assert_eq!(r.usage.output_tokens, Some(6));
    }

    #[test]
    fn max_tokens_maps_to_incomplete() {
        let mut e = ResponsesStreamEncoder::new("m");
        let mut out = e.encode(&CanonicalEvent::TextDelta("x".into())).to_vec();
        out.extend_from_slice(&e.encode(&CanonicalEvent::Finish(StopReason::MaxTokens)));
        out.extend_from_slice(&e.finish());
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains("event: response.incomplete"));
        assert!(text.contains("max_output_tokens"));
    }

    #[test]
    fn anthropic_stream_converts_to_responses_client() {
        let upstream = concat!(
            "event: message_start\n",
            "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_a\",\"model\":\"c\",\"usage\":{\"input_tokens\":4,\"output_tokens\":1}}}\n\n",
            "event: content_block_delta\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hey\"}}\n\n",
            "event: message_delta\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":2}}\n\n",
            "event: message_stop\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let mut d = messages::MessagesStreamDecoder::default();
        let mut e = ResponsesStreamEncoder::new("loadout/x");
        let mut out = BytesMut::new();
        for ev in events(upstream.as_bytes()) {
            for c in d.feed(&ev).expect("feed") {
                out.extend_from_slice(&e.encode(&c));
            }
        }
        out.extend_from_slice(&e.finish());
        let text = String::from_utf8(out.to_vec()).expect("utf8");
        assert!(text.contains("\"delta\":\"hey\""));
        assert!(text.contains("\"total_tokens\":6"));
        assert!(text.contains("event: response.completed"));
    }
}
