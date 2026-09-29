// SPDX-License-Identifier: Apache-2.0

//! Protocol-neutral model. Only what all three protocols can express (plus reasoning, which
//! round-trips between compatible providers) lives here.

use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CanonicalRequest {
    pub model: String,
    /// System / developer instructions, in order.
    pub instructions: Vec<String>,
    pub messages: Vec<Message>,
    pub tools: Vec<Tool>,
    pub tool_choice: Option<ToolChoice>,
    pub max_output_tokens: Option<u32>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub stop: Vec<String>,
    pub reasoning: Option<Reasoning>,
    pub response_format: Option<ResponseFormat>,
    pub parallel_tool_calls: Option<bool>,
    pub user: Option<String>,
    pub stream: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Message {
    pub role: Role,
    pub parts: Vec<Part>,
}

impl Message {
    pub fn new(role: Role, parts: Vec<Part>) -> Self {
        Self { role, parts }
    }

    pub fn text(role: Role, text: impl Into<String>) -> Self {
        Self::new(role, vec![Part::Text(text.into())])
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Text(String),
    Image(Image),
    ToolCall {
        id: String,
        name: String,
        /// JSON-encoded arguments, as produced by the model.
        arguments: String,
    },
    ToolResult {
        call_id: String,
        content: Vec<ToolResultContent>,
        is_error: bool,
    },
    /// Model reasoning. `signature` / `redacted` are provider-issued opaque values that are
    /// only valid when sent back to the same provider.
    Reasoning {
        text: String,
        signature: Option<String>,
        redacted: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum ToolResultContent {
    Text(String),
    Image(Image),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Image {
    Url(String),
    Base64 { media_type: String, data: String },
}

impl Image {
    /// Parses `data:<media>;base64,<data>` URLs into [`Image::Base64`]; other URLs stay URLs.
    pub fn from_url(url: &str) -> Self {
        if let Some(rest) = url.strip_prefix("data:")
            && let Some((meta, data)) = rest.split_once(',')
            && let Some(media_type) = meta.strip_suffix(";base64")
        {
            return Self::Base64 {
                media_type: media_type.to_owned(),
                data: data.to_owned(),
            };
        }
        Self::Url(url.to_owned())
    }

    /// URL form usable by OpenAI protocols (data URL for inline images).
    pub fn to_url(&self) -> String {
        match self {
            Self::Url(u) => u.clone(),
            Self::Base64 { media_type, data } => format!("data:{media_type};base64,{data}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    /// JSON Schema of the arguments.
    pub parameters: Value,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    Named(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effort {
    Minimal,
    Low,
    Medium,
    High,
}

impl Effort {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "minimal" => Some(Self::Minimal),
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// Thinking-token budget used when a protocol needs a number.
    pub fn budget_tokens(self) -> u32 {
        match self {
            Self::Minimal => 1024,
            Self::Low => 2048,
            Self::Medium => 8192,
            Self::High => 24576,
        }
    }

    pub fn from_budget(tokens: u32) -> Self {
        match tokens {
            0..=1024 => Self::Minimal,
            1025..=4096 => Self::Low,
            4097..=16384 => Self::Medium,
            _ => Self::High,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reasoning {
    pub effort: Option<Effort>,
    pub budget_tokens: Option<u32>,
}

impl Reasoning {
    pub fn effort(&self) -> Effort {
        self.effort
            .or(self.budget_tokens.map(Effort::from_budget))
            .unwrap_or(Effort::Medium)
    }

    pub fn budget(&self) -> u32 {
        self.budget_tokens
            .unwrap_or_else(|| self.effort().budget_tokens())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ResponseFormat {
    Text,
    JsonObject,
    JsonSchema {
        name: String,
        schema: Value,
        strict: Option<bool>,
    },
}

// ---- streaming ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

impl Usage {
    /// Later reports override earlier ones field by field (Anthropic reports input tokens at
    /// the start and output tokens at the end).
    pub fn merge(&mut self, other: &Usage) {
        let pick = |a: &mut Option<u64>, b: Option<u64>| {
            if b.is_some() {
                *a = b;
            }
        };
        pick(&mut self.input_tokens, other.input_tokens);
        pick(&mut self.output_tokens, other.output_tokens);
        pick(&mut self.cache_read_tokens, other.cache_read_tokens);
        pick(&mut self.cache_write_tokens, other.cache_write_tokens);
        pick(&mut self.reasoning_tokens, other.reasoning_tokens);
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    ToolUse,
    StopSequence,
    ContentFilter,
    Other(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum CanonicalEvent {
    Start {
        id: Option<String>,
        model: Option<String>,
    },
    TextDelta(String),
    ReasoningDelta(String),
    ReasoningSignature(String),
    /// A tool call begins. `index` numbers tool calls 0, 1, 2… within the response.
    ToolCallStart {
        index: u32,
        id: String,
        name: String,
    },
    ToolCallArgs {
        index: u32,
        delta: String,
    },
    Usage(Usage),
    Finish(StopReason),
    Error {
        message: String,
        kind: Option<String>,
    },
}

impl CanonicalEvent {
    /// Whether this event carries model output (used for TTFT and the commit point).
    pub fn is_content(&self) -> bool {
        matches!(
            self,
            Self::TextDelta(_)
                | Self::ReasoningDelta(_)
                | Self::ToolCallStart { .. }
                | Self::ToolCallArgs { .. }
        )
    }
}

/// A complete (non-streamed) response.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CanonicalResponse {
    pub id: Option<String>,
    pub model: Option<String>,
    pub text: String,
    pub reasoning: String,
    pub reasoning_signature: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub stop_reason: Option<StopReason>,
    pub usage: Usage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_urls_become_base64_images() {
        let img = Image::from_url("data:image/png;base64,QUJD");
        assert_eq!(
            img,
            Image::Base64 {
                media_type: "image/png".into(),
                data: "QUJD".into()
            }
        );
        assert_eq!(img.to_url(), "data:image/png;base64,QUJD");
        assert_eq!(
            Image::from_url("https://x.test/a.png"),
            Image::Url("https://x.test/a.png".into())
        );
    }

    #[test]
    fn usage_merge_keeps_known_fields() {
        let mut u = Usage {
            input_tokens: Some(10),
            ..Usage::default()
        };
        u.merge(&Usage {
            output_tokens: Some(5),
            ..Usage::default()
        });
        assert_eq!(u.input_tokens, Some(10));
        assert_eq!(u.output_tokens, Some(5));
    }

    #[test]
    fn effort_budget_mapping_round_trips_by_bucket() {
        for e in [Effort::Minimal, Effort::Low, Effort::Medium, Effort::High] {
            assert_eq!(Effort::from_budget(e.budget_tokens()), e);
        }
    }
}
