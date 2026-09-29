// SPDX-License-Identifier: Apache-2.0

use crate::{CanonicalEvent, CanonicalResponse, StopReason, ToolCall};

/// Folds a canonical event stream into a complete response (for non-streaming clients).
#[derive(Debug, Default)]
pub struct Aggregator {
    response: CanonicalResponse,
    error: Option<String>,
}

impl Aggregator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, event: &CanonicalEvent) {
        let r = &mut self.response;
        match event {
            CanonicalEvent::Start { id, model } => {
                if r.id.is_none() {
                    r.id.clone_from(id);
                }
                if r.model.is_none() {
                    r.model.clone_from(model);
                }
            }
            CanonicalEvent::TextDelta(t) => r.text.push_str(t),
            CanonicalEvent::ReasoningDelta(t) => r.reasoning.push_str(t),
            CanonicalEvent::ReasoningSignature(s) => {
                r.reasoning_signature
                    .get_or_insert_with(String::new)
                    .push_str(s);
            }
            CanonicalEvent::ToolCallStart { index, id, name } => {
                let i = *index as usize;
                if r.tool_calls.len() <= i {
                    r.tool_calls.resize_with(i + 1, || ToolCall {
                        id: String::new(),
                        name: String::new(),
                        arguments: String::new(),
                    });
                }
                r.tool_calls[i].id.clone_from(id);
                r.tool_calls[i].name.clone_from(name);
            }
            CanonicalEvent::ToolCallArgs { index, delta } => {
                if let Some(call) = r.tool_calls.get_mut(*index as usize) {
                    call.arguments.push_str(delta);
                }
            }
            CanonicalEvent::Usage(u) => r.usage.merge(u),
            CanonicalEvent::Finish(reason) => r.stop_reason = Some(reason.clone()),
            CanonicalEvent::Error { message, .. } => self.error = Some(message.clone()),
        }
    }

    /// The upstream error, if the stream reported one.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn finish(mut self) -> CanonicalResponse {
        if self.response.stop_reason.is_none() {
            self.response.stop_reason = Some(if self.response.tool_calls.is_empty() {
                StopReason::EndTurn
            } else {
                StopReason::ToolUse
            });
        }
        self.response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Usage;

    #[test]
    fn aggregates_text_tools_and_usage() {
        let mut a = Aggregator::new();
        for e in [
            CanonicalEvent::Start {
                id: Some("r1".into()),
                model: Some("m".into()),
            },
            CanonicalEvent::TextDelta("Hel".into()),
            CanonicalEvent::TextDelta("lo".into()),
            CanonicalEvent::ToolCallStart {
                index: 0,
                id: "c1".into(),
                name: "f".into(),
            },
            CanonicalEvent::ToolCallArgs {
                index: 0,
                delta: "{\"a\":".into(),
            },
            CanonicalEvent::ToolCallArgs {
                index: 0,
                delta: "1}".into(),
            },
            CanonicalEvent::Usage(Usage {
                input_tokens: Some(3),
                output_tokens: Some(4),
                ..Usage::default()
            }),
        ] {
            a.push(&e);
        }
        let r = a.finish();
        assert_eq!(r.id.as_deref(), Some("r1"));
        assert_eq!(r.text, "Hello");
        assert_eq!(r.tool_calls[0].arguments, "{\"a\":1}");
        assert_eq!(r.stop_reason, Some(StopReason::ToolUse));
        assert_eq!(r.usage.output_tokens, Some(4));
    }
}
