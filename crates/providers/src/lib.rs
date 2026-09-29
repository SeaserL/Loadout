// SPDX-License-Identifier: Apache-2.0

//! Built-in Provider modules (ADR-005, ADR-007: compiled in, no dynamic plugins).
//!
//! Provider-specific behaviour stays inside its module; shared code only sees
//! [`ProviderModule`] and the descriptor.

mod anthropic_compatible;
mod claude_code;
mod codex;
mod commandcode;
mod common;
mod deepseek;
mod gemini_cli;
mod kimi_code;
mod official;
mod openai_compatible;
mod opencode_go;
mod openrouter;
mod registry;

pub use anthropic_compatible::AnthropicCompatible;
pub use claude_code::ClaudeCode;
pub use codex::Codex;
pub use commandcode::CommandCode;
pub use deepseek::DeepSeek;
pub use gemini_cli::GeminiCli;
pub use kimi_code::KimiCode;
pub use official::{Anthropic, Ollama, OpenAi};
pub use openai_compatible::OpenAiCompatible;
pub use opencode_go::OpenCodeGo;
pub use openrouter::OpenRouter;
pub use registry::Registry;

#[cfg(test)]
pub(crate) mod testutil;

pub use loadout_provider_sdk::ProviderModule;
