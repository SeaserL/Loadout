// SPDX-License-Identifier: Apache-2.0

//! What to type into the page for a request (ADR-014 §5, ExecPlan 017 §5.6).

use loadout_protocol::{CanonicalRequest, Message, Part, Role};

/// How API requests map onto web conversations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConversationMode {
    /// Requests continue one web conversation; only the newest user message is typed.
    #[default]
    Persistent,
    /// Every request opens a temporary conversation holding the whole history.
    Stateless,
}

impl ConversationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Persistent => "persistent",
            Self::Stateless => "stateless",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "persistent" => Some(Self::Persistent),
            "stateless" => Some(Self::Stateless),
            _ => None,
        }
    }
}

const SYSTEM_START: &str = "[System instructions from the API caller]";
const SYSTEM_END: &str = "[End of system instructions]";

/// The text of a message; images and tool traffic cannot be typed into a chat page.
fn message_text(m: &Message) -> Result<String, String> {
    let mut out: Vec<&str> = Vec::new();
    for p in &m.parts {
        match p {
            Part::Text(t) => out.push(t),
            Part::Reasoning { .. } => {}
            Part::Image(_) => {
                return Err("web sessions take text only; the request has an image".into());
            }
            Part::ToolCall { .. } | Part::ToolResult { .. } => {
                return Err("web sessions do not support tool calls".into());
            }
        }
    }
    Ok(out.join("\n"))
}

fn system_block(req: &CanonicalRequest) -> Option<String> {
    let instructions: Vec<&str> = req
        .instructions
        .iter()
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty())
        .collect();
    (!instructions.is_empty()).then(|| {
        format!(
            "{SYSTEM_START}\n{}\n{SYSTEM_END}",
            instructions.join("\n\n")
        )
    })
}

/// The newest user message (the request must end with one).
pub(crate) fn last_user_text(req: &CanonicalRequest) -> Result<String, String> {
    match req.messages.last() {
        Some(m) if m.role == Role::User => message_text(m),
        _ => Err("the last message must be a user message".into()),
    }
}

/// The whole request as one message: system instructions, then the history with role
/// labels when there is more than one message.
pub fn flatten(req: &CanonicalRequest) -> Result<String, String> {
    last_user_text(req)?;
    let mut blocks: Vec<String> = system_block(req).into_iter().collect();
    if req.messages.len() == 1 {
        blocks.push(message_text(&req.messages[0])?);
    } else {
        for m in &req.messages {
            let label = match m.role {
                Role::User => "[User]",
                Role::Assistant => "[Assistant]",
            };
            blocks.push(format!("{label}\n{}", message_text(m)?));
        }
    }
    Ok(blocks.join("\n\n"))
}

/// Request parameters a chat page cannot honour (ExecPlan 017 §5.3). `max_output_tokens`
/// is accepted and not enforced (Messages requires it).
pub(crate) fn unsupported_parameter(req: &CanonicalRequest) -> Option<&'static str> {
    if !req.tools.is_empty() || req.tool_choice.is_some() {
        Some("tools")
    } else if req.temperature.is_some() {
        Some("temperature")
    } else if req.top_p.is_some() {
        Some("top_p")
    } else if !req.stop.is_empty() {
        Some("stop")
    } else if req.reasoning.is_some() {
        Some("reasoning")
    } else if req.response_format.is_some() {
        Some("response_format")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use loadout_protocol::Message;

    use super::*;

    fn req(instructions: &[&str], messages: Vec<Message>) -> CanonicalRequest {
        CanonicalRequest {
            model: "m".into(),
            instructions: instructions.iter().map(|s| (*s).to_owned()).collect(),
            messages,
            ..CanonicalRequest::default()
        }
    }

    #[test]
    fn flattening_labels_history_and_wraps_system_text() {
        let single = req(&["Be brief."], vec![Message::text(Role::User, "Hi")]);
        assert_eq!(
            flatten(&single).expect("flat"),
            "[System instructions from the API caller]\nBe brief.\n[End of system instructions]\n\nHi"
        );
        let history = req(
            &[],
            vec![
                Message::text(Role::User, "Q1"),
                Message::text(Role::Assistant, "A1"),
                Message::text(Role::User, "Q2"),
            ],
        );
        assert_eq!(
            flatten(&history).expect("flat"),
            "[User]\nQ1\n\n[Assistant]\nA1\n\n[User]\nQ2"
        );
        assert_eq!(last_user_text(&history).expect("last"), "Q2");
    }

    #[test]
    fn requests_must_end_with_user_text() {
        let ends_with_assistant = req(
            &[],
            vec![
                Message::text(Role::User, "Q"),
                Message::text(Role::Assistant, "A"),
            ],
        );
        assert!(flatten(&ends_with_assistant).is_err());
        let tool = req(
            &[],
            vec![Message::new(
                Role::User,
                vec![Part::ToolResult {
                    call_id: "c".into(),
                    content: Vec::new(),
                    is_error: false,
                }],
            )],
        );
        assert!(last_user_text(&tool).is_err());
    }

    #[test]
    fn sampling_parameters_are_reported_but_max_tokens_is_not() {
        let mut r = req(&[], vec![Message::text(Role::User, "x")]);
        r.max_output_tokens = Some(100);
        assert_eq!(unsupported_parameter(&r), None);
        r.temperature = Some(0.2);
        assert_eq!(unsupported_parameter(&r), Some("temperature"));
    }
}
